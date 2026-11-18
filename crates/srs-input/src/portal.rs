use crate::{FRESHNESS, Level, POLL, PORTAL_HOLD_LIMIT, State, worker::Publisher};
use ashpd::desktop::global_shortcuts::{GlobalShortcuts, NewShortcut};
use futures_util::StreamExt;
use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime},
};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

fn registry_missing(error: &ashpd::Error) -> bool {
    match error {
        ashpd::Error::PortalNotFound(interface) => {
            interface.as_str() == "org.freedesktop.host.portal.Registry"
        }
        ashpd::Error::Zbus(error) | ashpd::Error::Portal(ashpd::PortalError::ZBus(error)) => {
            match error {
                zbus::Error::MethodError(name, _, _) => matches!(
                    name.as_str(),
                    "org.freedesktop.DBus.Error.UnknownInterface"
                        | "org.freedesktop.DBus.Error.UnknownMethod"
                ),
                zbus::Error::FDO(error) => matches!(
                    error.as_ref(),
                    zbus::fdo::Error::UnknownInterface(_) | zbus::fdo::Error::UnknownMethod(_)
                ),
                _ => false,
            }
        }
        _ => false,
    }
}

fn trigger(key: u8, modifiers: u8) -> String {
    let mut parts = Vec::new();
    for (bit, name) in ["SHIFT", "CTRL", "ALT", "LOGO"].iter().enumerate() {
        if modifiers & (1 << bit) != 0 {
            parts.push((*name).to_string());
        }
    }
    parts.push(format!("F{key}"));
    parts.join("+")
}
pub(crate) async fn run(
    key: u8,
    modifiers: u8,
    description: &str,
    publisher: &mut Publisher,
) -> Result<(), State> {
    // Each attempt owns its D-Bus connection, so cancellation drops its session
    // and outstanding chooser requests instead of retaining a shared proxy.
    let connection = tokio::time::timeout(Duration::from_secs(3), async {
        zbus::connection::Builder::session()?
            .max_queued(32)
            .method_timeout(Duration::from_secs(3))
            .build()
            .await
    })
    .await
    .map_err(|_| State::Unavailable)?
    .map_err(|_| State::Unavailable)?;
    // Register this exact connection before any desktop portal call. Modern
    // GlobalShortcuts rejects anonymous host apps (including cargo/service
    // launches). Sandboxed apps retain their portal-provided identity in ashpd.
    let app_id = crate::APPLICATION_ID.parse().map_err(|_| State::Failed)?;
    match tokio::time::timeout(
        Duration::from_secs(3),
        ashpd::register_host_app_with_connection(connection.clone(), app_id),
    )
    .await
    {
        Ok(Ok(())) => (),
        // Older portals have no Registry: let their normal identity/permission
        // checks decide. Never fall through a registration denial or timeout.
        Ok(Err(error)) if registry_missing(&error) => (),
        _ => return Err(State::Unavailable),
    }
    let proxy = GlobalShortcuts::with_connection(connection.clone())
        .await
        .map_err(|_| State::Unavailable)?;
    let mut signals = proxy
        .receive_all_signals()
        .await
        .map_err(|_| State::Unavailable)?;
    let mut owners = proxy
        .receive_owner_changed()
        .await
        .map_err(|_| State::Unavailable)?;
    let session = tokio::time::timeout(
        Duration::from_secs(3),
        proxy.create_session(Default::default()),
    )
    .await
    .map_err(|_| State::Unavailable)?
    .map_err(|_| State::Unavailable)?;
    let path = serde_json::to_value(&session).map_err(|_| State::Failed)?;
    let path = path.as_str().ok_or(State::Failed)?;
    let mut closed = session
        .receive_closed()
        .await
        .map_err(|_| State::Unavailable)?;
    let preferred = trigger(key, modifiers);
    let shortcuts = [NewShortcut::new("ptt", description).preferred_trigger(preferred.as_str())];
    let result = tokio::time::timeout(
        Duration::from_secs(90),
        proxy.bind_shortcuts(&session, &shortcuts, None, Default::default()),
    )
    .await
    .map_err(|_| State::Denied)?
    .map_err(|_| State::Denied)?;
    let response = result.response().map_err(|_| State::Denied)?;
    let binding = response
        .shortcuts()
        .iter()
        .find(|s| s.id() == "ptt")
        .ok_or(State::Denied)?;
    publisher.trigger(binding.trigger_description());
    // Discard setup-time input. Portals report edges, not current key state:
    // a release must be seen before the first usable press.
    drain(&mut signals)?;
    let mut level = Level::Unknown;
    let mut released = false;
    let mut began = Instant::now();
    let mut last_tick = Instant::now();
    let mut last_wall = SystemTime::now();
    let mut timer = tokio::time::interval(POLL);
    timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let outcome = loop {
        tokio::select! {
            biased;
            _ = connection.closed() => break Err(State::Disconnected),
            _ = owners.next() => break Err(State::Disconnected),
            _ = closed.next() => break Err(State::Disconnected),
            _ = timer.tick() => {
                let now = Instant::now();
                if now.duration_since(last_tick) >= FRESHNESS || !SystemTime::now().duration_since(last_wall).is_ok_and(|gap| gap < FRESHNESS) {
                    level = Level::Unknown; released = false;
                    drain(&mut signals)?;
                }
                last_tick = now;
                last_wall = SystemTime::now();
                if level == Level::Held && now.duration_since(began) >= PORTAL_HOLD_LIMIT {
                    level = Level::Unknown; released = false;
                }
                publisher.sample(level);
            }
            message = signals.next() => {
                let Some(message) = message else { break Err(State::Disconnected) };
                let header = message.header();
                let member = header.member().map(|m| m.as_str()).unwrap_or("");
                if matches!(member, "Activated" | "Deactivated") {
                    let Ok((session_path, id, _, _)) = message.body().deserialize::<(OwnedObjectPath, String, u64, HashMap<String, OwnedValue>)>() else { break Err(State::Failed) };
                    if session_path.as_str() != path || id != "ptt" { continue; }
                    if member == "Deactivated" { level = Level::Released; released = true; }
                    else if released && level != Level::Held { level = Level::Held; began = Instant::now(); }
                    publisher.sample(level);
                } else if member == "ShortcutsChanged" {
                    // A reconfigured binding requires explicit re-enabling.
                    break Err(State::Disconnected);
                }
            }
        }
    };
    let _ = tokio::time::timeout(Duration::from_millis(300), session.close()).await;
    outcome
}
use futures_util::FutureExt;

#[cfg(test)]
mod tests {
    #[test]
    fn preferred_trigger_uses_xdg_modifier_names() {
        assert_eq!(super::trigger(8, 0), "F8");
        assert_eq!(super::trigger(12, 15), "SHIFT+CTRL+ALT+LOGO+F12");
    }
}

#[cfg(test)]
#[path = "portal_tests.rs"]
mod native_tests;

fn drain<S: futures_util::Stream + Unpin>(stream: &mut S) -> Result<(), State> {
    for _ in 0..32 {
        match stream.next().now_or_never() {
            None => return Ok(()),
            Some(None) => return Err(State::Disconnected),
            Some(Some(_)) => (),
        }
    }
    // A flooded queue cannot be interpreted as fresh physical edges.
    Err(State::Failed)
}
