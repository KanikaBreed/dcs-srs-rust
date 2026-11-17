//! Receive matching translated from PlayerRadioInfoBase.CanHearTransmission and
//! desktop DCSPlayerRadioInfo.CanHearTransmission, Ciribob and contributors,
//! GPLv3, upstream d01f540ce5e363e9ac841775c8f7e2253138cd81.
//! Pure domain rules: no sockets, clocks, codecs, devices, or UI dependencies.

use srs_protocol::voice::{Frequency, Modulation};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReceiveRadio {
    pub frequency_hz: f64,
    pub guard_hz: f64,
    pub modulation: Modulation,
    pub encryption_key: Option<u8>,
    pub intercom_unit_id: u32,
}

impl Default for ReceiveRadio {
    fn default() -> Self {
        Self {
            frequency_hz: 1.0,
            guard_hz: 1.0,
            modulation: Modulation::DISABLED,
            encryption_key: None,
            intercom_unit_id: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Reception {
    pub radio: usize,
    pub secondary: bool,
    /// Simulated SRS encryption only; no cryptographic secrecy is provided.
    pub decryptable: bool,
}

pub fn frequency_matches(a: f64, b: f64) -> bool {
    a.is_finite() && b.is_finite() && a >= 0.0 && b >= 0.0 && (a - b).abs() < 500.0
}

/// Matches upstream search order, including modulation-independent guard and
/// intercom bypass. `blocked` is a slot bitmask. Callers must also apply the
/// desktop's post-match blocked-radio check, except for intercom, MIDS, or global.
/// Unlike upstream, the fallback guard result never inherits another radio's
/// decryptable flag. See docs/RECEIVE.md and the independent C# fixture regression.
pub fn match_radio(
    radios: &[ReceiveRadio],
    local_unit: u32,
    incoming: Frequency,
    sender_unit: u32,
    strict: bool,
    blocked: u16,
) -> Option<Reception> {
    if incoming.modulation == Modulation::DISABLED || !incoming.hz.is_finite() || incoming.hz < 0.0
    {
        return None;
    }
    let mut fallback = None;
    for (slot, radio) in radios.iter().take(11).enumerate() {
        if radio.modulation == Modulation::DISABLED {
            continue;
        }
        let reception = |secondary, decryptable| Reception {
            radio: slot,
            secondary,
            decryptable,
        };
        if radio.modulation == Modulation::INTERCOM && incoming.modulation == Modulation::INTERCOM {
            if sender_unit > 0
                && (local_unit == sender_unit || radio.intercom_unit_id == sender_unit)
            {
                return Some(reception(false, true));
            }
            continue;
        }
        let decryptable = radio.encryption_key.unwrap_or(0) == incoming.encryption
            || (!strict && incoming.encryption == 0);
        if radio.frequency_hz > 10_000.0
            && radio.modulation == incoming.modulation
            && frequency_matches(radio.frequency_hz, incoming.hz)
        {
            let matched = reception(false, decryptable);
            if decryptable && blocked & (1 << slot) == 0 {
                return Some(matched);
            }
            fallback = Some(matched);
        }
        if radio.guard_hz > 10_000.0 && frequency_matches(radio.guard_hz, incoming.hz) {
            let matched = reception(true, decryptable);
            if decryptable {
                return Some(matched);
            }
            fallback = Some(matched);
        }
    }
    fallback
}

/// Desktop IRL_RADIO_TX blocks these modulations while PTT is held.
pub fn blocks_while_transmitting(modulation: Modulation) -> bool {
    matches!(
        modulation,
        Modulation::AM
            | Modulation::FM
            | Modulation::HAVEQUICK
            | Modulation::MIDS
            | Modulation::SINCGARS
    )
}
