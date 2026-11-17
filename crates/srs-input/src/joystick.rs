use crate::{Device, Level, POLL, State, worker::Publisher};
pub(crate) struct Context {
    _sdl: sdl3::Sdl,
    subsystem: sdl3::JoystickSubsystem,
}
pub(crate) fn context(slot: &mut Option<Context>) -> Result<&mut Context, State> {
    if slot.is_none() {
        sdl3::hint::set("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1");
        let sdl = sdl3::init().map_err(|_| State::Unavailable)?;
        let subsystem = sdl.joystick().map_err(|_| State::Unavailable)?;
        subsystem.set_joystick_events_enabled(false); // Poll state, never accumulate SDL events.
        *slot = Some(Context {
            _sdl: sdl,
            subsystem,
        });
    }
    slot.as_mut().ok_or(State::Failed)
}
impl Context {
    pub fn scan(&self) -> Result<Vec<Device>, State> {
        self.subsystem.update();
        let ids = self.subsystem.joysticks().map_err(|_| State::Unavailable)?;
        let mut result = Vec::new();
        for id in ids.into_iter().take(32) {
            if let Ok(device) = self.subsystem.open(id) {
                result.push(Device {
                    instance: device.id().raw(),
                    guid: device.guid().to_string(),
                    name: crate::clean(&device.name()),
                    buttons: device.num_buttons().min(128),
                });
            }
        }
        Ok(result)
    }
    fn open(&self, instance: u32) -> Result<sdl3::joystick::Joystick, State> {
        self.subsystem
            .open(sdl3::joystick::JoystickId::new(instance))
            .map_err(|_| State::Disconnected)
    }
}
pub(crate) async fn run(
    slot: &mut Option<Context>,
    instance: u32,
    button: u32,
    publisher: &mut Publisher,
) -> Result<(), State> {
    let context = context(slot)?;
    let device = context.open(instance)?;
    if button >= device.num_buttons() {
        return Err(State::Failed);
    }
    loop {
        context.subsystem.update();
        if !device.connected() {
            return Err(State::Disconnected);
        }
        let held = device.button(button).map_err(|_| State::Disconnected)?;
        publisher.sample(if held { Level::Held } else { Level::Released });
        tokio::time::sleep(POLL).await;
    }
}

#[cfg(test)]
#[path = "joystick_tests.rs"]
mod tests;
