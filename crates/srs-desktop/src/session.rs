#![forbid(unsafe_code)]
//! One joinable thread per explicitly started session. No UI or audio callback
//! executes network/codec work. Passwords are never formatted or persisted.
use crate::input::{Intent, Lease, Update};
use srs_client::{
    Identity, Options, RADIO_COUNT, Radio, Session, commands::RadioEdit, disabled_radios,
};
use srs_device::native::{Input, Output};
use srs_protocol::voice::{ClientId, Modulation};
use srs_toc::{Phase, Status};
use std::{
    net::SocketAddr,
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
}
pub struct Config {
    pub address: SocketAddr,
    pub callsign: String,
    pub password: String,
    pub input: String,
    pub output: String,
    pub radios: [Radio; RADIO_COUNT],
}
impl Config {
    pub fn parse(
        address: &str,
        callsign: &str,
        password: String,
        input: String,
        output: String,
    ) -> Result<Self, Problem> {
        let address: SocketAddr = address.trim().parse().map_err(|_| Problem::Address)?;
        if address.port() == 0 || address.ip().is_unspecified() || address.ip().is_multicast() {
            return Err(Problem::Address);
        }
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
        }
    }
}
pub struct Edit {
    pub id: u64,
    pub at: Instant,
    pub change: RadioEdit,
}
pub struct Handle {
    stop: watch::Sender<bool>,
    pub input: watch::Sender<Intent>,
    pub edits: mpsc::Sender<Edit>,
    pub view: watch::Receiver<Snapshot>,
    thread: Option<JoinHandle<()>>,
}
impl Handle {
    pub fn start(config: Config, initial: Intent) -> Result<Self, Problem> {
        let (stop, stopped) = watch::channel(false);
        let (input, inputs) = watch::channel(initial);
        let (edits, requests) = mpsc::channel(1);
        let (view, receiver) = watch::channel(Snapshot {
            radios: config.radios.clone(),
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
                            result = connected(config, inputs, requests, &view) => result,
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
async fn connected(
    mut config: Config,
    mut inputs: watch::Receiver<Intent>,
    mut edits: mpsc::Receiver<Edit>,
    view: &watch::Sender<Snapshot>,
) -> Result<(), Problem> {
    let mut session = Session::connect(
        config.address,
        identity(config.callsign)?,
        Options::default(),
    )
    .await
    .map_err(|_| Problem::Connect)?;
    let auth = session.authenticate(&config.password).await;
    config.password.clear();
    auth.map_err(|_| Problem::Authenticate)?;
    // Validate the prepared radios against current server policy before audio opens.
    session
        .set_radios(config.radios)
        .await
        .map_err(|_| Problem::Radio)?;
    let mut capture = Input::open(&config.input).map_err(|_| Problem::Device)?;
    let mut playback = Output::open(&config.output).map_err(|_| Problem::Device)?;
    capture.start().map_err(|_| Problem::Device)?;
    playback.start().map_err(|_| Problem::Device)?;
    let (mut controller, events) = srs_toc::controls();
    let (status_tx, status) = srs_toc::status_channel();
    let (radios_tx, radios) = watch::channel(session.radios().clone());
    let run = srs_toc::run_observed(
        session,
        capture.reader(),
        playback.writer(),
        events,
        status_tx,
        Some(radios_tx),
    );
    let drive = async {
        let mut lease = Lease::default();
        let mut released = true;
        let mut ticker = tokio::time::interval(Duration::from_millis(25));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
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
                snapshot.radios = radios.borrow().clone();
            });
            tokio::select! {
                biased;
                changed = inputs.changed() => { if changed.is_err() { return Ok(()); } },
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
                    // Any action held while an edit was in flight is consumed.
                    lease.update(*inputs.borrow_and_update(), false, Instant::now());
                    view.send_modify(|s| {
                        s.editing = false;
                        s.completed_edit = edit.id;
                        s.radios = radios.borrow().clone();
                        if result.is_err() { s.problem = Problem::Radio; }
                    });
                },
                _ = ticker.tick() => (),
            }
        }
    };
    tokio::select! {
        biased;
        result = run => result.map(|_| ()).map_err(|_| Problem::Session),
        result = drive => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        for address in [
            "hostname:5002",
            "127.0.0.1:0",
            "0.0.0.0:5002",
            "224.0.0.1:5002",
        ] {
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
