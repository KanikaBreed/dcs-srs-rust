//! Voice format ported from `Common/Models/UDPVoicePacket.cs`, and modulation
//! values from `Common/Models/Player/Modulation.cs`, by Ciribob and contributors
//! (GPLv3), revision d01f540ce5e363e9ac841775c8f7e2253138cd81.
//!
//! Unlike the reference decoder, lengths are validated before slicing or
//! allocating. Audio is borrowed, frequencies are bounded by the packet length.

use std::{error::Error, fmt};

pub const ID_BYTES: usize = 22;
pub const HEADER_BYTES: usize = 6;
pub const FREQUENCY_BYTES: usize = 10;
pub const TAIL_BYTES: usize = 57;
/// Wire length-field limit. A socket transport may require a smaller datagram.
pub const MAX_PACKET_BYTES: usize = u16::MAX as usize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PacketError {
    InvalidId,
    TooShort,
    TooLarge,
    LengthMismatch,
    InvalidFrequencyLength,
    InvalidFrequency,
}

impl fmt::Display for PacketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidId => "SRS identity must be 22 URL-safe ASCII characters",
            Self::TooShort => "voice packet is shorter than its header and tail",
            Self::TooLarge => "voice packet exceeds its 16-bit wire length",
            Self::LengthMismatch => "voice packet lengths do not match its contents",
            Self::InvalidFrequencyLength => "frequency segment is not a multiple of ten bytes",
            Self::InvalidFrequency => "frequency must be finite and nonnegative",
        })
    }
}
impl Error for PacketError {}

/// Opaque wire identity. This validates its shape, not the underlying .NET GUID.
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
pub struct ClientId([u8; ID_BYTES]);

impl ClientId {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, PacketError> {
        let bytes: [u8; ID_BYTES] = bytes.try_into().map_err(|_| PacketError::InvalidId)?;
        if !bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
        {
            return Err(PacketError::InvalidId);
        }
        Ok(Self(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; ID_BYTES] {
        &self.0
    }
}

impl fmt::Debug for ClientId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Identifiers need not appear in routine network diagnostics.
        f.write_str("ClientId([redacted])")
    }
}

/// Preserve unknown byte values rather than misclassifying a future modulation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Modulation(pub u8);

impl Modulation {
    pub const AM: Self = Self(0);
    pub const FM: Self = Self(1);
    pub const INTERCOM: Self = Self(2);
    pub const DISABLED: Self = Self(3);
    pub const HAVEQUICK: Self = Self(4);
    pub const SATCOM: Self = Self(5);
    pub const MIDS: Self = Self(6);
    pub const SINCGARS: Self = Self(7);
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frequency {
    pub hz: f64,
    pub modulation: Modulation,
    /// SRS simulated key matching, not transport encryption.
    pub encryption: u8,
}

impl Frequency {
    fn validate(&self) -> Result<(), PacketError> {
        if !self.hz.is_finite() || self.hz < 0.0 {
            return Err(PacketError::InvalidFrequency);
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq)]
pub struct VoicePacket<'a> {
    pub audio: &'a [u8],
    /// Not capped to the UI's 11 radio slots: the wire format has its own bound.
    pub frequencies: Vec<Frequency>,
    pub unit_id: u32,
    pub packet_number: u64,
    pub retransmission_count: u8,
    pub original_client_id: ClientId,
    pub client_id: ClientId,
}

impl fmt::Debug for VoicePacket<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VoicePacket")
            .field("audio_bytes", &self.audio.len())
            .field("frequencies", &self.frequencies.len())
            .field("packet_number", &self.packet_number)
            .field("retransmission_count", &self.retransmission_count)
            .finish_non_exhaustive()
    }
}

impl<'a> VoicePacket<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, PacketError> {
        if bytes.len() > MAX_PACKET_BYTES {
            return Err(PacketError::TooLarge);
        }
        if bytes.len() < HEADER_BYTES + TAIL_BYTES {
            return Err(PacketError::TooShort);
        }
        let packet_len = u16::from_le_bytes([bytes[0], bytes[1]]) as usize;
        let audio_len = u16::from_le_bytes([bytes[2], bytes[3]]) as usize;
        let freq_len = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
        if packet_len != bytes.len() {
            return Err(PacketError::LengthMismatch);
        }
        if !freq_len.is_multiple_of(FREQUENCY_BYTES) {
            return Err(PacketError::InvalidFrequencyLength);
        }
        // Three u16-derived lengths cannot overflow usize on supported targets.
        if HEADER_BYTES + audio_len + freq_len + TAIL_BYTES != packet_len {
            return Err(PacketError::LengthMismatch);
        }
        let freq_start = HEADER_BYTES + audio_len;
        let tail_start = freq_start + freq_len;
        let tail = &bytes[tail_start..];
        let original_client_id = ClientId::from_bytes(&tail[13..35])?;
        let client_id = ClientId::from_bytes(&tail[35..57])?;
        let mut frequencies = Vec::with_capacity(freq_len / FREQUENCY_BYTES);
        for entry in bytes[freq_start..tail_start]
            .as_chunks::<FREQUENCY_BYTES>()
            .0
        {
            let frequency = Frequency {
                hz: f64::from_le_bytes(
                    entry[..8]
                        .try_into()
                        .map_err(|_| PacketError::LengthMismatch)?,
                ),
                modulation: Modulation(entry[8]),
                encryption: entry[9],
            };
            frequency.validate()?;
            frequencies.push(frequency);
        }
        Ok(Self {
            audio: &bytes[HEADER_BYTES..freq_start],
            frequencies,
            unit_id: u32::from_le_bytes(
                tail[..4]
                    .try_into()
                    .map_err(|_| PacketError::LengthMismatch)?,
            ),
            packet_number: u64::from_le_bytes(
                tail[4..12]
                    .try_into()
                    .map_err(|_| PacketError::LengthMismatch)?,
            ),
            retransmission_count: tail[12],
            original_client_id,
            client_id,
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>, PacketError> {
        let freq_len = self
            .frequencies
            .len()
            .checked_mul(FREQUENCY_BYTES)
            .ok_or(PacketError::TooLarge)?;
        let len = (HEADER_BYTES + TAIL_BYTES)
            .checked_add(self.audio.len())
            .and_then(|n| n.checked_add(freq_len))
            .filter(|n| *n <= MAX_PACKET_BYTES)
            .ok_or(PacketError::TooLarge)?;
        for frequency in &self.frequencies {
            frequency.validate()?;
        }
        let mut bytes = Vec::with_capacity(len);
        // Each component is bounded by the checked complete packet length.
        bytes.extend_from_slice(&(len as u16).to_le_bytes());
        bytes.extend_from_slice(&(self.audio.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&(freq_len as u16).to_le_bytes());
        bytes.extend_from_slice(self.audio);
        for frequency in &self.frequencies {
            bytes.extend_from_slice(&frequency.hz.to_le_bytes());
            bytes.push(frequency.modulation.0);
            bytes.push(frequency.encryption);
        }
        bytes.extend_from_slice(&self.unit_id.to_le_bytes());
        bytes.extend_from_slice(&self.packet_number.to_le_bytes());
        bytes.push(self.retransmission_count);
        bytes.extend_from_slice(self.original_client_id.as_bytes());
        bytes.extend_from_slice(self.client_id.as_bytes());
        Ok(bytes)
    }
}

/// The server distinguishes 22-byte identity keepalives from voice datagrams.
#[derive(Clone, Debug, PartialEq)]
pub enum Datagram<'a> {
    Keepalive(ClientId),
    Voice(VoicePacket<'a>),
}

impl<'a> Datagram<'a> {
    pub fn decode(bytes: &'a [u8]) -> Result<Self, PacketError> {
        if bytes.len() == ID_BYTES {
            Ok(Self::Keepalive(ClientId::from_bytes(bytes)?))
        } else {
            VoicePacket::decode(bytes).map(Self::Voice)
        }
    }

    pub fn encode(&self) -> Result<Vec<u8>, PacketError> {
        match self {
            Self::Keepalive(id) => Ok(id.as_bytes().to_vec()),
            Self::Voice(packet) => packet.encode(),
        }
    }
}
