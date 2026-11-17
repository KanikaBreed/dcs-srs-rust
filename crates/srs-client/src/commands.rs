//! Bounded local radio edits. Acknowledgement means a completed TCP write,
//! never server acceptance or permission to transmit.
use crate::{Error, RADIO_COUNT, Radio, Session};
use srs_protocol::voice::Modulation;
use std::time::Duration;
use tokio::{
    sync::{mpsc, oneshot},
    time::{Instant, timeout_at},
};
const REQUEST_AGE: Duration = Duration::from_millis(250);
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RadioChange {
    Tune {
        slot: usize,
        hz: f64,
        modulation: Modulation,
    },
    Guard {
        slot: usize,
        hz: f64,
    },
    Key {
        slot: usize,
        key: Option<u8>,
    },
    Disable {
        slot: usize,
    },
}
pub struct RadioSnapshot {
    pub radios: [Radio; RADIO_COUNT],
    pub revision: u64,
}
pub struct RadioControl(mpsc::Sender<Request>);
pub struct RadioUpdates(pub(crate) mpsc::Receiver<Request>);
pub(crate) struct Request {
    pub change: RadioChange,
    pub expires: Instant,
    pub reply: oneshot::Sender<Result<RadioSnapshot, Error>>,
}
pub fn radio_controls() -> (RadioControl, RadioUpdates) {
    let (tx, rx) = mpsc::channel(4);
    (RadioControl(tx), RadioUpdates(rx))
}
impl RadioControl {
    /// Edits one field against the session's current authoritative radio state.
    /// Expired/abandoned requests are skipped before a write starts. Cancellation
    /// after a write starts does not undo it: obtain a new snapshot before TX.
    pub async fn apply(&self, change: RadioChange) -> Result<RadioSnapshot, Error> {
        let expires = Instant::now() + REQUEST_AGE;
        let (reply, result) = oneshot::channel();
        timeout_at(
            expires,
            self.0.send(Request {
                change,
                expires,
                reply,
            }),
        )
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(|_| Error::Disconnected)?;
        result.await.map_err(|_| Error::Disconnected)?
    }
}
impl Session {
    pub fn radios(&self) -> &[Radio; RADIO_COUNT] {
        &self.radios
    }
    pub(crate) fn edited_radios(&self, change: RadioChange) -> Result<[Radio; RADIO_COUNT], Error> {
        let mut radios = self.radios.clone();
        let slot = match change {
            RadioChange::Tune { slot, .. }
            | RadioChange::Guard { slot, .. }
            | RadioChange::Key { slot, .. }
            | RadioChange::Disable { slot } => slot,
        };
        let radio = radios.get_mut(slot).ok_or(Error::InvalidRadio)?;
        match change {
            RadioChange::Tune { hz, modulation, .. } => {
                radio.frequency_hz = hz;
                radio.modulation = modulation;
            }
            RadioChange::Guard { hz, .. } => radio.guard_hz = hz,
            RadioChange::Key { key, .. } => radio.encryption_key = key,
            RadioChange::Disable { .. } => *radio = Radio::default(),
        }
        self.validate_update(&radios)?;
        Ok(radios)
    }
}
