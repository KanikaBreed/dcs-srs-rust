//! Codec settings from Common/Constants.cs and desktop AudioManager.cs, Ciribob
//! and contributors (GPLv3), upstream d01f540ce5e363e9ac841775c8f7e2253138cd81.
//! Run on a codec worker, not a device callback. Callers own fixed-size buffers;
//! codec state is allocated at construction and belongs to one audio stream.

use std::fmt;

pub mod jitter;

pub const INPUT_RATE: u32 = 16_000;
pub const OUTPUT_RATE: u32 = 48_000;
pub const FRAME_MS: usize = 40;
pub const INPUT_SAMPLES: usize = 640;
pub const OUTPUT_SAMPLES: usize = 1920;
pub const MAX_OPUS_BYTES: usize = 4000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodecError {
    InvalidPacket,
    WrongDuration,
    CodecFailure,
    ClockReversed,
    ClockOverflow,
    BufferFull,
}
impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidPacket => "invalid or oversized Opus packet",
            Self::WrongDuration => "SRS audio requires a 40 ms Opus packet",
            Self::CodecFailure => "Opus operation failed",
            Self::ClockReversed => "playout clock moved backwards",
            Self::ClockOverflow => "playout clock exhausted",
            Self::BufferFull => "encoded frame buffer is full",
        })
    }
}
impl std::error::Error for CodecError {}

pub struct Encoder(opus::Encoder);
impl Encoder {
    pub fn new() -> Result<Self, CodecError> {
        let mut encoder =
            opus::Encoder::new(INPUT_RATE, opus::Channels::Mono, opus::Application::Voip)
                .map_err(|_| CodecError::CodecFailure)?;
        encoder
            .set_inband_fec(false)
            .map_err(|_| CodecError::CodecFailure)?;
        Ok(Self(encoder))
    }
    pub fn encode(
        &mut self,
        pcm: &[i16; INPUT_SAMPLES],
        output: &mut [u8; MAX_OPUS_BYTES],
    ) -> Result<usize, CodecError> {
        self.0
            .encode(pcm, output)
            .map_err(|_| CodecError::CodecFailure)
    }
    pub fn reset(&mut self) -> Result<(), CodecError> {
        self.0.reset_state().map_err(|_| CodecError::CodecFailure)
    }
}

pub struct Decoder(opus::Decoder);
impl Decoder {
    pub fn new() -> Result<Self, CodecError> {
        opus::Decoder::new(OUTPUT_RATE, opus::Channels::Mono)
            .map(Self)
            .map_err(|_| CodecError::CodecFailure)
    }
    pub fn decode(
        &mut self,
        packet: &[u8],
        output: &mut [i16; OUTPUT_SAMPLES],
    ) -> Result<(), CodecError> {
        validate_packet(packet)?;
        let samples = self
            .0
            .decode(packet, output, false)
            .map_err(|_| CodecError::InvalidPacket)?;
        if samples != OUTPUT_SAMPLES {
            return Err(CodecError::WrongDuration);
        }
        Ok(())
    }
    /// Explicit packet-loss concealment; missing network input never implicitly
    /// becomes silence or PLC. The jitter buffer chooses this operation.
    pub fn conceal(&mut self, output: &mut [i16; OUTPUT_SAMPLES]) -> Result<(), CodecError> {
        self.0
            .decode(&[], output, false)
            .map_err(|_| CodecError::CodecFailure)?;
        Ok(())
    }
    pub fn reset(&mut self) -> Result<(), CodecError> {
        self.0.reset_state().map_err(|_| CodecError::CodecFailure)
    }
}

pub fn codec_version() -> &'static str {
    opus::version()
}

/// Cheap framing/duration check before allocating a decoder or queue entry.
/// Decoding can still fail for a corrupt payload.
pub fn validate_packet(packet: &[u8]) -> Result<(), CodecError> {
    if packet.is_empty() || packet.len() > MAX_OPUS_BYTES {
        return Err(CodecError::InvalidPacket);
    }
    if opus::packet::get_nb_samples(packet, OUTPUT_RATE).map_err(|_| CodecError::InvalidPacket)?
        != OUTPUT_SAMPLES
    {
        return Err(CodecError::WrongDuration);
    }
    Ok(())
}
