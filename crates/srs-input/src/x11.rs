use crate::{FRESHNESS, Level, POLL, State, worker::Publisher};
use std::time::Duration;
use x11rb_async::{
    connection::Connection, protocol::xproto::ConnectionExt, rust_connection::RustConnection,
};
pub(crate) async fn run(key: u8, modifiers: u8, publisher: &mut Publisher) -> Result<(), State> {
    let (connection, _, drive) =
        tokio::time::timeout(Duration::from_secs(3), RustConnection::connect(None))
            .await
            .map_err(|_| State::Unavailable)?
            .map_err(|_| State::Unavailable)?;
    tokio::pin!(drive);
    let poll = async {
        let setup = connection.setup();
        let first = setup.min_keycode;
        let count = setup
            .max_keycode
            .checked_sub(first)
            .and_then(|n| n.checked_add(1))
            .ok_or(State::Failed)?;
        let mapping = tokio::time::timeout(Duration::from_secs(3), async {
            connection
                .get_keyboard_mapping(first, count)
                .await
                .map_err(|_| State::Failed)?
                .reply()
                .await
                .map_err(|_| State::Failed)
        })
        .await
        .map_err(|_| State::Unavailable)??;
        let width = usize::from(mapping.keysyms_per_keycode);
        if width == 0 {
            return Err(State::Unavailable);
        }
        let codes = |symbols: &[u32]| -> Vec<u8> {
            mapping
                .keysyms
                .chunks(width)
                .enumerate()
                .filter(|(_, keys)| keys.iter().any(|symbol| symbols.contains(symbol)))
                .map(|(i, _)| first + i as u8)
                .collect()
        };
        let selected = codes(&[0xffbd + u32::from(key)]);
        if selected.is_empty() {
            return Err(State::Unavailable);
        }
        let modifier_codes = [
            codes(&[0xffe1, 0xffe2]),
            codes(&[0xffe3, 0xffe4]),
            codes(&[0xffe9, 0xffea]),
            codes(&[0xffeb, 0xffec]),
        ];
        loop {
            let state = tokio::time::timeout(FRESHNESS, async {
                connection
                    .query_keymap()
                    .await
                    .map_err(|_| State::Disconnected)?
                    .reply()
                    .await
                    .map_err(|_| State::Disconnected)
            })
            .await
            .map_err(|_| State::Disconnected)??;
            let down = |codes: &[u8]| {
                codes
                    .iter()
                    .any(|code| state.keys[usize::from(*code) / 8] & (1 << (*code % 8)) != 0)
            };
            let actual = modifier_codes
                .iter()
                .enumerate()
                .fold(0, |mask, (bit, codes)| {
                    mask | (u8::from(down(codes)) << bit)
                });
            publisher.sample(if !down(&selected) {
                Level::Released
            } else if actual == modifiers {
                Level::Held
            } else {
                Level::Unknown
            });
            tokio::time::sleep(POLL).await;
        }
    };
    tokio::select! { _ = &mut drive => Err(State::Disconnected), result = poll => result }
}

#[cfg(test)]
#[path = "x11_tests.rs"]
mod tests;
