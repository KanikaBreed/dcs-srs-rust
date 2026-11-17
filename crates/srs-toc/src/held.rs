//! Foreground held-key policy, independent of terminal/OS APIs.
use std::time::{Duration, Instant};
pub const MAX_HOLD: Duration = Duration::from_secs(5);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Input {
    Press,
    Repeat,
    Release,
    FocusLost,
    FocusGained,
    Invalidate,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Action {
    None,
    Press,
    Release,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum State {
    AwaitRelease,
    Ready,
    Held(Instant),
}
pub struct HeldPtt {
    state: State,
    focused: bool,
}
impl Default for HeldPtt {
    fn default() -> Self {
        Self {
            state: State::AwaitRelease,
            focused: true,
        }
    }
}
impl HeldPtt {
    pub fn held(&self) -> bool {
        matches!(self.state, State::Held(_))
    }
    pub fn armed(&self) -> bool {
        self.focused && self.state == State::Ready
    }
    /// A release must be observed before the first press, and after interruption.
    /// Repeats and duplicate presses never rearm or extend the maximum hold.
    pub fn event(&mut self, input: Input, now: Instant) -> Action {
        match input {
            Input::Release => {
                self.state = if self.focused {
                    State::Ready
                } else {
                    State::AwaitRelease
                };
                Action::Release
            }
            Input::Press if self.focused && self.state == State::Ready => {
                self.state = State::Held(now);
                Action::Press
            }
            Input::FocusLost => {
                self.focused = false;
                self.state = State::AwaitRelease;
                Action::Release
            }
            Input::FocusGained => {
                self.focused = true;
                Action::None
            }
            Input::Invalidate => {
                self.state = State::AwaitRelease;
                Action::Release
            }
            _ => Action::None,
        }
    }
    pub fn tick(&mut self, now: Instant) -> Action {
        if let State::Held(start) = self.state
            && (now < start || now.duration_since(start) >= MAX_HOLD)
        {
            return self.event(Input::Invalidate, now);
        }
        Action::None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_calibration_repeats_focus_and_hold_deadline_never_rearm() {
        let now = Instant::now();
        let mut ptt = HeldPtt::default();
        assert_eq!(ptt.event(Input::Press, now), Action::None);
        assert_eq!(ptt.event(Input::Repeat, now), Action::None);
        ptt.event(Input::Release, now);
        assert!(ptt.armed());
        assert_eq!(ptt.event(Input::Press, now), Action::Press);
        assert_eq!(
            ptt.event(Input::Press, now + Duration::from_secs(4)),
            Action::None
        );
        assert_eq!(
            ptt.event(Input::Repeat, now + Duration::from_secs(4)),
            Action::None
        );
        assert_eq!(ptt.tick(now + MAX_HOLD), Action::Release);
        assert_eq!(ptt.event(Input::Press, now + MAX_HOLD), Action::None);
        ptt.event(Input::Release, now);
        ptt.event(Input::Press, now);
        assert_eq!(ptt.event(Input::FocusLost, now), Action::Release);
        ptt.event(Input::Release, now);
        ptt.event(Input::FocusGained, now);
        assert_eq!(ptt.event(Input::Press, now), Action::None);
        ptt.event(Input::Release, now);
        assert_eq!(ptt.event(Input::Press, now), Action::Press);
        assert_eq!(ptt.event(Input::Invalidate, now), Action::Release);
        assert_eq!(ptt.event(Input::Repeat, now), Action::None);
    }
}
