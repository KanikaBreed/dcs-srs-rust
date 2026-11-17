//! One input owner, a latest-value channel, and non-revivable 250 ms leases.
use crate::Error;
use std::time::Duration;
use tokio::{sync::watch, time::Instant};

pub const INPUT_LEASE: Duration = Duration::from_millis(250);
#[derive(Clone, Copy)]
pub(crate) struct Intent {
    pub generation: u64,
    pub held: Option<(usize, Instant)>,
}
/// Not cloneable: dropping the sole owner shuts down its session.
pub struct Controller {
    sender: watch::Sender<Intent>,
}
pub struct InputEvents(pub(crate) watch::Receiver<Intent>);
pub fn controls() -> (Controller, InputEvents) {
    let (sender, receiver) = watch::channel(Intent {
        generation: 0,
        held: None,
    });
    (Controller { sender }, InputEvents(receiver))
}
impl Controller {
    /// A new intentional press. Never call this to recover a held/revoked press.
    pub fn press(&mut self, radio: usize) -> Result<(), Error> {
        if radio >= srs_client::RADIO_COUNT {
            self.release()?;
            return Err(Error::InvalidInput);
        }
        let generation = self
            .sender
            .borrow()
            .generation
            .checked_add(1)
            .ok_or(Error::InvalidInput)?;
        self.sender.send_replace(Intent {
            generation,
            held: Some((radio, Instant::now() + INPUT_LEASE)),
        });
        Ok(())
    }
    /// Only an active owner may renew. An expired lease requires a fresh press.
    pub fn renew(&mut self) -> Result<(), Error> {
        let previous = *self.sender.borrow();
        let now = Instant::now();
        let Some((radio, _)) = previous.held.filter(|(_, until)| *until > now) else {
            self.release()?;
            return Err(Error::InputExpired);
        };
        self.sender.send_replace(Intent {
            held: Some((radio, now + INPUT_LEASE)),
            ..previous
        });
        Ok(())
    }
    pub fn release(&mut self) -> Result<(), Error> {
        let generation = self
            .sender
            .borrow()
            .generation
            .checked_add(1)
            .ok_or(Error::InvalidInput)?;
        self.sender.send_replace(Intent {
            generation,
            held: None,
        });
        Ok(())
    }
}
