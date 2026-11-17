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
    radios: watch::Receiver<[Radio; srs_client::RADIO_COUNT]>,
    details: watch::Receiver<srs_toc::observation::SessionDetails>,
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
        Self::with_channels(held_before_ready, 1).await
    }
    async fn with_channels(held_before_ready: bool, channels: u16) -> Self {
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
        let (mut playback, output, output_signal) = playback_bridge(channels).unwrap();
        let (mut owner, inputs) = srs_toc::controls();
        if held_before_ready {
            owner.press(1).unwrap();
        }
        let (status_tx, status) = srs_toc::status_channel();
        let (radios_tx, radios) = watch::channel(disabled_radios());
        let (details_tx, details) = watch::channel(srs_toc::observation::SessionDetails::default());
        let task = tokio::spawn(async move {
            srs_toc::run_with_views(
                session,
                &mut capture,
                &mut playback,
                inputs,
                status_tx,
                Some(radios_tx),
                Some(details_tx),
            )
            .await
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
            radios,
            details,
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
async fn radio_observer_tracks_server_policy_adjustments() {
    let mut h = Harness::new(false).await;
    assert_eq!(h.radios.borrow()[1].frequency_hz, 251e6);
    h.owner
        .as_mut()
        .unwrap()
        .change_radio(srs_client::commands::RadioChange::Key {
            slot: 1,
            key: Some(7),
        })
        .await
        .unwrap();
    assert_eq!(h.radios.borrow()[1].encryption_key, Some(7));
    send(
        &mut h.tcp,
        json!({"MsgType":4,"ServerSettings":{"ALLOW_RADIO_ENCRYPTION":"false"}}),
    )
    .await;
    timeout(Duration::from_secs(3), async {
        loop {
            if h.radios.borrow_and_update()[1].encryption_key.is_none() {
                break;
            }
            h.radios.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(h.radios.borrow()[1].frequency_hz, 251e6);
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
    assert_eq!(h.radios.borrow()[1].frequency_hz, 252e6);
    assert_eq!(h.radios.borrow()[1].modulation, Modulation::FM);
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

#[tokio::test]
async fn complete_preset_releases_held_ptt_and_publishes_one_validated_update() {
    use srs_client::commands::RadioEdit;
    let mut h = Harness::new(false).await;
    h.press().await;
    h.feed(2).await;
    let mut preset = disabled_radios();
    preset[1] = Radio {
        name: "Command Net".into(),
        ..Radio::tuned(252e6, Modulation::FM)
    };
    preset[2] = Radio {
        guard_hz: 243e6,
        encryption_key: Some(7),
        ..Radio::tuned(253e6, Modulation::AM)
    };
    let snapshot = h
        .owner
        .as_mut()
        .unwrap()
        .edit_radios(RadioEdit::Replace(Box::new(preset.clone())))
        .await
        .unwrap();
    assert!(snapshot == preset);
    h.phase(Phase::Receiving).await;
    assert!(*h.radios.borrow() == preset);
    let update = read(&mut h.tcp).await;
    assert_eq!(update["MsgType"], 3);
    let wire = &update["Client"]["RadioInfo"]["radios"];
    assert_eq!(wire[1]["freq"], 252e6);
    assert_eq!(wire[1]["Name"], "commandnet");
    assert_eq!(wire[2]["freq"], 253e6);
    assert_eq!(wire[2]["encKey"], 7);
    assert!(
        timeout(Duration::from_millis(30), h.tcp.fill_buf())
            .await
            .is_err(),
        "preset emitted more than one update"
    );
    assert_eq!(h.owner.as_mut().unwrap().renew(), Err(Error::InputExpired));
    h.feed(4).await;
    let mut bytes = [0; 8192];
    assert!(
        h.udp.try_recv_from(&mut bytes).is_err(),
        "old held audio crossed preset change"
    );
    h.owner.as_mut().unwrap().press(2).unwrap();
    h.phase(Phase::Transmitting).await;
    h.feed(4).await;
    let bytes = h.packet().await;
    let packet = VoicePacket::decode(&bytes).unwrap();
    assert_eq!(packet.frequencies[0].hz, 253e6);
    assert_eq!(packet.frequencies[0].encryption, 7);
    h.stop().await;
}

#[tokio::test]
async fn monitor_mute_flushes_partial_playback_and_tracks_muted_rx_without_tcp_edits() {
    use srs_client::receive::{DEFAULT_MONITOR, RadioLevel};
    let mut h = Harness::new(false).await;
    h.remote_frame(1, b'B').await;
    timeout(Duration::from_secs(3), async {
        while h.status.borrow_and_update().received_frames[1] == 0 {
            h.status.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    let mut pcm = [0f32; 480];
    h.output.process(&mut pcm, Instant::now().into_std());
    assert!(pcm.iter().any(|n| n.abs() > 0.05));
    let mut levels = DEFAULT_MONITOR;
    levels[1] = RadioLevel::new(35, true).unwrap();
    h.owner.as_mut().unwrap().set_monitor(levels);
    sleep(Duration::from_millis(15)).await;
    pcm.fill(1.);
    h.output.process(&mut pcm, Instant::now().into_std());
    assert!(
        pcm.iter().all(|n| *n == 0.),
        "mute must discard the partial native frame"
    );
    let count = h.status.borrow().received_frames[1];
    // A new original speaker avoids depending on real-time PLC sequence timing.
    h.remote_frame(1, b'C').await;
    timeout(Duration::from_secs(3), async {
        while h.status.borrow_and_update().received_frames[1] == count {
            h.status.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(h.status.borrow().received_frames[2], 0);
    pcm.fill(1.);
    h.output.process(&mut pcm, Instant::now().into_std());
    assert!(pcm.iter().all(|n| *n == 0.), "muted RX must stay silent");
    assert!(
        timeout(Duration::from_millis(20), h.tcp.fill_buf())
            .await
            .is_err(),
        "local monitor change wrote to TCP"
    );
    let count = h.status.borrow().received_frames[1];
    h.owner.as_mut().unwrap().set_monitor(DEFAULT_MONITOR);
    h.remote_frame(1, b'D').await;
    // Unmuting may expose a remaining C concealment tail before D's new frame.
    // Keep consuming 10 ms callbacks while D traverses its jitter delay, as a
    // real output device does. One callback is not a promise of new voice.
    timeout(Duration::from_secs(3), async {
        loop {
            h.output.process(&mut pcm, Instant::now().into_std());
            if h.status.borrow().received_frames[1] > count && pcm.iter().any(|n| n.abs() > 0.05) {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(h.status.borrow().playback_overflows, 0);
    h.stop().await;
}

#[tokio::test]
async fn session_details_follow_roster_and_policy_without_private_fields() {
    let mut h = Harness::new(false).await;
    assert_eq!(h.details.borrow().version, "2.4.1.0");
    assert_eq!(
        h.details.borrow().coalition,
        Some(srs_client::Coalition::Blue)
    );
    let peers: Vec<_> = (0..257).map(|index| json!({"ClientGuid":format!("{index:022}"),"Name":"Alpha\n\u{202e}Bravo", "Coalition":2, "LatLngPosition":{"lat":1.0}, "Private":"sensitive"})).collect();
    send(&mut h.tcp, json!({"MsgType":2,"Version":"2.4.1.0","Clients":peers,
        "ServerSettings":{"EXTERNAL_AWACS_MODE":"true", "ALLOW_RADIO_ENCRYPTION":"FALSE", "SHOW_TUNED_COUNT":"nonsense", "RETRANSMISSION_NODE_LIMIT":"3", "ADMIN_PASSWORD":"never-display"}})).await;
    timeout(Duration::from_secs(3), async {
        loop {
            if h.details.borrow_and_update().total_clients == 257 {
                break;
            }
            h.details.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    {
        let details = h.details.borrow();
        assert_eq!(details.clients.len(), 256);
        assert_eq!(details.clients[0].name, "AlphaBravo");
        assert_eq!(details.clients[0].coalition, Some(2));
        assert_eq!(details.policy[8], Some(false));
        assert_eq!(details.policy[10], None);
        assert_eq!(details.retransmission_limit, Some(3));
        let display = format!("{details:?}");
        for excluded in ["sensitive", "never-display", "LatLngPosition", "ClientGuid"] {
            assert!(!display.contains(excluded));
        }
    }
    h.stop().await;
}

#[tokio::test]
async fn stereo_worker_routes_and_flushes_partial_output_and_mono_keeps_right_radio() {
    use srs_client::receive::{DEFAULT_MONITOR, RadioLevel, Routing};
    for channels in [1, 2] {
        let mut h = Harness::with_channels(false, channels).await;
        let mut levels = DEFAULT_MONITOR;
        levels[1] = RadioLevel::FULL.with_routing(Routing::Right);
        h.owner.as_mut().unwrap().set_monitor(levels);
        h.remote_frame(1, b'B').await;
        timeout(Duration::from_secs(3), async {
            while h.status.borrow_and_update().received_frames[1] == 0 {
                h.status.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        let mut pcm = vec![0_f32; 480 * channels as usize];
        h.output.process(&mut pcm, Instant::now().into_std());
        assert!(pcm.iter().any(|s| s.abs() > 0.05));
        if channels == 2 {
            assert!(pcm.as_chunks::<2>().0.iter().all(|p| p[0] == 0.0));
        }
        levels[1] = RadioLevel::FULL.with_routing(Routing::Left);
        h.status.borrow_and_update();
        h.owner.as_mut().unwrap().set_monitor(levels);
        // In this receive-only worker, each publication follows processing the
        // latest monitor command. Await that boundary rather than an OS sleep.
        timeout(Duration::from_secs(3), h.status.changed())
            .await
            .unwrap()
            .unwrap();
        // Old partial right-channel audio must be invalidated before the next callback.
        h.output.process(&mut pcm, Instant::now().into_std());
        if channels == 2 {
            assert!(pcm.as_chunks::<2>().0.iter().all(|p| p[1] == 0.0));
        }
        let received = h.status.borrow().received_frames[1];
        h.remote_frame(1, b'C').await;
        timeout(Duration::from_secs(3), async {
            loop {
                h.output.process(&mut pcm, Instant::now().into_std());
                if channels == 2 {
                    assert!(pcm.as_chunks::<2>().0.iter().all(|p| p[1] == 0.0));
                }
                if h.status.borrow().received_frames[1] > received
                    && pcm.iter().any(|s| s.abs() > 0.05)
                {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert!(
            timeout(Duration::from_millis(20), h.tcp.fill_buf())
                .await
                .is_err(),
            "routing wrote to TCP"
        );
        assert_eq!(h.status.borrow().playback_overflows, 0);
        h.stop().await;
    }
}

#[tokio::test]
async fn worker_suspension_stops_audio_even_if_input_renews_on_resume() {
    let mut h = Harness::new(false).await;
    h.press().await;
    h.feed(1).await;
    // Suspend this owned runtime only. No system sleep or shared process signal.
    std::thread::sleep(Duration::from_millis(350));
    let _ = h.owner.as_mut().unwrap().renew();
    assert!(
        timeout(Duration::from_secs(3), &mut h.task)
            .await
            .unwrap()
            .unwrap()
            .is_err()
    );
    assert!(h.input_signal.is_stopped() && h.output_signal.is_stopped());
    assert_eq!(h.status.borrow().phase, Phase::Stopped);
    let mut pcm = [1f32; 480];
    h.output.process(&mut pcm, Instant::now().into_std());
    assert!(pcm.iter().all(|sample| *sample == 0.0));
}
