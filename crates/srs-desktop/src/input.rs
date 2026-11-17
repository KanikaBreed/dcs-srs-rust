#![forbid(unsafe_code)]
//! UI ownership and worker freshness; neither can revive an interrupted hold.
use srs_toc::held::{Action, HeldPtt, Input};
use std::time::{Duration, Instant};

pub const FRESHNESS: Duration = Duration::from_millis(150);
#[derive(Clone, Copy)]
pub struct Intent {
    pub generation: u64,
    pub radio: Option<usize>,
    pub at: Instant,
}
pub struct UiInput {
    keys: [HeldPtt; 2],
    owner: Option<usize>,
    generation: u64,
    last_tick: Instant,
    quiet_since: Option<Instant>,
}
impl UiInput {
    pub fn new(now: Instant) -> Self {
        Self {
            keys: [HeldPtt::default(), HeldPtt::default()],
            owner: None,
            generation: 0,
            last_tick: now,
            quiet_since: Some(now),
        }
    }
    pub fn invalidate(&mut self, now: Instant) {
        for key in &mut self.keys {
            key.event(Input::Invalidate, now);
        }
        self.owner = None;
        self.generation = self.generation.wrapping_add(1);
    }
    pub fn focus(&mut self, focused: bool, now: Instant) {
        self.invalidate(now);
        for key in &mut self.keys {
            key.event(
                if focused {
                    Input::FocusGained
                } else {
                    Input::FocusLost
                },
                now,
            );
        }
    }
    pub fn event(&mut self, source: usize, input: Input, now: Instant) {
        if now.saturating_duration_since(self.last_tick) >= FRESHNESS {
            self.invalidate(now);
            self.quiet_since = Some(now);
        }
        if self.quiet_since.is_some() {
            self.quiet_since = Some(now);
            return;
        }
        match self.keys[source].event(input, now) {
            Action::Press if self.owner.is_none() => {
                self.owner = Some(source);
                self.generation = self.generation.wrapping_add(1);
            }
            Action::Press => {
                // Do not switch owners while a different input is held.
                self.keys[source].event(Input::Invalidate, now);
            }
            Action::Release if self.owner == Some(source) => {
                self.owner = None;
                self.generation = self.generation.wrapping_add(1);
            }
            _ => (),
        }
    }
    pub fn tick(&mut self, now: Instant) {
        if now.saturating_duration_since(self.last_tick) >= FRESHNESS {
            self.invalidate(now);
            self.quiet_since = Some(now);
        } else if self
            .quiet_since
            .is_some_and(|at| now.saturating_duration_since(at) >= FRESHNESS)
        {
            self.quiet_since = None;
        }
        self.last_tick = now;
        if let Some(source) = self.owner
            && self.keys[source].tick(now) == Action::Release
        {
            self.invalidate(now);
        }
    }
    pub fn intent(&self, radio: usize, now: Instant) -> Intent {
        Intent {
            generation: self.generation,
            radio: self.owner.map(|_| radio),
            at: now,
        }
    }
    pub fn armed(&self) -> bool {
        self.keys.iter().any(HeldPtt::armed) && self.quiet_since.is_none()
    }
}

#[derive(Default)]
pub struct Lease {
    generation: Option<u64>,
    active: bool,
}
#[derive(Debug, PartialEq)]
pub enum Update {
    Press(usize),
    Renew,
    Release,
}
impl Lease {
    /// Consume even an ineligible generation, so readiness/freshness recovery
    /// cannot start a previously held action.
    pub fn update(&mut self, intent: Intent, ready: bool, now: Instant) -> Update {
        let fresh = now.saturating_duration_since(intent.at) < FRESHNESS;
        let new = self.generation != Some(intent.generation);
        self.generation = Some(intent.generation);
        if !fresh || !ready || intent.radio.is_none() {
            self.active = false;
            return Update::Release;
        }
        if new {
            self.active = true;
            Update::Press(intent.radio.unwrap_or(0))
        } else if self.active {
            Update::Renew
        } else {
            Update::Release
        }
    }
    pub fn revoke(&mut self) {
        self.active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expired_or_pre_registration_hold_cannot_revive() {
        let now = Instant::now();
        let mut lease = Lease::default();
        let mut intent = Intent {
            generation: 1,
            radio: Some(2),
            at: now,
        };
        assert_eq!(lease.update(intent, false, now), Update::Release);
        assert_eq!(lease.update(intent, true, now), Update::Release);
        intent.generation += 1;
        assert_eq!(lease.update(intent, true, now), Update::Press(2));
        assert_eq!(lease.update(intent, true, now + FRESHNESS), Update::Release);
        intent.at = now + FRESHNESS;
        assert_eq!(lease.update(intent, true, intent.at), Update::Release);
        intent.generation += 1;
        assert_eq!(lease.update(intent, true, intent.at), Update::Press(2));
        lease.revoke();
        assert_eq!(lease.update(intent, true, intent.at), Update::Release);
    }
    #[test]
    fn stall_discards_queued_release_and_press_then_needs_fresh_release() {
        let now = Instant::now();
        let mut ui = UiInput::new(now);
        for ms in [50, 100, 150, 200] {
            ui.tick(now + Duration::from_millis(ms));
        }
        let at = now + Duration::from_millis(200);
        ui.event(0, Input::Release, at);
        ui.event(0, Input::Press, at);
        assert_eq!(ui.intent(1, at).radio, Some(1));
        let at = at + Duration::from_secs(1);
        ui.event(0, Input::Release, at);
        ui.event(0, Input::Press, at);
        assert_eq!(ui.intent(1, at).radio, None);
        for ms in [0, 50, 100, 150, 200] {
            ui.tick(at + Duration::from_millis(ms));
        }
        let at = at + Duration::from_millis(200);
        ui.event(0, Input::Repeat, at);
        ui.event(0, Input::Press, at);
        assert_eq!(ui.intent(1, at).radio, None);
        ui.event(0, Input::Release, at);
        ui.event(0, Input::Press, at);
        assert_eq!(ui.intent(1, at).radio, Some(1));
        ui.focus(false, at);
        ui.focus(true, at);
        ui.event(0, Input::Repeat, at);
        assert_eq!(ui.intent(1, at).radio, None);
    }
}
