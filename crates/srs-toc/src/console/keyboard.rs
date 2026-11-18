//! Foreground terminal adapter. It never installs a global keyboard hook.
use super::{announce, show_radio};
#[cfg(unix)]
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::{
    event::{
        self, DisableFocusChange, EnableFocusChange, Event, KeyCode, KeyEventKind, KeyModifiers,
    },
    execute, terminal,
};
use srs_client::{RADIO_COUNT, Radio, commands::RadioChange};
use srs_protocol::voice::Modulation;
use srs_toc::{
    Controller, Phase, Status,
    held::{Action, HeldPtt, Input},
};
use std::{
    error::Error,
    io::{self, IsTerminal},
    time::Duration,
};
use tokio::{
    sync::watch,
    time::{Instant, MissedTickBehavior, interval},
};
const HELP: &str = "KEYBOARD: tap/release F8 to arm; hold F8 to transmit (maximum 5s). 1-9 select radio (0 selects 10); f edits MHz; m toggles AM/FM; x disables radio; q quits. Focus loss/retuning requires releasing F8 again.";
const REARM: &str = "PTT RELEASED: tap/release F8 before the next hold.";
pub struct TerminalGuard {
    #[cfg(unix)]
    enhanced: bool,
}
impl TerminalGuard {
    pub fn open() -> io::Result<Self> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(io::Error::other(
                "keyboard PTT requires a foreground terminal",
            ));
        }
        terminal::enable_raw_mode()?;
        let guard = Self {
            #[cfg(unix)]
            enhanced: false,
        };
        #[cfg(unix)]
        let mut guard = guard;
        #[cfg(unix)]
        {
            if !terminal::supports_keyboard_enhancement()? {
                return Err(io::Error::other(
                    "terminal lacks key-release reporting; use timed mode",
                ));
            }
            // Mark before write so partial setup still attempts to pop on error.
            guard.enhanced = true;
            execute!(
                io::stdout(),
                PushKeyboardEnhancementFlags(
                    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
                        | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                )
            )?;
        }
        execute!(io::stdout(), EnableFocusChange)?;
        // Native Windows events distinguish F8 from pasted text; the legacy
        // WinAPI does not implement bracketed-paste terminal commands.
        #[cfg(unix)]
        execute!(io::stdout(), EnableBracketedPaste)?;
        Ok(guard)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        if self.enhanced {
            let _ = execute!(io::stdout(), PopKeyboardEnhancementFlags);
        }
        let _ = execute!(io::stdout(), DisableFocusChange);
        #[cfg(unix)]
        let _ = execute!(io::stdout(), DisableBracketedPaste);
        let _ = terminal::disable_raw_mode();
    }
}
fn invalidate(ptt: &mut HeldPtt, owner: &mut Controller) -> Result<(), Box<dyn Error>> {
    ptt.event(Input::Invalidate, std::time::Instant::now());
    owner.release()?;
    announce(REARM)?;
    Ok(())
}
async fn change(
    owner: &mut Controller,
    radios: &mut [Radio; RADIO_COUNT],
    edit: RadioChange,
    selected: usize,
) -> Result<(), Box<dyn Error>> {
    match owner.change_radio(edit).await {
        Ok(snapshot) => {
            *radios = snapshot;
            show_radio(selected, &radios[selected])?;
        }
        Err(srs_toc::Error::Control(
            srs_client::Error::InvalidRadio | srs_client::Error::PolicyDenied,
        )) => announce("RADIO UPDATE REJECTED: settings are invalid or denied by server policy.")?,
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
pub async fn operate(
    mut owner: Controller,
    mut status: watch::Receiver<Status>,
    seconds: u64,
    mut radios: [Radio; RADIO_COUNT],
) -> Result<(), Box<dyn Error>> {
    announce(HELP)?;
    let mut selected = 1;
    show_radio(selected, &radios[selected])?;
    let mut ptt = HeldPtt::default();
    let end = Instant::now() + Duration::from_secs(seconds);
    let mut ticker = interval(Duration::from_millis(25));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut last_poll = Instant::now();
    let mut draining = false;
    let mut phase = Phase::Connecting;
    let mut edit: Option<String> = None;
    loop {
        ticker.tick().await;
        let now = Instant::now();
        if now >= end || status.has_changed().is_err() {
            return Ok(());
        }
        if now.duration_since(last_poll) >= srs_toc::INPUT_LEASE {
            invalidate(&mut ptt, &mut owner)?;
            draining = true;
            edit = None;
        }
        last_poll = now;
        let current = status.borrow_and_update().phase;
        if current != phase {
            if phase == Phase::Transmitting && current != Phase::Transmitting && ptt.held() {
                invalidate(&mut ptt, &mut owner)?;
            }
            phase = current;
            announce(match phase {
                Phase::Connecting => "CONNECTING",
                Phase::Receiving => "RX READY",
                Phase::Transmitting => "TX LOCAL: delivery is not acknowledged.",
                Phase::Stopped => "STOPPED",
            })?;
            if phase == Phase::Stopped {
                return Ok(());
            }
        }
        if ptt.tick(now.into_std()) == Action::Release {
            owner.release()?;
            announce(REARM)?;
        }
        let mut budget_exhausted = true;
        for _ in 0..64 {
            if !event::poll(Duration::ZERO)? {
                budget_exhausted = false;
                if draining {
                    draining = false;
                    announce("INPUT READY: tap/release F8 to rearm.")?;
                }
                break;
            }
            let event = event::read()?;
            if draining {
                continue;
            }
            let action = match event {
                Event::FocusLost => {
                    edit = None;
                    ptt.event(Input::FocusLost, now.into_std())
                }
                Event::FocusGained => ptt.event(Input::FocusGained, now.into_std()),
                Event::Paste(_) => {
                    edit = None;
                    ptt.event(Input::Invalidate, now.into_std())
                }
                Event::Key(key) if key.code == KeyCode::F(8) => {
                    let input = match key.kind {
                        KeyEventKind::Release => Input::Release,
                        KeyEventKind::Repeat => Input::Repeat,
                        KeyEventKind::Press => Input::Press,
                    };
                    if input == Input::Press
                        && !ptt.held()
                        && (edit.is_some()
                            || !key.modifiers.is_empty()
                            || phase != Phase::Receiving
                            || radios[selected].modulation == Modulation::DISABLED
                            || radios[selected].frequency_hz <= 10_000.0)
                    {
                        ptt.event(Input::Invalidate, now.into_std())
                    } else {
                        ptt.event(input, now.into_std())
                    }
                }
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        return Ok(());
                    }
                    if let Some(text) = &mut edit {
                        match key.code {
                            KeyCode::Esc => {
                                edit = None;
                                announce("Frequency edit cancelled.")?;
                            }
                            KeyCode::Backspace => {
                                text.pop();
                                announce(&format!("MHz> {text}"))?;
                            }
                            KeyCode::Char(c)
                                if (c.is_ascii_digit() || c == '.') && text.len() < 16 =>
                            {
                                text.push(c);
                                announce(&format!("MHz> {text}"))?;
                            }
                            KeyCode::Enter => {
                                let hz = text
                                    .parse::<f64>()
                                    .ok()
                                    .map(|n| n * 1e6)
                                    .filter(|n| n.is_finite() && (10_001.0..=10e9).contains(n));
                                edit = None;
                                if let Some(hz) = hz {
                                    let modulation =
                                        if radios[selected].modulation == Modulation::DISABLED {
                                            Modulation::AM
                                        } else {
                                            radios[selected].modulation
                                        };
                                    change(
                                        &mut owner,
                                        &mut radios,
                                        RadioChange::Tune {
                                            slot: selected,
                                            hz,
                                            modulation,
                                        },
                                        selected,
                                    )
                                    .await?;
                                    draining = true;
                                } else {
                                    announce("Invalid frequency.")?;
                                }
                            }
                            _ => (),
                        }
                    } else {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                            KeyCode::Char(c @ '0'..='9') => {
                                invalidate(&mut ptt, &mut owner)?;
                                selected = if c == '0' {
                                    10
                                } else {
                                    (c as u8 - b'0') as usize
                                };
                                show_radio(selected, &radios[selected])?;
                            }
                            KeyCode::Char('f') => {
                                invalidate(&mut ptt, &mut owner)?;
                                edit = Some(String::new());
                                announce("MHz> type frequency then Enter; Escape cancels.")?;
                            }
                            KeyCode::Char('m') if radios[selected].frequency_hz > 10_000.0 => {
                                invalidate(&mut ptt, &mut owner)?;
                                let modulation = if radios[selected].modulation == Modulation::FM {
                                    Modulation::AM
                                } else {
                                    Modulation::FM
                                };
                                let hz = radios[selected].frequency_hz;
                                change(
                                    &mut owner,
                                    &mut radios,
                                    RadioChange::Tune {
                                        slot: selected,
                                        hz,
                                        modulation,
                                    },
                                    selected,
                                )
                                .await?;
                                draining = true;
                            }
                            KeyCode::Char('x') => {
                                invalidate(&mut ptt, &mut owner)?;
                                change(
                                    &mut owner,
                                    &mut radios,
                                    RadioChange::Disable { slot: selected },
                                    selected,
                                )
                                .await?;
                                draining = true;
                            }
                            _ => (),
                        }
                    }
                    Action::None
                }
                _ => Action::None,
            };
            match action {
                Action::Press => owner.press(selected)?,
                Action::Release => {
                    owner.release()?;
                    if ptt.armed() {
                        announce("PTT ARMED: hold F8 to transmit.")?;
                    } else {
                        announce(REARM)?;
                    }
                }
                Action::None => (),
            }
        }
        // Only a full batch can signal a backlog. A key may arrive after the
        // empty-queue check (especially in response to INPUT READY); it belongs
        // to the next tick and must not be discarded as overflow.
        if budget_exhausted && event::poll(Duration::ZERO)? {
            invalidate(&mut ptt, &mut owner)?;
            draining = true;
        }
        if ptt.held() && owner.renew().is_err() {
            invalidate(&mut ptt, &mut owner)?;
        }
    }
}
