use crate::{FRESHNESS, Level, Mode, Snapshot, State};
use std::{
    thread,
    time::{Instant, SystemTime},
};
use tokio::sync::watch;

#[derive(Clone)]
struct Request {
    revision: u64,
    mode: Mode,
}
pub struct Worker {
    request: Option<watch::Sender<Request>>,
    pub view: watch::Receiver<Snapshot>,
    thread: Option<thread::JoinHandle<()>>,
    revision: u64,
}
impl Worker {
    /// Creates an idle owned worker. No device or shortcut is opened until asked.
    pub fn spawn() -> std::io::Result<Self> {
        let (request, mut requests) = watch::channel(Request {
            revision: 0,
            mode: Mode::Off,
        });
        let (view, snapshots) = watch::channel(Snapshot::default());
        let thread = thread::Builder::new()
            .name("srs-input".into())
            .spawn(move || {
                let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                else {
                    return;
                };
                runtime.block_on(async move {
                let mut joysticks = None;
                loop {
                    let request = requests.borrow_and_update().clone();
                    let mut publisher = Publisher::new(view.clone(), request.revision);
                    let result = tokio::select! {
                        biased;
                        changed = requests.changed() => { if changed.is_err() { break; } continue; }
                        result = run(request.mode, &mut publisher, &mut joysticks) => result,
                    };
                    if let Err(state) = result { publisher.state(state); }
                    if requests.changed().await.is_err() { break; }
                }
                view.send_modify(|s| { s.state = State::Off; s.level = Level::Unknown; });
            });
            })?;
        Ok(Self {
            request: Some(request),
            view: snapshots,
            thread: Some(thread),
            revision: 0,
        })
    }
    pub fn configure(&mut self, mode: Mode) -> Result<u64, State> {
        if !mode.valid() {
            return Err(State::Failed);
        }
        self.revision = self.revision.checked_add(1).ok_or(State::Failed)?;
        self.request
            .as_ref()
            .ok_or(State::Disconnected)?
            .send(Request {
                revision: self.revision,
                mode,
            })
            .map_err(|_| State::Disconnected)?;
        Ok(self.revision)
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.request.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
pub(crate) struct Publisher {
    sender: watch::Sender<Snapshot>,
    previous: Instant,
    wall: SystemTime,
}
impl Publisher {
    pub(crate) fn new(sender: watch::Sender<Snapshot>, revision: u64) -> Self {
        let now = Instant::now();
        let wall = SystemTime::now();
        sender.send_modify(|s| {
            s.revision = revision;
            s.epoch += 1;
            s.sequence = 0;
            s.level = Level::Unknown;
            s.state = State::Starting;
            s.at = now;
            s.wall = wall;
            s.trigger.clear();
        });
        Self {
            sender,
            previous: now,
            wall,
        }
    }
    pub fn state(&mut self, state: State) {
        self.sender.send_modify(|s| {
            s.state = state;
            s.level = Level::Unknown;
            s.sequence += 1;
        });
    }
    pub fn devices(&mut self, devices: Vec<crate::Device>) {
        self.sender.send_modify(|s| s.devices = devices);
    }
    #[cfg(target_os = "linux")]
    pub fn trigger(&mut self, trigger: &str) {
        self.sender
            .send_modify(|s| s.trigger = crate::clean(trigger));
    }
    pub fn sample(&mut self, level: Level) {
        let now = Instant::now();
        let wall = SystemTime::now();
        let stalled = now.saturating_duration_since(self.previous) >= FRESHNESS
            || !wall
                .duration_since(self.wall)
                .is_ok_and(|age| age < FRESHNESS);
        self.sender.send_modify(|s| {
            if stalled {
                s.epoch += 1;
            }
            if s.level != level {
                s.sequence += 1;
            }
            s.level = level;
            s.state = State::Ready;
            s.at = now;
            s.wall = wall;
        });
        self.previous = now;
        self.wall = wall;
    }
}
async fn run(
    mode: Mode,
    publisher: &mut Publisher,
    joysticks: &mut Option<crate::joystick::Context>,
) -> Result<(), State> {
    match mode {
        Mode::Off => {
            publisher.state(State::Off);
            Ok(())
        }
        Mode::ScanJoysticks => {
            let context = crate::joystick::context(joysticks)?;
            publisher.devices(context.scan()?);
            publisher.state(State::Off);
            Ok(())
        }
        Mode::Joystick { instance, button } => {
            crate::joystick::run(joysticks, instance, button, publisher).await
        }
        Mode::Keyboard {
            key,
            modifiers,
            description,
        } => {
            let _ = &description;
            #[cfg(target_os = "linux")]
            {
                if std::env::var_os("WAYLAND_DISPLAY").is_some()
                    || std::env::var("XDG_SESSION_TYPE").as_deref() == Ok("wayland")
                {
                    crate::portal::run(key, modifiers, &description, publisher).await
                } else {
                    crate::x11::run(key, modifiers, publisher).await
                }
            }
            #[cfg(target_os = "windows")]
            {
                crate::windows::run(key, modifiers, publisher).await
            }
            #[cfg(not(any(target_os = "windows", target_os = "linux")))]
            {
                let _ = (key, modifiers, publisher);
                Err(State::Unavailable)
            }
        }
    }
}
