#![forbid(unsafe_code)]
//! One joinable thread per explicitly started session. No UI or audio callback
//! executes network/codec work. Passwords are never formatted or persisted.
use crate::endpoint::Endpoint;
use crate::input::{Intent, Lease, Update};
use srs_client::receive::{DEFAULT_MONITOR, Monitor};
use srs_client::{
    Identity, Options, RADIO_COUNT, Radio, Session, commands::RadioEdit, disabled_radios,
};
use srs_device::native::{Input, Output};
use srs_protocol::voice::{ClientId, Modulation};
use srs_toc::{Phase, Status};
use std::{
    thread::JoinHandle,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, watch};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(i32)]
pub enum Problem {
    #[default]
    None = 0,
    Address = 1,
    Callsign,
    Password,
    Device,
    Connect,
    Authenticate,
    Radio,
    Session,
    Runtime,
    Backlog,
    Identity,
    Frequency,
    Profile,
    Resolve,
    Network,
}
#[derive(Clone)]
pub struct Config {
    pub address: Endpoint,
    pub callsign: String,
    pub password: String,
    pub input: String,
    pub output: String,
    pub radios: [Radio; RADIO_COUNT],
    pub monitor: Monitor,
    pub retry: bool,
}
impl Config {
    pub fn parse(
        address: &str,
        callsign: &str,
        password: String,
        input: String,
        output: String,
    ) -> Result<Self, Problem> {
        let address = Endpoint::parse(address)?;
        let callsign = callsign.trim();
        if callsign.is_empty() || callsign.len() > 128 || callsign.chars().any(char::is_control) {
            return Err(Problem::Callsign);
        }
        if password.len() > 1024 {
            return Err(Problem::Password);
        }
        Ok(Self {
            address,
            callsign: callsign.to_owned(),
            password,
            input,
            output,
            radios: disabled_radios(),
            monitor: DEFAULT_MONITOR,
            retry: false,
        })
    }
}
pub fn frequency(value: &str) -> Result<f64, Problem> {
    let hz = value
        .trim()
        .parse::<f64>()
        .map_err(|_| Problem::Frequency)?
        * 1e6;
    if !hz.is_finite() || !(10_001.0..=10e9).contains(&hz) {
        return Err(Problem::Frequency);
    }
    Ok(hz)
}
fn identity(callsign: String) -> Result<Identity, Problem> {
    // Uniform 132 random bits in the existing 22-byte wire identity alphabet.
    let mut bytes = [0_u8; 22];
    getrandom::fill(&mut bytes).map_err(|_| Problem::Identity)?;
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    for byte in &mut bytes {
        *byte = ALPHABET[usize::from(*byte & 63)];
    }
    let id = ClientId::from_bytes(&bytes).map_err(|_| Problem::Identity)?;
    Ok(Identity::new(id, callsign))
}
#[derive(Clone)]
pub struct Snapshot {
    pub status: Status,
    pub radios: [Radio; RADIO_COUNT],
    pub problem: Problem,
    pub editing: bool,
    pub finished: bool,
    pub completed_edit: u64,
    pub details: srs_toc::observation::SessionDetails,
    pub retry_attempt: u8,
    pub monitor: Monitor,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            status: Status::default(),
            radios: disabled_radios(),
            problem: Problem::None,
            editing: false,
            finished: false,
            completed_edit: 0,
            details: srs_toc::observation::SessionDetails::default(),
            retry_attempt: 0,
            monitor: DEFAULT_MONITOR,
        }
    }
}
pub struct Edit {
    pub id: u64,
    pub at: Instant,
    pub change: RadioEdit,
    pub monitor: Option<Monitor>,
}
pub struct Handle {
    stop: watch::Sender<bool>,
    pub input: watch::Sender<Intent>,
    pub edits: mpsc::Sender<Edit>,
    pub monitor: watch::Sender<Monitor>,
    pub view: watch::Receiver<Snapshot>,
    thread: Option<JoinHandle<()>>,
}
impl Handle {
    pub fn start(config: Config, initial: Intent) -> Result<Self, Problem> {
        let (stop, stopped) = watch::channel(false);
        let (input, inputs) = watch::channel(initial);
        let (monitor, levels) = watch::channel(config.monitor);
        let (edits, requests) = mpsc::channel(1);
        let (view, receiver) = watch::channel(Snapshot {
            radios: config.radios.clone(),
            monitor: config.monitor,
            ..Snapshot::default()
        });
        let thread = std::thread::Builder::new()
            .name("srs-session".into())
            .spawn(move || {
                let result = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime.block_on(async {
                        tokio::select! {
                            biased;
                            _ = cancelled(stopped) => Ok(()),
                            result = reconnecting(config, inputs, requests, levels, &view) => result,
                        }
                    }),
                    Err(_) => Err(Problem::Runtime),
                };
                view.send_modify(|snapshot| {
                    snapshot.status.phase = Phase::Stopped;
                    snapshot.finished = true;
                    snapshot.editing = false;
                    snapshot.problem = result.err().unwrap_or(Problem::None);
                });
            })
            .map_err(|_| Problem::Runtime)?;
        Ok(Self {
            stop,
            input,
            edits,
            monitor,
            view: receiver,
            thread: Some(thread),
        })
    }
    pub fn stop(&self) {
        self.stop.send_replace(true);
    }
    pub fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.stop();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
async fn cancelled(mut stop: watch::Receiver<bool>) {
    while !*stop.borrow_and_update() {
        if stop.changed().await.is_err() {
            return;
        }
    }
}
fn network(error: srs_client::Error) -> bool {
    matches!(
        error,
        srs_client::Error::Io(_) | srs_client::Error::Timeout | srs_client::Error::Disconnected
    )
}
fn session_problem(error: srs_toc::Error) -> Problem {
    use srs_client::voice::VoiceError;
    match error {
        srs_toc::Error::Control(e) if network(e) => Problem::Network,
        srs_toc::Error::Voice(
            VoiceError::Io(_) | VoiceError::Timeout | VoiceError::ControlDisconnected,
        ) => Problem::Network,
        srs_toc::Error::Interrupted => Problem::Device,
        srs_toc::Error::Device(_) => Problem::Device,
        srs_toc::Error::Control(
            srs_client::Error::EamDisabled
            | srs_client::Error::AuthenticationRejected
            | srs_client::Error::NotAuthenticated,
        ) => Problem::Authenticate,
        srs_toc::Error::Control(srs_client::Error::PolicyDenied) => Problem::Radio,
        _ => Problem::Session,
    }
}
const RETRY_DELAYS: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];
async fn reconnecting(
    mut config: Config,
    inputs: watch::Receiver<Intent>,
    mut edits: mpsc::Receiver<Edit>,
    mut monitor: watch::Receiver<Monitor>,
    view: &watch::Sender<Snapshot>,
) -> Result<(), Problem> {
    if !config.retry {
        // Do not retain a second password copy for a non-retrying session.
        return connected(config, inputs, &mut edits, monitor, view).await;
    }
    let mut delays = RETRY_DELAYS.into_iter().enumerate();
    loop {
        // The snapshot includes acknowledged presets. Consume the GUI watch's
        // older target so it cannot overwrite that preset on a fresh attempt.
        monitor.borrow_and_update();
        // Each attempt owns a new TCP/UDP session, identity and native streams.
        // connected drops all of these before the next backoff begins.
        let result = connected(
            config.clone(),
            inputs.clone(),
            &mut edits,
            monitor.clone(),
            view,
        )
        .await;
        if result != Err(Problem::Network) {
            return result;
        }
        let Some((attempt, delay)) = delays.next() else {
            return result;
        };
        let snapshot = view.borrow().clone();
        config.radios = snapshot.radios;
        config.monitor = snapshot.monitor;
        while edits.try_recv().is_ok() {} // queued edits belong to the failed session
        view.send_modify(|s| {
            s.status.phase = Phase::Connecting;
            s.retry_attempt = (attempt + 1) as u8;
            s.editing = false;
            s.problem = Problem::None;
        });
        tokio::time::sleep(delay).await;
    }
}
async fn connected(
    mut config: Config,
    mut inputs: watch::Receiver<Intent>,
    edits: &mut mpsc::Receiver<Edit>,
    mut monitor: watch::Receiver<Monitor>,
    view: &watch::Sender<Snapshot>,
) -> Result<(), Problem> {
    view.send_modify(|s| {
        s.problem = Problem::None;
        s.status.phase = Phase::Connecting;
    });
    let addresses = config.address.resolve().await?;
    let mut connected = None;
    for address in addresses {
        let result = Session::connect(
            address,
            identity(config.callsign.clone())?,
            Options::default(),
        )
        .await;
        match result {
            Ok(session) => {
                connected = Some(session);
                break;
            }
            Err(error) if network(error) => (),
            Err(_) => return Err(Problem::Connect),
        }
    }
    let mut session = connected.ok_or(Problem::Network)?;
    let auth = session.authenticate(&config.password).await;
    config.password.clear();
    auth.map_err(|e| {
        if network(e) {
            Problem::Network
        } else {
            Problem::Authenticate
        }
    })?;
    // Validate the prepared radios against current server policy before audio opens.
    session.set_radios(config.radios).await.map_err(|e| {
        if network(e) {
            Problem::Network
        } else {
            Problem::Radio
        }
    })?;
    let mut capture = Input::open(&config.input).map_err(|_| Problem::Device)?;
    let mut playback = Output::open(&config.output).map_err(|_| Problem::Device)?;
    capture.start().map_err(|_| Problem::Device)?;
    playback.start().map_err(|_| Problem::Device)?;
    let (mut controller, events) = srs_toc::controls();
    controller.set_monitor(config.monitor);
    let mut current_monitor = config.monitor;
    let (status_tx, status) = srs_toc::status_channel();
    let (radios_tx, radios) = watch::channel(session.radios().clone());
    let (details_tx, details) =
        watch::channel(srs_toc::observation::SessionDetails::read(&session));
    let run = srs_toc::run_with_views(
        session,
        capture.reader(),
        playback.writer(),
        events,
        status_tx,
        Some(radios_tx),
        Some(details_tx),
    );
    let drive = async {
        let mut lease = Lease::default();
        lease.update(*inputs.borrow_and_update(), false, Instant::now());
        let mut released = true;
        let mut ticker = tokio::time::interval(Duration::from_millis(25));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            if monitor.has_changed().unwrap_or(false) {
                current_monitor = *monitor.borrow_and_update();
                controller.set_monitor(current_monitor);
            }
            let current = *status.borrow();
            let ready = matches!(current.phase, Phase::Receiving | Phase::Transmitting);
            let intent = *inputs.borrow_and_update();
            let permitted = ready
                && intent.radio.is_none_or(|slot| {
                    radios
                        .borrow()
                        .get(slot)
                        .is_some_and(|r| matches!(r.modulation, Modulation::AM | Modulation::FM))
                });
            let result = match lease.update(intent, permitted, Instant::now()) {
                Update::Press(slot) => {
                    released = false;
                    controller.press(slot)
                }
                Update::Renew => controller.renew(),
                Update::Release if !released => {
                    released = true;
                    controller.release()
                }
                Update::Release => Ok(()),
            };
            if result.is_err() {
                lease.revoke();
            }
            view.send_modify(|snapshot| {
                snapshot.status = current;
                snapshot.monitor = current_monitor;
                snapshot.details = details.borrow().clone();
                snapshot.radios = radios.borrow().clone();
            });
            tokio::select! {
                biased;
                changed = inputs.changed() => { if changed.is_err() { return Ok(()); } },
                _ = monitor.changed() => { current_monitor = *monitor.borrow_and_update(); controller.set_monitor(current_monitor); },
                edit = edits.recv() => {
                    let Some(edit) = edit else { return Ok(()); };
                    let _ = controller.release();
                    released = true;
                    lease.revoke();
                    if !ready || edit.at.elapsed() >= crate::input::FRESHNESS {
                        view.send_modify(|s| { s.problem = Problem::Backlog; s.completed_edit = edit.id; });
                        continue;
                    }
                    view.send_modify(|s| { s.editing = true; s.problem = Problem::None; });
                    let result = controller.edit_radios(edit.change).await;
                    if result.is_ok() && let Some(levels) = edit.monitor {
                        // Consume the old GUI target; the acknowledged preset is
                        // authoritative until a subsequent intentional level edit.
                        monitor.borrow_and_update();
                        current_monitor = levels;
                        controller.set_monitor(levels);
                    }
                    // Any action held while an edit was in flight is consumed.
                    lease.update(*inputs.borrow_and_update(), false, Instant::now());
                    view.send_modify(|s| {
                        s.editing = false;
                        s.completed_edit = edit.id;
                        s.radios = radios.borrow().clone();
                        s.monitor = current_monitor;
                        if result.is_err() { s.problem = Problem::Radio; }
                    });
                },
                _ = ticker.tick() => (),
            }
        }
    };
    tokio::select! {
        biased;
        result = run => result.map(|_| ()).map_err(session_problem),
        result = drive => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn test_config(address: std::net::SocketAddr) -> Config {
        let mut config = Config::parse(
            &address.to_string(),
            "Fixture",
            "test".into(),
            "default".into(),
            "default".into(),
        )
        .unwrap();
        config.retry = true;
        config
    }
    fn test_intent() -> Intent {
        Intent {
            generation: 0,
            radio: None,
            at: Instant::now(),
        }
    }
    #[tokio::test]
    async fn network_recovery_has_three_retries_and_fresh_identities() {
        use tokio::{
            io::{AsyncBufReadExt, BufReader},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = test_config(listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let mut ids = std::collections::HashSet::new();
            for _ in 0..4 {
                let mut peer = BufReader::new(listener.accept().await.unwrap().0);
                let mut line = String::new();
                peer.read_line(&mut line).await.unwrap();
                let message: serde_json::Value = serde_json::from_str(&line).unwrap();
                assert!(ids.insert(message["Client"]["ClientGuid"].as_str().unwrap().to_owned()));
                // Close during SYNC; no authentication or native device opening.
            }
            ids.len()
        });
        let (_input, inputs) = watch::channel(test_intent());
        let (_edit, edits) = mpsc::channel(1);
        let (_monitor, monitor) = watch::channel(DEFAULT_MONITOR);
        let (view, _) = watch::channel(Snapshot::default());
        let began = Instant::now();
        assert_eq!(
            reconnecting(config, inputs, edits, monitor, &view).await,
            Err(Problem::Network)
        );
        assert_eq!(server.await.unwrap(), 4);
        assert_eq!(view.borrow().retry_attempt, 3);
        assert!(began.elapsed() >= Duration::from_secs(7));
    }
    #[tokio::test]
    async fn authentication_rejection_does_not_retry_or_open_devices() {
        use tokio::{
            io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let config = test_config(listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let mut peer = BufReader::new(listener.accept().await.unwrap().0);
            let mut line = String::new();
            peer.read_line(&mut line).await.unwrap();
            peer.get_mut().write_all(b"{\"MsgType\":2,\"Version\":\"2.4.1.0\",\"Clients\":[],\"ServerSettings\":{\"EXTERNAL_AWACS_MODE\":\"true\"}}\n").await.unwrap();
            line.clear();
            peer.read_line(&mut line).await.unwrap();
            peer.get_mut()
                .write_all(b"{\"MsgType\":7,\"Client\":{\"Coalition\":0}}\n")
                .await
                .unwrap();
        });
        let (_input, inputs) = watch::channel(test_intent());
        let (_edit, edits) = mpsc::channel(1);
        let (_monitor, monitor) = watch::channel(DEFAULT_MONITOR);
        let (view, _) = watch::channel(Snapshot::default());
        assert_eq!(
            reconnecting(config, inputs, edits, monitor, &view).await,
            Err(Problem::Authenticate)
        );
        assert_eq!(view.borrow().retry_attempt, 0);
        server.await.unwrap();
    }
    #[test]
    fn device_policy_and_protocol_errors_are_terminal() {
        assert_eq!(
            session_problem(srs_toc::Error::Interrupted),
            Problem::Device
        );
        assert_eq!(
            session_problem(srs_toc::Error::Device(srs_device::Error::Stopped)),
            Problem::Device
        );
        assert_eq!(
            session_problem(srs_toc::Error::Control(srs_client::Error::EamDisabled)),
            Problem::Authenticate
        );
        assert_eq!(
            session_problem(srs_toc::Error::Control(srs_client::Error::PolicyDenied)),
            Problem::Radio
        );
        assert_eq!(
            session_problem(srs_toc::Error::Control(srs_client::Error::InvalidMessage)),
            Problem::Session
        );
        assert_eq!(
            session_problem(srs_toc::Error::Control(srs_client::Error::Disconnected)),
            Problem::Network
        );
    }
    #[test]
    fn disconnect_cancels_pending_recovery_without_waiting_for_backoff() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let config = test_config(listener.local_addr().unwrap());
        let mut handle = Handle::start(config, test_intent()).unwrap();
        let (peer, _) = listener.accept().unwrap();
        drop(peer);
        let deadline = Instant::now() + Duration::from_secs(3);
        while handle.view.borrow_and_update().retry_attempt == 0 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let began = Instant::now();
        handle.stop();
        drop(handle);
        assert!(began.elapsed() < Duration::from_millis(500));
    }
    #[test]
    fn configuration_rejects_invalid_values_without_opening_devices_or_network() {
        assert!(
            Config::parse(
                "127.0.0.1:5002",
                "TOC",
                "".into(),
                "default".into(),
                "default".into()
            )
            .is_ok()
        );
        for address in ["127.0.0.1:0", "0.0.0.0:5002", "224.0.0.1:5002"] {
            assert!(
                Config::parse(
                    address,
                    "TOC",
                    "".into(),
                    "default".into(),
                    "default".into()
                )
                .is_err()
            );
        }
        for value in ["NaN", "inf", "-1", "0", "10001", "251,5"] {
            assert!(frequency(value).is_err());
        }
        assert_eq!(frequency("251.125").ok(), Some(251_125_000.0));
    }
    #[test]
    fn cancellation_joins_connection_worker() {
        // Loopback address: cancellation is signalled immediately;
        // this test needs no listener and never reaches audio setup.
        let config = Config::parse(
            "127.0.0.1:1",
            "TEST",
            "".into(),
            "default".into(),
            "default".into(),
        )
        .ok()
        .unwrap();
        let handle = Handle::start(
            config,
            Intent {
                generation: 0,
                radio: None,
                at: Instant::now(),
            },
        )
        .ok()
        .unwrap();
        handle.stop();
        drop(handle);
    }
}
