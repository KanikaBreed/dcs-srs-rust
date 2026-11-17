use super::*;
use srs_protocol::voice::Modulation;
use tokio::time::timeout;

fn id() -> ClientId {
    ClientId::from_bytes(b"AAAAAAAAAAAAAAAAAAAAAA").unwrap()
}
fn authorization() -> Authorization {
    let mut auth = Authorization::disabled();
    auth.revision = 1;
    auth.channels[1] = Some(Channel {
        frequency: Frequency {
            hz: 251_000_000.0,
            modulation: Modulation::AM,
            encryption: 7,
        },
        unit_id: 100_000_001,
    });
    let mut radios = [srs_core::ReceiveRadio::default(); 11];
    radios[1] = srs_core::ReceiveRadio {
        frequency_hz: 251_000_000.0,
        encryption_key: Some(7),
        modulation: Modulation::AM,
        ..srs_core::ReceiveRadio::default()
    };
    auth.receive = Some(std::sync::Arc::new(crate::receive::Policy {
        radios,
        unit: 100_000_001,
        strict: false,
        irl_tx: false,
        global: vec![],
    }));
    auth
}
async fn registered() -> (
    VoiceConnection,
    UdpSocket,
    SocketAddr,
    watch::Sender<Authorization>,
    watch::Sender<bool>,
) {
    let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let echo = tokio::spawn(async move {
        let mut bytes = [0; 65536];
        let (n, peer) = server.recv_from(&mut bytes).await.unwrap();
        assert_eq!(&bytes[..n], id().as_bytes());
        server.send_to(&bytes[..n], peer).await.unwrap();
        (server, peer)
    });
    let (auth, rx) = watch::channel(authorization());
    let (live, live_rx) = watch::channel(true);
    let connection = VoiceConnection::register(address, id(), rx, live_rx, Duration::from_secs(2))
        .await
        .unwrap();
    let (server, peer) = echo.await.unwrap();
    (connection, server, peer, auth, live)
}

#[tokio::test]
async fn transmit_requires_ptt_and_preserves_wire_identity_radio_and_sequence() {
    let (mut voice, server, _, _auth, _live) = registered().await;
    assert_eq!(
        voice.send_frame(&[1, 2]).await,
        Err(VoiceError::NotTransmitting)
    );
    assert_eq!(voice.press_ptt(0), Err(VoiceError::InvalidRadio));
    voice.press_ptt(1).unwrap();
    for sequence in 1..=2 {
        assert_eq!(voice.send_frame(&[1, 2]).await.unwrap(), sequence);
        let mut bytes = [0; 8192];
        let (n, _) = server.recv_from(&mut bytes).await.unwrap();
        let packet = VoicePacket::decode(&bytes[..n]).unwrap();
        assert_eq!(packet.packet_number, sequence);
        assert_eq!(packet.original_client_id, id());
        assert_eq!(packet.client_id, id());
        assert_eq!(packet.unit_id, 100_000_001);
        assert_eq!(packet.retransmission_count, 0);
        assert_eq!(packet.frequencies[0].hz, 251_000_000.0);
        assert_eq!(packet.frequencies[0].encryption, 7);
    }
    voice.release_ptt();
    assert!(!voice.is_transmitting());
    assert_eq!(
        voice.send_frame(&[1]).await,
        Err(VoiceError::NotTransmitting)
    );
}

#[tokio::test]
async fn authorization_changes_require_a_fresh_press_and_control_close_invalidates_immediately() {
    let (mut voice, _server, _, auth, live) = registered().await;
    voice.press_ptt(1).unwrap();
    let mut changed = authorization();
    changed.revision = 2;
    auth.send_replace(changed);
    assert!(!voice.is_transmitting());
    assert_eq!(
        voice.send_frame(&[1]).await,
        Err(VoiceError::NotTransmitting)
    );
    voice.press_ptt(1).unwrap();
    assert!(voice.is_transmitting());
    live.send_replace(false);
    assert!(!voice.is_transmitting());
    assert!(!voice.is_ready());
    assert_eq!(
        voice.send_frame(&[1]).await,
        Err(VoiceError::ControlDisconnected)
    );
    assert!(matches!(
        voice.next_event().await,
        Err(VoiceError::ControlDisconnected)
    ));
}

#[tokio::test]
async fn receive_is_cancel_safe_and_policy_notifications_release_ptt() {
    let (mut voice, _server, _, auth, _live) = registered().await;
    voice.press_ptt(1).unwrap();
    assert!(
        timeout(Duration::from_millis(20), voice.next_event())
            .await
            .is_err()
    );
    assert!(voice.is_ready());
    assert!(voice.is_transmitting());
    let mut disabled = Authorization::disabled();
    disabled.revision = 2;
    auth.send_replace(disabled);
    assert!(matches!(
        voice.next_event().await.unwrap(),
        VoiceEvent::AuthorizationChanged
    ));
    assert!(!voice.is_transmitting());
    assert_eq!(voice.press_ptt(1), Err(VoiceError::InvalidRadio));
}

#[tokio::test]
async fn keepalive_identity_and_datagram_bounds_are_checked_before_liveness() {
    let (mut voice, server, peer, _auth, _live) = registered().await;
    let old = voice.last_received;
    for bytes in [
        b"BBBBBBBBBBBBBBBBBBBBBB".to_vec(),
        vec![0; 4],
        vec![0; MAX_VOICE_DATAGRAM + 1],
    ] {
        server.send_to(&bytes, peer).await.unwrap();
        assert!(matches!(
            voice.next_event().await.unwrap(),
            VoiceEvent::Rejected
        ));
        assert_eq!(voice.last_received, old);
    }
    let packet = VoicePacket {
        audio: &[1, 2, 3],
        frequencies: vec![authorization().channels[1].unwrap().frequency],
        unit_id: 100_000_001,
        packet_number: 5,
        retransmission_count: 0,
        original_client_id: id(),
        client_id: id(),
    };
    server
        .send_to(&packet.encode().unwrap(), peer)
        .await
        .unwrap();
    let VoiceEvent::Voice(frame) = voice.next_event().await.unwrap() else {
        panic!("expected voice")
    };
    assert_eq!(frame.audio, [1, 2, 3]);
    assert_eq!(frame.packet_number, 5);
    assert!(voice.last_received >= old);
}

#[tokio::test]
async fn keepalive_and_timeout_follow_upstream_intervals() {
    let (mut voice, server, _, _auth, _live) = registered().await;
    tokio::time::pause();
    tokio::time::advance(KEEPALIVE_INTERVAL).await;
    tokio::time::resume();
    let echo = tokio::spawn(async move {
        let mut bytes = [0; 22];
        let (n, peer) = server.recv_from(&mut bytes).await.unwrap();
        assert_eq!(n, 22);
        assert_eq!(&bytes, id().as_bytes());
        server.send_to(&bytes, peer).await.unwrap();
    });
    assert!(matches!(
        voice.next_event().await.unwrap(),
        VoiceEvent::Keepalive
    ));
    echo.await.unwrap();
    voice.press_ptt(1).unwrap();
    tokio::time::pause();
    tokio::time::advance(VOICE_TIMEOUT).await;
    tokio::time::resume();
    assert!(!voice.is_ready());
    assert!(!voice.is_transmitting());
    assert!(matches!(voice.next_event().await, Err(VoiceError::Timeout)));
}

#[tokio::test]
async fn registration_requires_matching_echo_and_cannot_be_kept_alive_by_junk() {
    let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let echo = tokio::spawn(async move {
        let mut bytes = [0; 22];
        let (_, peer) = server.recv_from(&mut bytes).await.unwrap();
        server
            .send_to(b"BBBBBBBBBBBBBBBBBBBBBB", peer)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
    });
    let (_auth, rx) = watch::channel(authorization());
    let (_live, live_rx) = watch::channel(true);
    assert!(matches!(
        VoiceConnection::register(address, id(), rx, live_rx, Duration::from_millis(100)).await,
        Err(VoiceError::Timeout)
    ));
    echo.await.unwrap();
}

#[tokio::test]
async fn invalid_audio_and_sequence_exhaustion_release_ptt() {
    let (mut voice, _server, _, _auth, _live) = registered().await;
    for bytes in [vec![], vec![0; MAX_AUDIO_BYTES + 1]] {
        voice.press_ptt(1).unwrap();
        assert_eq!(
            voice.send_frame(&bytes).await,
            Err(VoiceError::InvalidAudio)
        );
        assert!(!voice.is_transmitting());
    }
    voice.sequence = u64::MAX;
    voice.press_ptt(1).unwrap();
    assert_eq!(
        voice.send_frame(&[1]).await,
        Err(VoiceError::SequenceExhausted)
    );
    assert!(!voice.is_transmitting());
}

#[tokio::test]
async fn registration_retries_after_a_lost_initial_keepalive() {
    let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let address = server.local_addr().unwrap();
    let echo = tokio::spawn(async move {
        let mut bytes = [0; 22];
        server.recv_from(&mut bytes).await.unwrap(); // Deliberately drop first ping.
        let (n, peer) = server.recv_from(&mut bytes).await.unwrap();
        assert_eq!(n, 22);
        server.send_to(&bytes, peer).await.unwrap();
    });
    let (_auth, rx) = watch::channel(authorization());
    let (_live, live_rx) = watch::channel(true);
    let voice = VoiceConnection::register(address, id(), rx, live_rx, Duration::from_secs(3))
        .await
        .unwrap();
    assert!(voice.is_ready());
    assert!(!voice.is_transmitting());
    echo.await.unwrap();
}

fn audio_packet(voice: &VoiceConnection, sequence: u64) -> ReceivedVoice {
    let mut encoder = srs_audio::Encoder::new().unwrap();
    let pcm = std::array::from_fn(|i| {
        (12000.0 * (std::f64::consts::TAU * 700.0 * i as f64 / 16000.0).sin()) as i16
    });
    let mut bytes = [0; srs_audio::MAX_OPUS_BYTES];
    let len = encoder.encode(&pcm, &mut bytes).unwrap();
    ReceivedVoice {
        generation: voice.authorization.borrow().revision,
        receiver: voice.id,
        audio: bytes[..len].to_vec(),
        frequencies: vec![Frequency {
            hz: 251_000_000.0,
            modulation: Modulation::AM,
            encryption: 7,
        }],
        unit_id: 100_000_001,
        packet_number: sequence,
        retransmission_count: 0,
        original_client_id: ClientId::from_bytes(b"BBBBBBBBBBBBBBBBBBBBBB").unwrap(),
        client_id: ClientId::from_bytes(b"BBBBBBBBBBBBBBBBBBBBBB").unwrap(),
        received_at: Instant::now(),
    }
}
fn update_policy(
    auth: &watch::Sender<Authorization>,
    change: impl FnOnce(&mut crate::receive::Policy),
) {
    auth.send_modify(|auth| {
        auth.revision += 1;
        change(std::sync::Arc::make_mut(auth.receive.as_mut().unwrap()));
    });
}

#[tokio::test(start_paused = true)]
async fn receive_gate_rejects_wrong_keys_and_modulation_but_allows_guard() {
    use crate::receive::QueueOutcome;
    let (mut voice, _server, _peer, auth, _live) = registered().await;
    let mut packet = audio_packet(&voice, 1);
    packet.frequencies[0].encryption = 8;
    assert_eq!(
        voice.buffer_voice(&packet).unwrap(),
        QueueOutcome::Ineligible
    );
    packet.frequencies[0].encryption = 0;
    assert!(matches!(
        voice.buffer_voice(&packet).unwrap(),
        QueueOutcome::Queued | QueueOutcome::Resynchronized
    ));
    update_policy(&auth, |p| p.strict = true);
    packet.generation = auth.borrow().revision;
    assert_eq!(
        voice.buffer_voice(&packet).unwrap(),
        QueueOutcome::Ineligible
    );
    packet.frequencies[0].encryption = 7;
    packet.frequencies[0].modulation = Modulation::FM;
    assert_eq!(
        voice.buffer_voice(&packet).unwrap(),
        QueueOutcome::Ineligible
    );
    update_policy(&auth, |p| {
        p.radios[1].frequency_hz = 305_000_000.0;
        p.radios[1].guard_hz = 251_000_000.0;
    });
    packet.generation = auth.borrow().revision;
    voice.buffer_voice(&packet).unwrap();
    tokio::time::advance(Duration::from_millis(80)).await;
    let mut pcm = [0; srs_audio::OUTPUT_SAMPLES];
    let frame = voice.poll_audio(&mut pcm).unwrap().unwrap();
    assert!(frame.secondary);
    assert_eq!(frame.radio, 1);
    assert!(pcm.iter().any(|&x| x != 0));
}

#[tokio::test(start_paused = true)]
async fn tune_and_control_revocation_clear_queued_audio_before_playout() {
    let (mut voice, _server, _peer, auth, live) = registered().await;
    let packet = audio_packet(&voice, 1);
    voice.buffer_voice(&packet).unwrap();
    update_policy(&auth, |p| p.radios[1].frequency_hz = 305_000_000.0);
    assert_eq!(
        voice.buffer_voice(&packet).unwrap(),
        crate::receive::QueueOutcome::Stale
    );
    tokio::time::advance(Duration::from_millis(80)).await;
    let mut pcm = [99; srs_audio::OUTPUT_SAMPLES];
    assert!(voice.poll_audio(&mut pcm).unwrap().is_none());
    assert!(pcm.iter().all(|&x| x == 0));
    update_policy(&auth, |p| p.radios[1].frequency_hz = 251_000_000.0);
    voice.buffer_voice(&audio_packet(&voice, 2)).unwrap();
    live.send_replace(false);
    assert_eq!(
        voice.poll_audio(&mut pcm),
        Err(VoiceError::ControlDisconnected)
    );
    assert!(pcm.iter().all(|&x| x == 0));
}

#[tokio::test(start_paused = true)]
async fn transmit_blocking_rechecks_backlog_and_global_exception() {
    let (mut voice, _server, _peer, auth, _live) = registered().await;
    update_policy(&auth, |p| p.irl_tx = true);
    voice.buffer_voice(&audio_packet(&voice, 1)).unwrap();
    voice.press_ptt(1).unwrap();
    voice.release_ptt();
    assert_eq!(voice.audio_deadline().unwrap(), None);
    voice.press_ptt(1).unwrap();
    assert_eq!(
        voice.buffer_voice(&audio_packet(&voice, 2)).unwrap(),
        crate::receive::QueueOutcome::Ineligible
    );
    update_policy(&auth, |p| p.global.push(251_000_000.0));
    voice.press_ptt(1).unwrap();
    assert!(matches!(
        voice.buffer_voice(&audio_packet(&voice, 3)).unwrap(),
        crate::receive::QueueOutcome::Queued | crate::receive::QueueOutcome::Resynchronized
    ));
    // Client global matching is exact, unlike the frequency tolerance.
    let mut packet = audio_packet(&voice, 4);
    packet.frequencies[0].hz += 1.0;
    assert_eq!(
        voice.buffer_voice(&packet).unwrap(),
        crate::receive::QueueOutcome::Ineligible
    );
}

#[tokio::test(start_paused = true)]
async fn duplicate_relays_decode_once_and_delayed_handoff_is_discarded() {
    let (mut voice, _server, _peer, _auth, _live) = registered().await;
    let mut packet = audio_packet(&voice, 10);
    voice.buffer_voice(&packet).unwrap();
    packet.client_id = ClientId::from_bytes(b"CCCCCCCCCCCCCCCCCCCCCC").unwrap();
    packet.retransmission_count = 1;
    assert_eq!(
        voice.buffer_voice(&packet).unwrap(),
        crate::receive::QueueOutcome::Duplicate
    );
    tokio::time::advance(Duration::from_millis(80)).await;
    let mut pcm = [0; srs_audio::OUTPUT_SAMPLES];
    assert_eq!(
        voice.poll_audio(&mut pcm).unwrap().unwrap().packet_number,
        10
    );
    assert!(voice.poll_audio(&mut pcm).unwrap().is_none());
    tokio::time::advance(Duration::from_millis(40)).await;
    packet.packet_number = 11;
    assert_eq!(
        voice.buffer_voice(&packet).unwrap(),
        crate::receive::QueueOutcome::Stale
    );
}

#[tokio::test(start_paused = true)]
async fn speaker_capacity_rejects_newcomers_until_an_idle_slot_is_available() {
    let (mut voice, _server, _peer, _auth, _live) = registered().await;
    let mut packet = audio_packet(&voice, 1);
    for i in 0..crate::receive::MAX_SPEAKERS {
        let mut id = [b'A'; 22];
        id[0] += i as u8;
        packet.original_client_id = ClientId::from_bytes(&id).unwrap();
        assert!(matches!(
            voice.buffer_voice(&packet).unwrap(),
            crate::receive::QueueOutcome::Queued | crate::receive::QueueOutcome::Resynchronized
        ));
    }
    packet.original_client_id = ClientId::from_bytes(b"ZZZZZZZZZZZZZZZZZZZZZZ").unwrap();
    assert_eq!(
        voice.buffer_voice(&packet).unwrap(),
        crate::receive::QueueOutcome::Capacity
    );
    tokio::time::advance(Duration::from_secs(2)).await;
    packet.received_at = Instant::now();
    assert!(matches!(
        voice.buffer_voice(&packet).unwrap(),
        crate::receive::QueueOutcome::Queued | crate::receive::QueueOutcome::Resynchronized
    ));
}

#[tokio::test(start_paused = true)]
async fn voice_timeout_and_close_never_return_queued_audio() {
    let (mut voice, _server, _peer, _auth, _live) = registered().await;
    voice.buffer_voice(&audio_packet(&voice, 1)).unwrap();
    tokio::time::advance(VOICE_TIMEOUT).await;
    let mut pcm = [99; srs_audio::OUTPUT_SAMPLES];
    assert_eq!(voice.poll_audio(&mut pcm), Err(VoiceError::Timeout));
    assert!(pcm.iter().all(|&x| x == 0));
    voice.close();
    assert_eq!(voice.audio_deadline(), Err(VoiceError::NotReady));
}

#[tokio::test(start_paused = true)]
async fn peer_disconnect_allows_sequence_restart_without_releasing_local_ptt() {
    let (mut voice, _server, _peer, _auth, _live) = registered().await;
    let mut packet = audio_packet(&voice, 1_000);
    voice.press_ptt(1).unwrap();
    voice.buffer_voice(&packet).unwrap();
    voice.forget_speaker(packet.original_client_id);
    assert!(voice.is_transmitting());
    assert!(voice.audio_deadline().unwrap().is_none());
    packet.packet_number = 1;
    assert_eq!(
        voice.buffer_voice(&packet).unwrap(),
        crate::receive::QueueOutcome::Queued
    );
}

#[tokio::test(start_paused = true)]
async fn mono_mix_sums_due_speakers_once_saturates_and_revokes_backlog() {
    let (mut voice, _server, _, auth, _live) = registered().await;
    let mut packet = audio_packet(&voice, 1);
    let mut decoder = srs_audio::Decoder::new().unwrap();
    let mut single = [0; srs_audio::OUTPUT_SAMPLES];
    decoder.decode(&packet.audio, &mut single).unwrap();
    for speaker in b'B'..=b'E' {
        packet.original_client_id = ClientId::from_bytes(&[speaker; 22]).unwrap();
        voice.buffer_voice(&packet).unwrap();
    }
    let mut output = [1; srs_audio::OUTPUT_SAMPLES];
    assert_eq!(voice.poll_mix(&mut output).unwrap().speakers, 0);
    assert!(output.iter().all(|x| *x == 0));
    tokio::time::advance(Duration::from_millis(80)).await;
    let mix = voice.poll_mix(&mut output).unwrap();
    assert_eq!(mix.speakers, 4);
    assert!(mix.clipped_samples > 0);
    for (actual, sample) in output.iter().zip(single) {
        assert_eq!(
            *actual,
            (i32::from(sample) * 4).clamp(i16::MIN as i32, i16::MAX as i32) as i16
        );
    }
    assert_eq!(voice.poll_mix(&mut output).unwrap().speakers, 0);
    update_policy(&auth, |p| p.radios[1].frequency_hz = 252e6);
    tokio::time::advance(Duration::from_millis(40)).await;
    assert_eq!(voice.poll_mix(&mut output).unwrap().speakers, 0);
    assert!(output.iter().all(|x| *x == 0));
}

#[tokio::test(start_paused = true)]
async fn receive_levels_scale_each_radio_and_mute_keeps_decoders_and_rx_current() {
    use crate::receive::{DEFAULT_MONITOR, RadioLevel};
    let (mut voice, _server, _, auth, _live) = registered().await;
    update_policy(&auth, |p| {
        p.radios[2] = p.radios[1];
        p.radios[2].frequency_hz = 252e6;
    });
    let mut levels = DEFAULT_MONITOR;
    assert_eq!(RadioLevel::new(101, false), None);
    levels[1] = RadioLevel::new(50, false).unwrap();
    levels[2] = RadioLevel::new(25, false).unwrap();
    voice.set_monitor(levels);
    voice.press_ptt(1).unwrap();
    let mut decoder = srs_audio::Decoder::new().unwrap();
    let mut expected = [0; srs_audio::OUTPUT_SAMPLES];
    let mut output = [0; srs_audio::OUTPUT_SAMPLES];
    for sequence in 1..=3 {
        for (speaker, hz) in [(b'B', 251e6), (b'C', 252e6)] {
            let mut packet = audio_packet(&voice, sequence);
            packet.original_client_id = ClientId::from_bytes(&[speaker; 22]).unwrap();
            packet.frequencies[0].hz = hz;
            assert_eq!(
                voice.buffer_voice(&packet).unwrap(),
                crate::receive::QueueOutcome::Queued
            );
            assert_eq!(
                voice.buffer_voice(&packet).unwrap(),
                crate::receive::QueueOutcome::Duplicate
            );
            if speaker == b'B' {
                decoder.decode(&packet.audio, &mut expected).unwrap();
            }
        }
        if sequence == 2 {
            levels[1] = RadioLevel::new(50, true).unwrap();
            levels[2] = RadioLevel::new(25, true).unwrap();
            voice.set_monitor(levels); // Applies to frames that are already queued.
        } else if sequence == 3 {
            levels[1] = RadioLevel::FULL;
            levels[2] = RadioLevel::new(0, false).unwrap();
            voice.set_monitor(levels);
        }
        tokio::time::advance(Duration::from_millis(if sequence == 1 { 80 } else { 40 })).await;
        let mix = voice.poll_mix(&mut output).unwrap();
        assert_eq!(
            mix.speakers, 2,
            "one decode per original speaker despite relays"
        );
        assert_eq!(mix.received_radios, (1 << 1) | (1 << 2));
        assert_eq!(
            mix.audible_speakers,
            match sequence {
                1 => 2,
                2 => 0,
                _ => 1,
            }
        );
        let gain = match sequence {
            1 => 75,
            2 => 0,
            _ => 100,
        };
        for (actual, sample) in output.iter().zip(expected) {
            assert_eq!(*actual, (i32::from(sample) * gain / 100) as i16);
        }
        assert!(
            voice.is_transmitting(),
            "local receive controls must not alter TX authorization"
        );
    }
    tokio::time::advance(Duration::from_millis(40)).await;
    assert_eq!(
        voice.poll_mix(&mut output).unwrap().received_radios,
        0,
        "PLC must not invent new RX activity"
    );
}

#[tokio::test(start_paused = true)]
async fn single_speaker_playout_applies_local_gain_and_keeps_muted_metadata() {
    use crate::receive::{DEFAULT_MONITOR, RadioLevel};
    let (mut voice, _server, _, _auth, _live) = registered().await;
    let mut levels = DEFAULT_MONITOR;
    levels[1] = RadioLevel::new(25, false).unwrap();
    voice.set_monitor(levels);
    let packet = audio_packet(&voice, 1);
    let mut expected = [0; srs_audio::OUTPUT_SAMPLES];
    srs_audio::Decoder::new()
        .unwrap()
        .decode(&packet.audio, &mut expected)
        .unwrap();
    voice.buffer_voice(&packet).unwrap();
    tokio::time::advance(Duration::from_millis(80)).await;
    let mut pcm = [0; srs_audio::OUTPUT_SAMPLES];
    assert_eq!(voice.poll_audio(&mut pcm).unwrap().unwrap().radio, 1);
    for (actual, sample) in pcm.iter().zip(expected) {
        assert_eq!(*actual, (i32::from(sample) / 4) as i16);
    }
    voice.buffer_voice(&audio_packet(&voice, 2)).unwrap();
    levels[1] = RadioLevel::new(25, true).unwrap();
    voice.set_monitor(levels);
    tokio::time::advance(Duration::from_millis(40)).await;
    let frame = voice.poll_audio(&mut pcm).unwrap().unwrap();
    assert_eq!(frame.radio, 1);
    assert_eq!(frame.packet_number, 2);
    assert_eq!(frame.kind, srs_audio::jitter::FrameKind::Decoded);
    assert!(pcm.iter().all(|sample| *sample == 0));
}
