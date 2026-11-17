//! Bounded display data derived only from the server-visible session snapshot.
use srs_client::{Coalition, Session, State};

pub const DISPLAY_CLIENT_LIMIT: usize = 256;
pub const POLICY_KEYS: [&str; 15] = [
    "COALITION_AUDIO_SECURITY",
    "SPECTATORS_AUDIO_DISABLED",
    "LOS_ENABLED",
    "DISTANCE_ENABLED",
    "IRL_RADIO_TX",
    "IRL_RADIO_RX_INTERFERENCE",
    "RADIO_EXPANSION",
    "EXTERNAL_AWACS_MODE",
    "ALLOW_RADIO_ENCRYPTION",
    "STRICT_RADIO_ENCRYPTION",
    "SHOW_TUNED_COUNT",
    "SHOW_TRANSMITTER_NAME",
    "SERVER_PRESETS_ENABLED",
    "SERVER_EAM_RADIO_PRESET_ENABLED",
    "ALLOW_INSTRUCTOR_MODE",
];
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PeerSummary {
    pub name: String,
    pub coalition: Option<u8>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionDetails {
    pub version: String,
    pub coalition: Option<Coalition>,
    pub total_clients: usize,
    pub clients: Vec<PeerSummary>,
    pub policy: [Option<bool>; POLICY_KEYS.len()],
    pub retransmission_limit: Option<u32>,
}
fn display_name(value: &str) -> String {
    value
        .chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
        .take(80)
        .collect()
}
impl SessionDetails {
    pub fn read(session: &Session) -> Self {
        Self {
            version: display_name(session.server_version().unwrap_or_default()),
            coalition: match session.state() {
                State::Authenticated(c) => Some(c),
                _ => None,
            },
            total_clients: session.clients().len(),
            clients: session
                .clients()
                .values()
                .take(DISPLAY_CLIENT_LIMIT)
                .map(|peer| PeerSummary {
                    name: display_name(
                        peer.get("Name")
                            .and_then(|v| v.as_str())
                            .unwrap_or_default(),
                    ),
                    coalition: peer
                        .get("Coalition")
                        .and_then(|v| v.as_u64())
                        .filter(|n| *n <= 2)
                        .map(|n| n as u8),
                })
                .collect(),
            policy: POLICY_KEYS.map(|key| {
                session
                    .settings()
                    .get(key)
                    .and_then(|value| match value.trim() {
                        v if v.eq_ignore_ascii_case("true") => Some(true),
                        v if v.eq_ignore_ascii_case("false") => Some(false),
                        _ => None,
                    })
            }),
            retransmission_limit: session
                .settings()
                .get("RETRANSMISSION_NODE_LIMIT")
                .and_then(|s| s.trim().parse().ok()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn untrusted_names_cannot_insert_lines_or_bidi_overrides_and_are_bounded() {
        assert_eq!(display_name("Alpha\n\t\u{202e}Bravo\u{2066}"), "AlphaBravo");
        assert_eq!(display_name(&"é".repeat(300)).chars().count(), 80);
    }
}
