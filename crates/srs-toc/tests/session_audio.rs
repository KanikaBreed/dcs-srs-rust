//! Loopback TCP/UDP and synthetic callbacks only. No physical devices.
use serde_json::{Value, json};
use srs_audio::{Decoder, Encoder, MAX_OPUS_BYTES, OUTPUT_SAMPLES};
use srs_client::{Identity, Options, Radio, Session, disabled_radios};
use srs_device::bridge::{
    CaptureCallback, PlaybackCallback, StopSignal, capture_bridge, playback_bridge,
};
use srs_protocol::voice::{ClientId, Frequency, Modulation, VoicePacket};
use srs_toc::{Controller, Error, Phase, Status};
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream, UdpSocket},
    sync::watch,
    task::JoinHandle,
    time::{Instant, sleep, timeout},
};
const ID: &[u8] = b"TTTTTTTTTTTTTTTTTTTTTT";
async fn read(peer: &mut BufReader<TcpStream>) -> Value {
    let mut text = String::new();
    timeout(Duration::from_secs(3), peer.read_line(&mut text))
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str(&text).unwrap()
}
async fn send(peer: &mut BufReader<TcpStream>, value: Value) {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    peer.get_mut().write_all(&bytes).await.unwrap();
}
struct Harness {
    owner: Option<Controller>,
    status: watch::Receiver<Status>,
    task: JoinHandle<Result<Status, Error>>,
    input: CaptureCallback,
    output: PlaybackCallback,
    input_signal: StopSignal,
    output_signal: StopSignal,
    tcp: BufReader<TcpStream>,
    udp: UdpSocket,
    address: SocketAddr,
}
impl Harness {
    async fn new(held_before_ready: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let udp = UdpSocket::bind(address).await.unwrap();
        let accept = async {
            let mut peer = BufReader::new(listener.accept().await.unwrap().0);
            assert_eq!(read(&mut peer).await["MsgType"], 2);
            send(&mut peer, json!({"MsgType":2,"Version":"2.4.1.0","Clients":[],"ServerSettings":{"EXTERNAL_AWACS_MODE":"true"}})).await;
            assert_eq!(read(&mut peer).await["MsgType"], 7);
            send(&mut peer, json!({"MsgType":7,"Client":{"Coalition":2}})).await;
            assert_eq!(read(&mut peer).await["MsgType"], 3);
            peer
        };
        let connect = async {
            let mut s = Session::connect(
                address,
                Identity::new(ClientId::from_bytes(ID).unwrap(), "Generated TOC test"),
                Options::default(),
            )
            .await
            .unwrap();
            s.authenticate("fixture").await.unwrap();
            let mut radios = disabled_radios();
            radios[1] = Radio::tuned(251e6, Modulation::AM);
            s.set_radios(radios).await.unwrap();
            s
        };
        let (tcp, session) = tokio::join!(accept, connect);
        let (input, mut capture, input_signal) =
            capture_bridge(1, Instant::now().into_std()).unwrap();
        let (mut playback, output, output_signal) = playback_bridge(1).unwrap();
        let (mut owner, inputs) = srs_toc::controls();
        if held_before_ready {
            owner.press(1).unwrap();
        }
        let (status_tx, status) = srs_toc::status_channel();
        let task = tokio::spawn(async move {
            srs_toc::run(session, &mut capture, &mut playback, inputs, status_tx).await
        });
        let mut bytes = [0; 8192];
        let (n, address) = timeout(Duration::from_secs(3), udp.recv_from(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&bytes[..n], ID);
        udp.send_to(&bytes[..n], address).await.unwrap();
        let mut h = Self {
            owner: Some(owner),
            status,
            task,
            input,
            output,
            input_signal,
            output_signal,
            tcp,
            udp,
            address,
        };
        h.phase(Phase::Receiving).await;
        h
    }
    async fn phase(&mut self, phase: Phase) {
        timeout(Duration::from_secs(3), async {
            loop {
                if self.status.borrow_and_update().phase == phase {
                    break;
                }
                self.status.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
    }
    async fn press(&mut self) {
        self.owner.as_mut().unwrap().press(1).unwrap();
        self.phase(Phase::Transmitting).await;
    }
    async fn feed(&mut self, blocks: usize) {
        let pcm: [f32; 480] = std::array::from_fn(|i| {
            0.3 * (std::f32::consts::TAU * 700.0 * i as f32 / 48000.0).sin()
        });
        for _ in 0..blocks {
            self.input.process(&pcm, Instant::now().into_std());
            sleep(Duration::from_millis(10)).await;
        }
    }
    async fn packet(&self) -> Vec<u8> {
        let mut bytes = [0; 8192];
        let (n, _) = timeout(Duration::from_secs(3), self.udp.recv_from(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        bytes[..n].to_vec()
    }
    async fn stop(mut self) {
        drop(self.owner.take());
        timeout(Duration::from_secs(3), &mut self.task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert!(self.input_signal.is_stopped() && self.output_signal.is_stopped());
        assert_eq!(self.status.borrow().phase, Phase::Stopped);
    }
    async fn remote_frame(&self, sequence: u64, speaker: u8) {
        let mut encoder = Encoder::new().unwrap();
        let pcm = std::array::from_fn(|i| {
            (12000.0 * (std::f64::consts::TAU * 700.0 * i as f64 / 16000.0).sin()) as i16
        });
        let mut encoded = [0; MAX_OPUS_BYTES];
        let n = encoder.encode(&pcm, &mut encoded).unwrap();
        let id = ClientId::from_bytes(&[speaker; 22]).unwrap();
        let packet = VoicePacket {
            audio: &encoded[..n],
            frequencies: vec![Frequency {
                hz: 251e6,
                modulation: Modulation::AM,
                encryption: 0,
            }],
            unit_id: 100000001,
            packet_number: sequence,
            retransmission_count: 0,
            original_client_id: id,
            client_id: id,
        }
        .encode()
        .unwrap();
        self.udp.send_to(&packet, self.address).await.unwrap();
    }
}
impl Drop for Harness {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn capture_reaches_wire_only_during_fresh_ptt_and_repress_discards_partial_audio() {
    let mut h = Harness::new(true).await;
    h.feed(4).await;
    let mut byte = [0; 8192];
    assert!(h.udp.try_recv_from(&mut byte).is_err());
    h.press().await;
    h.feed(4).await;
    let packet = h.packet().await;
    let wire = VoicePacket::decode(&packet).unwrap();
    assert_eq!(wire.client_id.as_bytes(), ID);
    assert_eq!(wire.frequencies[0].hz, 251e6);
    let mut decoder = Decoder::new().unwrap();
    let mut pcm = [0; OUTPUT_SAMPLES];
    decoder.decode(wire.audio, &mut pcm).unwrap();
    assert!(pcm.iter().any(|n| n.unsigned_abs() > 1000));
    h.feed(2).await;
    h.owner.as_mut().unwrap().release().unwrap();
    h.phase(Phase::Receiving).await;
    h.press().await;
    h.feed(2).await;
    assert!(
        h.udp.try_recv_from(&mut byte).is_err(),
        "old partial capture crossed presses"
    );
    h.feed(2).await;
    let packet = h.packet().await;
    assert_eq!(VoicePacket::decode(&packet).unwrap().packet_number, 2);
    h.stop().await;
}
#[tokio::test]
async fn lease_expires_without_reviving_on_renew_and_stall_stops_both_devices() {
    let mut h = Harness::new(false).await;
    h.press().await;
    h.feed(28).await; // callbacks remain healthy, input owner does not renew
    assert_eq!(h.status.borrow().phase, Phase::Receiving);
    assert_eq!(h.owner.as_mut().unwrap().renew(), Err(Error::InputExpired));
    h.press().await;
    for _ in 0..4 {
        sleep(Duration::from_millis(90)).await;
        h.owner.as_mut().unwrap().renew().unwrap();
    }
    assert!(
        timeout(Duration::from_secs(3), &mut h.task)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert!(h.input_signal.is_stopped() && h.output_signal.is_stopped());
}
#[tokio::test]
async fn policy_change_releases_capture_and_held_renewal_cannot_restart_it() {
    let mut h = Harness::new(false).await;
    h.press().await;
    h.feed(2).await;
    send(
        &mut h.tcp,
        json!({"MsgType":4,"ServerSettings":{"IRL_RADIO_TX":"true"}}),
    )
    .await;
    h.phase(Phase::Receiving).await;
    h.owner.as_mut().unwrap().renew().unwrap();
    h.feed(4).await;
    let mut bytes = [0; 8192];
    assert!(h.udp.try_recv_from(&mut bytes).is_err());
    h.press().await;
    h.feed(4).await;
    h.packet().await;
    h.stop().await;
}
#[tokio::test]
async fn received_speakers_mix_and_policy_revocation_flushes_partial_playback() {
    let mut h = Harness::new(false).await;
    h.remote_frame(1, b'A').await;
    h.remote_frame(1, b'B').await;
    timeout(Duration::from_secs(3), async {
        while h.status.borrow_and_update().peak_speakers < 2 {
            h.status.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    let mut pcm = [0f32; 480];
    h.output.process(&mut pcm, Instant::now().into_std());
    assert!(pcm.iter().any(|n| n.abs() > 0.05));
    send(
        &mut h.tcp,
        json!({"MsgType":4,"ServerSettings":{"IRL_RADIO_TX":"true"}}),
    )
    .await;
    // Processing the settings update clears authorization even while TCP stays open.
    sleep(Duration::from_millis(30)).await;
    pcm.fill(1.0);
    h.output.process(&mut pcm, Instant::now().into_std());
    assert!(pcm.iter().all(|n| *n == 0.0));
    h.stop().await;
}
#[tokio::test]
async fn tcp_loss_device_failure_and_future_cancellation_stop_capture_and_playback() {
    for case in 0..5 {
        let mut h = Harness::new(false).await;
        h.press().await;
        h.feed(1).await;
        match case {
            0 => h.tcp.get_mut().shutdown().await.unwrap(),
            1 => h.input_signal.stop(),
            2 => h.output_signal.stop(),
            3 => h.task.abort(),
            _ => {
                send(
                    &mut h.tcp,
                    json!({"MsgType":4,"ServerSettings":{"EXTERNAL_AWACS_MODE":"false"}}),
                )
                .await
            }
        }
        let result = timeout(Duration::from_secs(3), &mut h.task).await.unwrap();
        assert!(result.is_err() || result.unwrap().is_err());
        assert!(h.input_signal.is_stopped() && h.output_signal.is_stopped());
        assert_eq!(h.status.borrow().phase, Phase::Stopped);
        let mut pcm = [1f32; 480];
        h.output.process(&mut pcm, Instant::now().into_std());
        assert!(pcm.iter().all(|n| *n == 0.0));
    }
}

#[tokio::test]
async fn retuning_held_transmit_requires_new_press_and_new_wire_channel() {
    use srs_client::commands::RadioChange;
    let mut h = Harness::new(false).await;
    h.press().await;
    h.feed(2).await;
    let radios = h
        .owner
        .as_mut()
        .unwrap()
        .change_radio(RadioChange::Tune {
            slot: 1,
            hz: 252e6,
            modulation: Modulation::FM,
        })
        .await
        .unwrap();
    assert_eq!(radios[1].frequency_hz, 252e6);
    h.phase(Phase::Receiving).await;
    h.feed(4).await;
    let mut bytes = [0; 8192];
    assert!(h.udp.try_recv_from(&mut bytes).is_err());
    // Exercise an immediate press after another write acknowledgement; the
    // worker must not swallow it while observing that local policy revision.
    h.owner
        .as_mut()
        .unwrap()
        .change_radio(RadioChange::Tune {
            slot: 2,
            hz: 253e6,
            modulation: Modulation::AM,
        })
        .await
        .unwrap();
    h.owner.as_mut().unwrap().press(2).unwrap();
    h.phase(Phase::Transmitting).await;
    h.feed(4).await;
    let bytes = h.packet().await;
    let packet = VoicePacket::decode(&bytes).unwrap();
    assert_eq!(packet.frequencies[0].hz, 253e6);
    assert_eq!(packet.frequencies[0].modulation, Modulation::AM);
    h.stop().await;
}
#[tokio::test]
async fn cancelled_radio_edit_inhibits_press_until_a_new_edit_is_acknowledged() {
    let (mut owner, _events) = srs_toc::controls();
    let change = srs_client::commands::RadioChange::Disable { slot: 1 };
    assert!(
        timeout(Duration::from_millis(2), owner.change_radio(change))
            .await
            .is_err()
    );
    assert_eq!(owner.press(1), Err(Error::InvalidInput));
    assert_eq!(owner.renew(), Err(Error::InputExpired));
}

#[tokio::test]
async fn rejected_edit_does_not_clear_an_abandoned_edits_input_inhibition() {
    use srs_client::commands::RadioChange;
    let mut h = Harness::new(false).await;
    send(
        &mut h.tcp,
        json!({"MsgType":4,"ServerSettings":{"ALLOW_RADIO_ENCRYPTION":"false"}}),
    )
    .await;
    sleep(Duration::from_millis(20)).await;
    // Poll the request once, then abandon it before the independent worker can
    // run. This checks cancellation without depending on timer granularity.
    let mut request = Box::pin(
        h.owner
            .as_mut()
            .unwrap()
            .change_radio(RadioChange::Disable { slot: 2 }),
    );
    std::future::poll_fn(|cx| {
        assert!(request.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    drop(request);
    assert!(matches!(
        h.owner
            .as_mut()
            .unwrap()
            .change_radio(RadioChange::Key {
                slot: 1,
                key: Some(7)
            })
            .await,
        Err(Error::Control(srs_client::Error::PolicyDenied))
    ));
    assert_eq!(h.owner.as_mut().unwrap().press(1), Err(Error::InvalidInput));
    h.owner
        .as_mut()
        .unwrap()
        .change_radio(RadioChange::Tune {
            slot: 1,
            hz: 251e6,
            modulation: Modulation::AM,
        })
        .await
        .unwrap();
    h.press().await;
    h.feed(4).await;
    h.packet().await;
    h.stop().await;
}
