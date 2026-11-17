//! One input owner, a latest-value channel, and non-revivable 250 ms leases.
use crate::Error;
use std::time::Duration;
use tokio::{sync::watch, time::Instant};

pub const INPUT_LEASE: Duration = Duration::from_millis(250);
#[derive(Clone, Copy)]
pub(crate) struct Intent {
    pub generation: u64,
    pub authorization: Option<u64>,
    pub held: Option<(usize, Instant)>,
}
/// Not cloneable: dropping the sole owner shuts down its session.
pub struct Controller {
    sender: watch::Sender<Intent>,
    radios: srs_client::commands::RadioControl,
    configuring: bool,
}
pub struct InputEvents(
    pub(crate) watch::Receiver<Intent>,
    pub(crate) srs_client::commands::RadioUpdates,
);
pub fn controls() -> (Controller, InputEvents) {
    let (sender, receiver) = watch::channel(Intent {
        generation: 0,
        authorization: None,
        held: None,
    });
    let (radios, updates) = srs_client::commands::radio_controls();
    (
        Controller {
            sender,
            radios,
            configuring: false,
        },
        InputEvents(receiver, updates),
    )
}
impl Controller {
    /// Release PTT before changing radio state. A cancelled update leaves this
    /// controller inhibited until another edit is acknowledged; never infer the
    /// radio state from a dropped write result.
    pub async fn change_radio(
        &mut self,
        change: srs_client::commands::RadioChange,
    ) -> Result<[srs_client::Radio; srs_client::RADIO_COUNT], Error> {
        self.edit_radios(change.into()).await
    }
    /// Apply a complete preset or name edit with the same release, policy and
    /// cancellation guarantees as a single-field edit.
    pub async fn edit_radios(
        &mut self,
        edit: srs_client::commands::RadioEdit,
    ) -> Result<[srs_client::Radio; srs_client::RADIO_COUNT], Error> {
        self.release()?;
        let already_inhibited = self.configuring;
        self.configuring = true;
        let result = tokio::time::timeout(Duration::from_secs(3), self.radios.update(edit)).await;
        match result {
            Ok(result) => match result {
                Ok(snapshot) => {
                    self.configuring = false;
                    self.sender
                        .send_modify(|intent| intent.authorization = Some(snapshot.revision));
                    Ok(snapshot.radios)
                }
                Err(error) => {
                    if matches!(
                        error,
                        srs_client::Error::InvalidRadio | srs_client::Error::PolicyDenied
                    ) {
                        self.configuring = already_inhibited;
                    }
                    Err(Error::Control(error))
                }
            },
            Err(_) => Err(Error::Control(srs_client::Error::Timeout)),
        }
    }

    /// A new intentional press. Never call this to recover a held/revoked press.
    pub fn press(&mut self, radio: usize) -> Result<(), Error> {
        if self.configuring || radio >= srs_client::RADIO_COUNT {
            self.release()?;
            return Err(Error::InvalidInput);
        }
        let generation = self
            .sender
            .borrow()
            .generation
            .checked_add(1)
            .ok_or(Error::InvalidInput)?;
        let authorization = self.sender.borrow().authorization;
        self.sender.send_replace(Intent {
            generation,
            authorization,
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
        let authorization = self.sender.borrow().authorization;
        self.sender.send_replace(Intent {
            generation,
            authorization,
            held: None,
        });
        Ok(())
    }
}
