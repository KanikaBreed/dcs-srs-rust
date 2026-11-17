use serde_json::{Value, json};
use srs_client::{
    Coalition, Error, Event, Identity, Options, Radio, Session, State, disabled_radios,
};
use srs_protocol::voice::{ClientId, Modulation};
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    time::{sleep, timeout},
};

const ID: &str = "AAAAAAAAAAAAAAAAAAAAAA";
const PEER: &str = "_____________________w";
const PASSWORD: &str = "synthetic-password";
type Peer = BufReader<TcpStream>;

fn identity() -> Identity {
    Identity::new(ClientId::from_bytes(ID.as_bytes()).unwrap(), "Fixture TOC")
}
fn options() -> Options {
    Options {
        operation_timeout: Duration::from_secs(2),
        ..Options::default()
    }
}
async fn listener() -> (TcpListener, SocketAddr) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    (listener, address)
}
async fn read(peer: &mut Peer) -> Value {
    let mut line = String::new();
    timeout(Duration::from_secs(3), peer.read_line(&mut line))
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str(&line).unwrap()
}
async fn send(peer: &mut Peer, message: Value) {
    let mut bytes = serde_json::to_vec(&message).unwrap();
    bytes.push(b'\n');
    peer.get_mut().write_all(&bytes).await.unwrap();
}
fn sync(enabled: bool) -> Value {
    json!({"MsgType":2,"Version":"2.4.1.0","Clients":[],"ServerSettings":{"EXTERNAL_AWACS_MODE": enabled.to_string()}})
}
async fn accept(listener: TcpListener) -> Peer {
    let mut peer = BufReader::new(listener.accept().await.unwrap().0);
    let request = read(&mut peer).await;
    assert_eq!(request["MsgType"], 2);
    assert_eq!(request["Version"], "2.4.1.0");
    assert_eq!(request["Client"]["Coalition"], 0);
    assert_eq!(
        request["Client"]["RadioInfo"]["radios"]
            .as_array()
            .unwrap()
            .len(),
        11
    );
    send(&mut peer, sync(true)).await;
    peer
}
async fn login(peer: &mut Peer, coalition: u8) {
    let request = read(peer).await;
    assert_eq!(request["MsgType"], 7);
    assert_eq!(request["ExternalAWACSModePassword"], PASSWORD);
    assert_eq!(request["Client"]["Coalition"], 0);
    // Stock server response deliberately has no ClientGuid.
    send(peer, json!({"MsgType":7,"Client":{"Coalition":coalition}})).await;
}

#[tokio::test]
async fn server_assigned_coalition_radio_exchange_and_explicit_leave() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let mut peer = accept(listener).await;
        login(&mut peer, 1).await;
        let update = read(&mut peer).await;
        assert_eq!(update["MsgType"], 3);
        assert_eq!(update["Client"]["Coalition"], 1);
        assert_eq!(update["Client"]["RadioInfo"]["unitId"], 100000001);
        let radio = &update["Client"]["RadioInfo"]["radios"][1];
        assert_eq!(radio["freq"], 251_000_000.0);
        assert_eq!(radio["secFreq"], 243_000_000.0);
        assert_eq!(radio["modulation"], 0);
        assert_eq!(radio["Model"], "arc164");
        assert_eq!(radio["enc"], true);
        assert_eq!(radio["encKey"], 7);
        send(&mut peer, update).await;
        let leave = read(&mut peer).await;
        assert_eq!(leave["MsgType"], 8);
        assert_eq!(leave["Client"]["Coalition"], 0);
        let reset = read(&mut peer).await;
        assert_eq!(reset["MsgType"], 3);
        assert_eq!(reset["Client"]["RadioInfo"]["radios"][1]["modulation"], 3);
    });
    let mut client = Session::connect(address, identity(), options())
        .await
        .unwrap();
    assert_eq!(client.state(), State::Connected);
    assert_eq!(
        client.set_radios(disabled_radios()).await,
        Err(Error::NotAuthenticated)
    );
    assert_eq!(client.authenticate(PASSWORD).await.unwrap(), Coalition::Red);
    let mut radios = disabled_radios();
    radios[1] = Radio::tuned(251_000_000.0, Modulation::AM);
    radios[1].guard_hz = 243_000_000.0;
    radios[1].model = " ARC-164 ".into();
    radios[1].encryption_key = Some(7);
    client.set_radios(radios).await.unwrap();
    assert!(matches!(
        client.next_event().await.unwrap(),
        Event::ClientUpdated(_)
    ));
    assert_eq!(
        client.clients()[ID]["RadioInfo"]["radios"][1]["freq"],
        251_000_000.0
    );
    client.leave_eam().await.unwrap();
    assert_eq!(client.state(), State::Connected);
    server.await.unwrap();
    assert_eq!(client.next_event().await, Err(Error::Disconnected));
    assert_eq!(client.state(), State::Disconnected);
}

#[tokio::test]
async fn rejected_credentials_allow_retry_but_never_enable_radio_updates() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let mut peer = accept(listener).await;
        login(&mut peer, 0).await;
        login(&mut peer, 2).await;
    });
    let mut client = Session::connect(address, identity(), options())
        .await
        .unwrap();
    assert_eq!(
        client.authenticate(PASSWORD).await,
        Err(Error::AuthenticationRejected)
    );
    assert_eq!(client.state(), State::Connected);
    assert_eq!(
        client.set_radios(disabled_radios()).await,
        Err(Error::NotAuthenticated)
    );
    assert_eq!(
        client.authenticate(PASSWORD).await.unwrap(),
        Coalition::Blue
    );
    server.await.unwrap();
}

#[tokio::test]
async fn disabled_eam_never_sends_a_password() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let mut peer = BufReader::new(listener.accept().await.unwrap().0);
        read(&mut peer).await;
        send(&mut peer, sync(false)).await;
        let mut line = String::new();
        assert_eq!(peer.read_line(&mut line).await.unwrap(), 0);
    });
    let mut client = Session::connect(address, identity(), options())
        .await
        .unwrap();
    assert_eq!(client.authenticate(PASSWORD).await, Err(Error::EamDisabled));
    client.close();
    server.await.unwrap();
}

#[tokio::test]
async fn fragmented_and_coalesced_messages_preserve_radios_on_metadata_update() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let mut peer = accept(listener).await;
        let messages = [
            json!({"MsgType":3,"Client":{"ClientGuid":PEER,"Name":"Other","RadioInfo":{"radios":[{"freq":30000000}]}}}),
            json!({"MsgType":0,"Client":{"ClientGuid":PEER,"Name":"Changed é 🚁","RadioInfo":null}}),
            json!({"MsgType":5,"Client":{"ClientGuid":PEER}}),
        ];
        let bytes = messages
            .into_iter()
            .map(|m| m.to_string() + "\n")
            .collect::<String>();
        for chunk in bytes.as_bytes().chunks(7) {
            peer.get_mut().write_all(chunk).await.unwrap();
        }
    });
    let mut client = Session::connect(address, identity(), options())
        .await
        .unwrap();
    client.next_event().await.unwrap();
    client.next_event().await.unwrap();
    assert_eq!(client.clients()[PEER]["Name"], "Changed é 🚁");
    assert_eq!(
        client.clients()[PEER]["RadioInfo"]["radios"][0]["freq"],
        30000000
    );
    assert!(matches!(
        client.next_event().await.unwrap(),
        Event::ClientDisconnected(_)
    ));
    assert!(client.clients().is_empty());
    server.await.unwrap();
}

#[tokio::test]
async fn version_policy_rejects_missing_malformed_old_and_explicit_mismatch() {
    for reply in [
        json!({"MsgType":2,"Clients":[]}),
        json!({"MsgType":2,"Version":"1.8.9.9","Clients":[]}),
        json!({"MsgType":2,"Version":"1.9","Clients":[]}),
        json!({"MsgType":2,"Version":"secret-invalid-version","Clients":[]}),
        json!({"MsgType":6,"Version":"2.4.1.0"}),
    ] {
        let (listener, address) = listener().await;
        let server = tokio::spawn(async move {
            let mut peer = BufReader::new(listener.accept().await.unwrap().0);
            read(&mut peer).await;
            send(&mut peer, reply).await;
        });
        assert!(matches!(
            Session::connect(address, identity(), options()).await,
            Err(Error::IncompatibleVersion)
        ));
        server.await.unwrap();
    }
}

#[tokio::test]
async fn invalid_authentication_response_closes_connection() {
    for coalition in [json!(-1), json!(3), json!("synthetic-secret"), Value::Null] {
        let (listener, address) = listener().await;
        let server = tokio::spawn(async move {
            let mut peer = accept(listener).await;
            read(&mut peer).await;
            send(
                &mut peer,
                json!({"MsgType":7,"Client":{"Coalition":coalition}}),
            )
            .await;
        });
        let mut client = Session::connect(address, identity(), options())
            .await
            .unwrap();
        assert_eq!(
            client.authenticate(PASSWORD).await,
            Err(Error::InvalidMessage)
        );
        assert_eq!(client.state(), State::Disconnected);
        server.await.unwrap();
    }
}

#[tokio::test]
async fn eam_revocation_clears_authorization_and_resets_remote_radios() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let mut peer = accept(listener).await;
        login(&mut peer, 2).await;
        send(
            &mut peer,
            json!({"MsgType":4,"ServerSettings":{"EXTERNAL_AWACS_MODE":"False"}}),
        )
        .await;
        assert_eq!(read(&mut peer).await["MsgType"], 8);
        let reset = read(&mut peer).await;
        assert_eq!(reset["Client"]["Coalition"], 0);
        assert_eq!(reset["Client"]["RadioInfo"]["radios"][1]["modulation"], 3);
    });
    let mut client = Session::connect(address, identity(), options())
        .await
        .unwrap();
    client.authenticate(PASSWORD).await.unwrap();
    assert_eq!(client.next_event().await.unwrap(), Event::EamDisabled);
    assert_eq!(client.state(), State::Connected);
    assert_eq!(
        client.set_radios(disabled_radios()).await,
        Err(Error::NotAuthenticated)
    );
    server.await.unwrap();
}

#[tokio::test]
async fn radio_validation_and_server_policy_block_invalid_writes() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let mut peer = accept(listener).await;
        login(&mut peer, 2).await;
        send(&mut peer, json!({"MsgType":4,"ServerSettings":{"EXTERNAL_AWACS_MODE":"True","ALLOW_RADIO_ENCRYPTION":"False"}})).await;
        let mut line = String::new();
        assert_eq!(peer.read_line(&mut line).await.unwrap(), 0);
    });
    let mut client = Session::connect(address, identity(), options())
        .await
        .unwrap();
    client.authenticate(PASSWORD).await.unwrap();
    client.next_event().await.unwrap();
    let mut radios = disabled_radios();
    radios[1].frequency_hz = f64::NAN;
    assert_eq!(client.set_radios(radios).await, Err(Error::InvalidRadio));
    let mut radios = disabled_radios();
    radios[1].encryption_key = Some(7);
    assert_eq!(client.set_radios(radios).await, Err(Error::PolicyDenied));
    let mut radios = disabled_radios();
    radios[0].modulation = Modulation::AM;
    assert_eq!(client.set_radios(radios).await, Err(Error::InvalidRadio));
    client.close();
    server.await.unwrap();
}

#[tokio::test]
async fn cancellation_closes_tcp_and_never_leaves_an_authenticated_session() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let mut peer = accept(listener).await;
        login(&mut peer, 2).await;
        let mut line = String::new();
        assert_eq!(peer.read_line(&mut line).await.unwrap(), 0);
    });
    let mut client = Session::connect(address, identity(), options())
        .await
        .unwrap();
    client.authenticate(PASSWORD).await.unwrap();
    assert!(
        timeout(Duration::from_millis(20), client.next_event())
            .await
            .is_err()
    );
    assert_eq!(client.state(), State::Disconnected);
    assert_eq!(
        client.set_radios(disabled_radios()).await,
        Err(Error::Disconnected)
    );
    server.await.unwrap();
}

#[tokio::test]
async fn missing_login_response_times_out_and_disconnects() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let mut peer = accept(listener).await;
        read(&mut peer).await;
        let mut line = String::new();
        assert_eq!(peer.read_line(&mut line).await.unwrap(), 0);
    });
    let mut client = Session::connect(
        address,
        identity(),
        Options {
            operation_timeout: Duration::from_millis(100),
            ..Options::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(client.authenticate(PASSWORD).await, Err(Error::Timeout));
    assert_eq!(client.state(), State::Disconnected);
    server.await.unwrap();
}

#[tokio::test]
async fn missing_sync_times_out_and_roster_limits_fail_closed() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let mut peer = BufReader::new(listener.accept().await.unwrap().0);
        read(&mut peer).await;
        sleep(Duration::from_millis(200)).await;
    });
    assert!(matches!(
        Session::connect(
            address,
            identity(),
            Options {
                operation_timeout: Duration::from_millis(50),
                ..Options::default()
            }
        )
        .await,
        Err(Error::Timeout)
    ));
    server.await.unwrap();
    let (listener, address) = self::listener().await;
    let server = tokio::spawn(async move {
        let mut peer = accept(listener).await;
        send(&mut peer, json!({"MsgType":3,"Client":{"ClientGuid":PEER,"Name":"x".repeat(srs_client::MAX_CLIENT_BYTES)}})).await;
    });
    let mut client = Session::connect(address, identity(), options())
        .await
        .unwrap();
    assert_eq!(client.next_event().await, Err(Error::ResourceLimit));
    assert_eq!(client.state(), State::Disconnected);
    server.await.unwrap();
}

#[tokio::test]
async fn malformed_frames_and_truncated_eof_are_terminal() {
    for bytes in [&b"{bad json}\n"[..], &b"{\"MsgType\":"[..]] {
        let (listener, address) = listener().await;
        let server = tokio::spawn(async move {
            let mut peer = accept(listener).await;
            peer.get_mut().write_all(bytes).await.unwrap();
        });
        let mut client = Session::connect(address, identity(), options())
            .await
            .unwrap();
        assert!(matches!(client.next_event().await, Err(Error::Protocol(_))));
        assert_eq!(client.state(), State::Disconnected);
        server.await.unwrap();
    }
}

#[tokio::test(start_paused = true)]
async fn periodic_refresh_keeps_idle_session_current_without_echo_pings() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let mut peer = accept(listener).await;
        login(&mut peer, 2).await;
        let mut line = String::new();
        // No short test timeout: virtual time advances to the 60-second refresh.
        peer.read_line(&mut line).await.unwrap();
        let update: Value = serde_json::from_str(&line).unwrap();
        assert_eq!(update["MsgType"], 3);
        assert_eq!(update["Client"]["Coalition"], 2);
        send(&mut peer, json!({"MsgType":1})).await;
    });
    // The refresh deadline is initialized during connect, before login. Paused
    // time may advance while real socket readiness is delivered on another OS.
    // Measure from before connect, rather than assuming login takes zero time.
    let started = tokio::time::Instant::now();
    let mut client = Session::connect(address, identity(), options())
        .await
        .unwrap();
    tokio::time::advance(Duration::from_millis(250)).await;
    client.authenticate(PASSWORD).await.unwrap();
    client.next_event().await.unwrap();
    assert!(started.elapsed() >= Duration::from_secs(60));
    server.await.unwrap();
}

#[tokio::test]
async fn partial_policy_changes_preserve_eam_and_remove_revoked_radio_features() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let mut peer = accept(listener).await;
        login(&mut peer, 2).await;
        let initial = read(&mut peer).await;
        assert_eq!(initial["Client"]["RadioInfo"]["radios"][1]["enc"], true);
        send(
            &mut peer,
            json!({"MsgType":4,"ServerSettings":{"ALLOW_RADIO_ENCRYPTION":"False"}}),
        )
        .await;
        let adjusted = read(&mut peer).await;
        assert_eq!(adjusted["MsgType"], 3);
        assert_eq!(adjusted["Client"]["Coalition"], 2);
        assert_eq!(adjusted["Client"]["RadioInfo"]["radios"][1]["enc"], false);
        assert_eq!(adjusted["Client"]["RadioInfo"]["radios"][1]["encKey"], 0);
    });
    let mut client = Session::connect(address, identity(), options())
        .await
        .unwrap();
    client.authenticate(PASSWORD).await.unwrap();
    let mut radios = disabled_radios();
    radios[1] = Radio::tuned(251_000_000.0, Modulation::AM);
    radios[1].encryption_key = Some(7);
    client.set_radios(radios).await.unwrap();
    assert_eq!(client.next_event().await.unwrap(), Event::RadiosAdjusted);
    assert_eq!(client.state(), State::Authenticated(Coalition::Blue));
    server.await.unwrap();
}

#[tokio::test]
async fn excessive_sync_client_count_and_incremental_settings_growth_are_bounded() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let mut peer = BufReader::new(listener.accept().await.unwrap().0);
        read(&mut peer).await;
        let clients: Vec<_> = (0..=srs_client::MAX_CLIENTS)
            .map(|n| json!({"ClientGuid":format!("{n:022}")}))
            .collect();
        send(
            &mut peer,
            json!({"MsgType":2,"Version":"2.4.1.0","Clients":clients}),
        )
        .await;
    });
    assert!(matches!(
        Session::connect(address, identity(), options()).await,
        Err(Error::ResourceLimit)
    ));
    server.await.unwrap();
    let (listener, address) = self::listener().await;
    let server = tokio::spawn(async move {
        let mut peer = accept(listener).await;
        for n in 0..3 {
            send(
                &mut peer,
                json!({"MsgType":4,"ServerSettings":{format!("unknown-{n}"):"x".repeat(400_000)}}),
            )
            .await;
        }
    });
    let mut client = Session::connect(address, identity(), options())
        .await
        .unwrap();
    client.next_event().await.unwrap();
    client.next_event().await.unwrap();
    assert_eq!(client.next_event().await, Err(Error::ResourceLimit));
    assert_eq!(client.state(), State::Disconnected);
    server.await.unwrap();
}

#[tokio::test]
async fn aggregate_roster_growth_across_small_messages_is_bounded() {
    let (listener, address) = listener().await;
    let server = tokio::spawn(async move {
        let mut peer = accept(listener).await;
        // Each message fits the per-client cap, but together they exceed 8 MiB.
        for n in 0..140 {
            send(&mut peer, json!({"MsgType":3,"Client":{"ClientGuid":format!("{n:022}"),"extra":"x".repeat(60_000)}})).await;
        }
    });
    let mut client = Session::connect(address, identity(), options())
        .await
        .unwrap();
    for _ in 0..139 {
        client.next_event().await.unwrap();
    }
    assert_eq!(client.next_event().await, Err(Error::ResourceLimit));
    assert_eq!(client.state(), State::Disconnected);
    server.await.unwrap();
}

#[tokio::test]
async fn cancelling_control_read_immediately_revokes_an_existing_udp_ptt() {
    let (listener, address) = listener().await;
    let udp = tokio::net::UdpSocket::bind(address).await.unwrap();
    let echo = tokio::spawn(async move {
        let mut bytes = [0; 22];
        let (n, peer) = udp.recv_from(&mut bytes).await.unwrap();
        assert_eq!(n, 22);
        udp.send_to(&bytes, peer).await.unwrap();
    });
    let server = tokio::spawn(async move {
        let mut peer = accept(listener).await;
        login(&mut peer, 2).await;
        assert_eq!(read(&mut peer).await["MsgType"], 3);
        let mut line = String::new();
        assert_eq!(peer.read_line(&mut line).await.unwrap(), 0);
    });
    let mut client = Session::connect(address, identity(), options())
        .await
        .unwrap();
    client.authenticate(PASSWORD).await.unwrap();
    let mut radios = disabled_radios();
    radios[1] = Radio::tuned(251_000_000.0, Modulation::AM);
    client.set_radios(radios).await.unwrap();
    let mut voice = client.open_voice().await.unwrap();
    voice.press_ptt(1).unwrap();
    assert!(voice.is_transmitting());
    assert!(
        timeout(Duration::from_millis(20), client.next_event())
            .await
            .is_err()
    );
    assert!(!voice.is_ready());
    assert!(!voice.is_transmitting());
    assert_eq!(
        voice.send_frame(&[1]).await,
        Err(srs_client::voice::VoiceError::ControlDisconnected)
    );
    echo.await.unwrap();
    server.await.unwrap();
}

#[tokio::test]
async fn receive_settings_invalidate_queued_audio_without_changing_local_radios() {
    use srs_client::{receive::QueueOutcome, voice::VoiceEvent};
    use srs_protocol::voice::{Frequency, VoicePacket};
    let (listener, address) = listener().await;
    let udp = tokio::net::UdpSocket::bind(address).await.unwrap();
    let echo = tokio::spawn(async move {
        let mut bytes = [0; 22];
        let (_, peer) = udp.recv_from(&mut bytes).await.unwrap();
        udp.send_to(&bytes, peer).await.unwrap();
        (udp, peer)
    });
    let server = tokio::spawn(async move {
        let mut peer = accept(listener).await;
        login(&mut peer, 2).await;
        read(&mut peer).await;
        peer
    });
    let mut client = Session::connect(address, identity(), options())
        .await
        .unwrap();
    client.authenticate(PASSWORD).await.unwrap();
    let mut radios = disabled_radios();
    radios[1] = Radio::tuned(251_000_000.0, Modulation::AM);
    radios[1].encryption_key = Some(7);
    client.set_radios(radios).await.unwrap();
    let mut server = server.await.unwrap();
    let mut voice = client.open_voice().await.unwrap();
    let (udp, address) = echo.await.unwrap();
    let mut encoder = srs_audio::Encoder::new().unwrap();
    let mut bytes = [0; 4000];
    let len = encoder.encode(&[0; 640], &mut bytes).unwrap();
    let packet = VoicePacket {
        audio: &bytes[..len],
        frequencies: vec![Frequency {
            hz: 251_000_000.0,
            modulation: Modulation::AM,
            encryption: 0,
        }],
        unit_id: 100000001,
        packet_number: 1,
        retransmission_count: 0,
        client_id: ClientId::from_bytes(PEER.as_bytes()).unwrap(),
        original_client_id: ClientId::from_bytes(PEER.as_bytes()).unwrap(),
    }
    .encode()
    .unwrap();
    udp.send_to(&packet, address).await.unwrap();
    let VoiceEvent::Voice(frame) = voice.next_event().await.unwrap() else {
        panic!("expected voice");
    };
    assert!(matches!(
        voice.buffer_voice(&frame).unwrap(),
        QueueOutcome::Queued | QueueOutcome::Resynchronized
    ));
    assert!(voice.audio_deadline().unwrap().is_some());
    send(
        &mut server,
        json!({"MsgType":4,"ServerSettings":{"STRICT_RADIO_ENCRYPTION":"true"}}),
    )
    .await;
    assert_eq!(client.next_event().await.unwrap(), Event::SettingsUpdated);
    assert!(voice.audio_deadline().unwrap().is_none());
    assert!(matches!(
        voice.next_event().await.unwrap(),
        VoiceEvent::AuthorizationChanged
    ));
    udp.send_to(&packet, address).await.unwrap();
    let VoiceEvent::Voice(frame) = voice.next_event().await.unwrap() else {
        panic!("expected voice");
    };
    assert_eq!(
        voice.buffer_voice(&frame).unwrap(),
        QueueOutcome::Ineligible
    );
    // Partial settings updates preserve strictness; MHz parsing grants global
    // decryption override only, and still obeys strict local key matching.
    send(
        &mut server,
        json!({"MsgType":4,"ServerSettings":{"GLOBAL_LOBBY_FREQUENCIES":" 251 ,invalid, NaN "}}),
    )
    .await;
    assert_eq!(client.next_event().await.unwrap(), Event::SettingsUpdated);
    assert_eq!(client.settings()["STRICT_RADIO_ENCRYPTION"], "true");
    send(
        &mut server,
        json!({"MsgType":4,"ServerSettings":{"STRICT_RADIO_ENCRYPTION":"invalid"}}),
    )
    .await;
    assert_eq!(client.next_event().await, Err(Error::InvalidMessage));
    let mut pcm = [99; srs_audio::OUTPUT_SAMPLES];
    assert!(voice.poll_audio(&mut pcm).is_err());
    assert!(pcm.iter().all(|&x| x == 0));
}

async fn editable_pair() -> (Session, Peer) {
    let (listener, address) = listener().await;
    let serve = async {
        let mut peer = accept(listener).await;
        login(&mut peer, 2).await;
        read(&mut peer).await;
        peer
    };
    let connect = async {
        let mut session = Session::connect(address, identity(), options())
            .await
            .unwrap();
        session.authenticate(PASSWORD).await.unwrap();
        let mut radios = disabled_radios();
        radios[1] = Radio::tuned(251e6, Modulation::AM);
        radios[1].guard_hz = 243e6;
        session.set_radios(radios).await.unwrap();
        session
    };
    let (peer, session) = tokio::join!(serve, connect);
    (session, peer)
}
#[tokio::test]
async fn live_edit_preserves_partial_tcp_message_and_rejects_policy_without_disconnect() {
    use srs_client::commands::{RadioChange, radio_controls};
    let (mut session, mut peer) = editable_pair().await;
    let (control, mut updates) = radio_controls();
    peer.get_mut()
        .write_all(b"{\"MsgType\":4,\"ServerSettings\":{\"ALLOW_RADIO_ENCRYPTION\":")
        .await
        .unwrap();
    let edit = async {
        sleep(Duration::from_millis(20)).await;
        control
            .apply(RadioChange::Tune {
                slot: 1,
                hz: 252e6,
                modulation: Modulation::FM,
            })
            .await
            .unwrap()
    };
    let (event, snapshot) = tokio::join!(session.next_event_with_updates(&mut updates), edit);
    assert_eq!(event.unwrap(), Event::LocalRadioUpdate);
    assert_eq!(snapshot.radios[1].frequency_hz, 252e6);
    assert_eq!(snapshot.radios[1].guard_hz, 243e6);
    assert_eq!(
        read(&mut peer).await["Client"]["RadioInfo"]["radios"][1]["freq"],
        252e6
    );
    peer.get_mut().write_all(b"\"false\"}}\n").await.unwrap();
    assert_eq!(
        session.next_event_with_updates(&mut updates).await.unwrap(),
        Event::SettingsUpdated
    );
    let (event, result) = tokio::join!(
        session.next_event_with_updates(&mut updates),
        control.apply(RadioChange::Key {
            slot: 1,
            key: Some(3)
        })
    );
    assert_eq!(event.unwrap(), Event::LocalRadioUpdate);
    assert!(matches!(result, Err(Error::PolicyDenied)));
    assert_eq!(session.radios()[1].encryption_key, None);
    assert_eq!(session.state(), State::Authenticated(Coalition::Blue));
}
#[tokio::test]
async fn abandoned_and_expired_radio_requests_do_not_replay_and_connection_survives() {
    use srs_client::commands::{RadioChange, radio_controls};
    let (mut session, mut peer) = editable_pair().await;
    let (control, mut updates) = radio_controls();
    assert!(
        timeout(
            Duration::from_millis(2),
            control.apply(RadioChange::Disable { slot: 1 })
        )
        .await
        .is_err()
    );
    let (event, snapshot) = tokio::join!(
        session.next_event_with_updates(&mut updates),
        control.apply(RadioChange::Guard { slot: 1, hz: 242e6 })
    );
    assert_eq!(event.unwrap(), Event::LocalRadioUpdate);
    assert_eq!(snapshot.unwrap().radios[1].frequency_hz, 251e6);
    assert_eq!(
        read(&mut peer).await["Client"]["RadioInfo"]["radios"][1]["secFreq"],
        242e6
    );
    let drive = async {
        sleep(Duration::from_millis(300)).await;
        session.next_event_with_updates(&mut updates).await
    };
    let (event, result) = tokio::join!(drive, control.apply(RadioChange::Disable { slot: 1 }));
    assert_eq!(event.unwrap(), Event::LocalRadioUpdate);
    assert!(matches!(result, Err(Error::Timeout)));
    assert_eq!(session.radios()[1].modulation, Modulation::AM);
    let (event, snapshot) = tokio::join!(
        session.next_event_with_updates(&mut updates),
        control.apply(RadioChange::Tune {
            slot: 2,
            hz: 124e6,
            modulation: Modulation::AM
        })
    );
    assert_eq!(event.unwrap(), Event::LocalRadioUpdate);
    assert_eq!(snapshot.unwrap().radios[2].frequency_hz, 124e6);
    assert_eq!(
        read(&mut peer).await["Client"]["RadioInfo"]["radios"][2]["freq"],
        124e6
    );
}

#[tokio::test]
async fn rejected_complete_presets_leave_every_radio_and_tcp_stream_unchanged() {
    use srs_client::commands::{RadioEdit, radio_controls};
    let (mut session, mut peer) = editable_pair().await;
    let (control, mut updates) = radio_controls();
    send(
        &mut peer,
        json!({"MsgType":4,"ServerSettings":{"ALLOW_RADIO_ENCRYPTION":"false"}}),
    )
    .await;
    session.next_event().await.unwrap();
    let original = session.radios().clone();
    for invalid_frequency in [true, false] {
        let mut preset = original.clone();
        preset[1] = Radio::tuned(252e6, Modulation::FM);
        preset[10] = Radio::tuned(
            if invalid_frequency { f64::NAN } else { 254e6 },
            Modulation::AM,
        );
        if !invalid_frequency {
            preset[10].encryption_key = Some(7);
        }
        let (event, result) = tokio::join!(
            session.next_event_with_updates(&mut updates),
            control.update(RadioEdit::Replace(Box::new(preset)))
        );
        assert_eq!(event.unwrap(), Event::LocalRadioUpdate);
        assert!(matches!(
            result,
            Err(Error::InvalidRadio | Error::PolicyDenied)
        ));
        assert!(*session.radios() == original);
        assert!(
            timeout(Duration::from_millis(30), peer.fill_buf())
                .await
                .is_err()
        );
    }
    assert_eq!(session.state(), State::Authenticated(Coalition::Blue));
}
