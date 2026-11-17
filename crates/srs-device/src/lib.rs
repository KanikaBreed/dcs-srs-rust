//! Explicit desktop device ownership. No stream opens until a caller requests it.
//! Codec/resampling work belongs outside callbacks. See docs/DEVICES.md.

pub mod bridge;
pub mod native;
mod resample;

use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    UnsupportedConfig,
    MissingDevice,
    Backend(cpal::ErrorKind),
    Stopped,
    LeaseExpired,
    StaleFrame,
    QueueFull,
    ClockReversed,
    GenerationExhausted,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnsupportedConfig => "device needs 48 kHz mono/stereo f32, i16, or u16 audio",
            Self::MissingDevice => "selected audio device is unavailable",
            Self::Backend(_) => "native audio operation failed",
            Self::Stopped => "audio stream stopped or failed; reopen explicitly",
            Self::LeaseExpired => "capture permit expired; a fresh press is required",
            Self::StaleFrame => "audio frame expired or belongs to an earlier capture permit",
            Self::QueueFull => "audio queue full; old playback invalidated",
            Self::ClockReversed => "audio clock moved backwards",
            Self::GenerationExhausted => "audio generation exhausted; reopen explicitly",
        })
    }
}
impl std::error::Error for Error {}
impl From<cpal::Error> for Error {
    fn from(error: cpal::Error) -> Self {
        Self::Backend(error.kind())
    }
}
