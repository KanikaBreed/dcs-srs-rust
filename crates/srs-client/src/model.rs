//! Standalone state derived from SRClientBase, RadioBase, PlayerRadioInfoBase,
//! and DCSRadioSyncHandler (Ciribob and contributors, GPLv3), upstream d01f540c.

use serde_json::{Map, Value, json};
use srs_protocol::voice::{ClientId, Modulation};

use crate::Error;

pub const RADIO_COUNT: usize = 11;
pub const EAM_UNIT_ID: u32 = 100_000_001;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Coalition {
    Red = 1,
    Blue = 2,
}

impl Coalition {
    pub(crate) fn parse(value: u64) -> Result<Option<Self>, Error> {
        match value {
            0 => Ok(None),
            1 => Ok(Some(Self::Red)),
            2 => Ok(Some(Self::Blue)),
            _ => Err(Error::InvalidMessage),
        }
    }
}

/// Local identity; the session always starts as a spectator. A password response
/// from the server, never a caller-supplied coalition, authorizes EAM updates.
#[derive(Clone)]
pub struct Identity {
    pub id: ClientId,
    pub callsign: String,
    pub unit_id: u32,
    pub allow_recording: bool,
}

impl Identity {
    pub fn new(id: ClientId, callsign: impl Into<String>) -> Self {
        Self {
            id,
            callsign: callsign.into(),
            unit_id: EAM_UNIT_ID,
            allow_recording: false,
        }
    }

    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.callsign.trim().is_empty()
            || self.callsign.len() > 128
            || self.callsign.chars().any(char::is_control)
            || self.unit_id < EAM_UNIT_ID
        {
            return Err(Error::InvalidIdentity);
        }
        Ok(())
    }
}

/// One radio slot. Frequencies are Hz; encryption is SRS simulated key matching.
#[derive(Clone, PartialEq)]
pub struct Radio {
    pub frequency_hz: f64,
    pub guard_hz: f64,
    pub modulation: Modulation,
    pub encryption_key: Option<u8>,
    pub retransmit: bool,
    pub intercom_unit_id: u32,
    pub name: String,
    pub model: String,
}

impl Default for Radio {
    fn default() -> Self {
        Self {
            frequency_hz: 1.0,
            guard_hz: 1.0,
            modulation: Modulation::DISABLED,
            encryption_key: None,
            retransmit: false,
            intercom_unit_id: 0,
            name: String::new(),
            model: String::new(),
        }
    }
}

impl Radio {
    pub fn tuned(frequency_hz: f64, modulation: Modulation) -> Self {
        Self {
            frequency_hz,
            modulation,
            ..Self::default()
        }
    }

    pub(crate) fn validate(&self) -> Result<(), Error> {
        if !self.frequency_hz.is_finite()
            || self.frequency_hz < 0.0
            || !self.guard_hz.is_finite()
            || self.guard_hz < 0.0
            || self.modulation.0 > 7
            || self.name.len() > 128
            || self.model.len() > 128
        {
            return Err(Error::InvalidRadio);
        }
        Ok(())
    }

    fn wire(&self) -> Value {
        // Match RadioBase's ASCII-alphanumeric normalization and 32-char cap.
        fn normalized(value: &str) -> String {
            value
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .take(32)
                .map(|c| c.to_ascii_lowercase())
                .collect()
        }
        json!({
            "freq": self.frequency_hz, "secFreq": self.guard_hz,
            "modulation": self.modulation.0, "enc": self.encryption_key.is_some(),
            "encKey": self.encryption_key.unwrap_or(0), "retransmit": self.retransmit,
            "IntercomUnitId": self.intercom_unit_id,
            "Name": normalized(&self.name), "Model": normalized(&self.model),
        })
    }
}

pub fn disabled_radios() -> [Radio; RADIO_COUNT] {
    std::array::from_fn(|_| Radio::default())
}

pub(crate) fn client_wire(
    identity: &Identity,
    coalition: Option<Coalition>,
    radios: &[Radio; RADIO_COUNT],
) -> Map<String, Value> {
    let mut client = Map::new();
    client.insert(
        "ClientGuid".into(),
        Value::String(String::from_utf8_lossy(identity.id.as_bytes()).into_owned()),
    );
    client.insert("Name".into(), Value::String(identity.callsign.clone()));
    client.insert("Coalition".into(), json!(coalition.map_or(0, |c| c as u8)));
    client.insert("AllowRecord".into(), json!(identity.allow_recording));
    client.insert("Seat".into(), json!(0));
    client.insert("Gateway".into(), json!(false));
    client.insert("GatewayClient".into(), json!(false));
    client.insert("DISEntityId".into(), json!(-1));
    client.insert(
        "LatLngPosition".into(),
        json!({"lat": 0.0, "lng": 0.0, "alt": 0.0}),
    );
    client.insert("RadioInfo".into(), json!({
        "unit": if coalition.is_some() { "EAM" } else { "" },
        "unitId": if coalition.is_some() { identity.unit_id } else { 0 },
        "radios": radios.iter().map(Radio::wire).collect::<Vec<_>>(),
        "ambient": {"vol": 0.0, "abType": ""},
        "iff": {"control": 2, "mic": -1, "mode1": -1, "mode2": -1, "mode3": -1, "mode4": false, "status": 0},
    }));
    client
}
