//! Desktop input only. No audio, recording, keyboard text, or network session.
mod joystick;
#[cfg(target_os = "linux")]
mod portal;
#[cfg(target_os = "windows")]
#[allow(unsafe_code)]
mod windows;
mod worker;
#[cfg(target_os = "linux")]
mod x11;
use std::time::{Duration, Instant, SystemTime};
pub use worker::Worker;

/// Shared desktop/portal identity; matches the development launcher's filename.
pub const APPLICATION_ID: &str = "io.github.KanikaBreed.DcsSrsRust";

pub const POLL: Duration = Duration::from_millis(20);
pub const FRESHNESS: Duration = Duration::from_millis(150);
pub const PORTAL_HOLD_LIMIT: Duration = Duration::from_secs(120);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    ScanJoysticks,
    Keyboard {
        key: u8,
        modifiers: u8,
        description: String,
    },
    Joystick {
        instance: u32,
        button: u32,
    },
}
impl Mode {
    fn valid(&self) -> bool {
        match self {
            Self::Keyboard {
                key,
                modifiers,
                description,
            } => {
                (1..=12).contains(key)
                    && *modifiers < 16
                    && !description.is_empty()
                    && description.len() <= 512
            }
            Self::Joystick { button, .. } => *button < 128,
            _ => true,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Level {
    #[default]
    Unknown,
    Released,
    Held,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Off,
    Starting,
    Ready,
    Unavailable,
    Denied,
    Disconnected,
    Failed,
}
#[derive(Clone, Debug)]
pub struct Device {
    pub instance: u32,
    pub guid: String,
    pub name: String,
    pub buttons: u32,
}
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub revision: u64,
    pub epoch: u64,
    pub sequence: u64,
    pub level: Level,
    pub state: State,
    pub at: Instant,
    pub wall: SystemTime,
    pub devices: Vec<Device>,
    pub trigger: String,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            revision: 0,
            epoch: 0,
            sequence: 0,
            level: Level::Unknown,
            state: State::Off,
            at: Instant::now(),
            wall: SystemTime::now(),
            devices: vec![],
            trigger: String::new(),
        }
    }
}
/// Accept physical state only after release; stalls and skipped edges cannot
/// resume a hold. This policy has no dependence on window focus or audio APIs.
#[derive(Default)]
pub struct Gate {
    epoch: Option<(u64, u64)>,
    sequence: u64,
    armed: bool,
    held: bool,
    after: Option<Instant>,
}
impl Gate {
    pub fn invalidate(&mut self) {
        self.invalidate_at(Instant::now());
    }
    pub fn invalidate_at(&mut self, now: Instant) {
        self.after = Some(now);
        self.clear();
    }
    fn clear(&mut self) {
        self.armed = false;
        self.held = false;
    }
    pub fn held(&self) -> bool {
        self.held
    }
    pub fn armed(&self) -> bool {
        self.armed
    }
    pub fn update(&mut self, sample: &Snapshot, now: Instant, wall: SystemTime) {
        let age_ok = now
            .checked_duration_since(sample.at)
            .is_some_and(|age| age < FRESHNESS)
            && wall
                .duration_since(sample.wall)
                .is_ok_and(|age| age < FRESHNESS);
        let epoch = (sample.revision, sample.epoch);
        if self.epoch != Some(epoch) || sample.sequence > self.sequence.saturating_add(1) {
            self.clear();
        }
        self.epoch = Some(epoch);
        self.sequence = sample.sequence;
        if !age_ok
            || sample.state != State::Ready
            || sample.level == Level::Unknown
            || self.after.is_some_and(|after| sample.at < after)
        {
            self.clear();
        } else if sample.level == Level::Released {
            self.after = None;
            self.armed = true;
            self.held = false;
        } else {
            self.held = self.armed;
        }
    }
}

pub(crate) fn clean(value: &str) -> String {
    value
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(80)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_hold_outlives_foreground_cap_but_not_stale_or_interrupted_input() {
        let mut sample = Snapshot {
            state: State::Ready,
            level: Level::Held,
            ..Snapshot::default()
        };
        let mut gate = Gate::default();
        gate.update(&sample, sample.at, sample.wall);
        assert!(!gate.held());
        sample.level = Level::Released;
        sample.sequence += 1;
        gate.update(&sample, sample.at, sample.wall);
        sample.level = Level::Held;
        sample.sequence += 1;
        for _ in 0..1000 {
            sample.at += POLL;
            sample.wall += POLL;
            gate.update(&sample, sample.at, sample.wall);
            assert!(gate.held());
        }
        gate.update(&sample, sample.at + FRESHNESS, sample.wall + FRESHNESS);
        assert!(!gate.held());
        sample.at += FRESHNESS;
        sample.wall += FRESHNESS;
        gate.update(&sample, sample.at, sample.wall);
        assert!(!gate.held());
        sample.level = Level::Released;
        sample.sequence += 1;
        gate.update(&sample, sample.at, sample.wall);
        sample.level = Level::Held;
        sample.sequence += 1;
        gate.update(&sample, sample.at, sample.wall);
        assert!(gate.held());
        sample.sequence += 2;
        gate.update(&sample, sample.at, sample.wall);
        assert!(
            !gate.held(),
            "unseen release/press must not restart transmission"
        );
    }
    #[test]
    fn sleep_wall_clock_and_backend_restarts_invalidate_even_with_fresh_monotonic_time() {
        let mut gate = Gate::default();
        let mut sample = Snapshot {
            state: State::Ready,
            level: Level::Released,
            ..Snapshot::default()
        };
        gate.update(&sample, sample.at, sample.wall);
        sample.level = Level::Held;
        sample.sequence += 1;
        gate.update(&sample, sample.at, sample.wall);
        assert!(gate.held());
        gate.update(&sample, sample.at, sample.wall + Duration::from_secs(600));
        assert!(!gate.held());
        sample.epoch += 1;
        gate.update(&sample, sample.at, sample.wall);
        assert!(!gate.held());
    }
}
