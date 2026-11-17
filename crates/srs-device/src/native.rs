//! CPAL desktop adapter. Uses default host (WASAPI, ALSA, CoreAudio), no automatic
//! device switching/restart. Error or route-change callbacks stop the bridge.
use crate::{
    Error,
    bridge::{self, CaptureReader, PlaybackWriter, StopSignal},
};
use cpal::{
    SampleFormat, Stream, StreamConfig, SupportedStreamConfig, SupportedStreamConfigRange,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub enum Direction {
    Input,
    Output,
}
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
}
#[derive(Clone, Copy, Debug)]
pub struct Format {
    pub channels: u16,
    pub sample_rate: u32,
    pub sample_format: SampleFormat,
}

pub fn devices(direction: Direction) -> Result<Vec<DeviceInfo>, Error> {
    let host = cpal::default_host();
    let devices: Vec<_> = match direction {
        Direction::Input => host.input_devices()?.collect(),
        Direction::Output => host.output_devices()?.collect(),
    };
    devices
        .into_iter()
        .map(|device| {
            Ok(DeviceInfo {
                id: device.id()?.to_string(),
                name: device.description()?.name().to_owned(),
            })
        })
        .collect()
}
fn select(selection: &str, direction: Direction) -> Result<cpal::Device, Error> {
    let host = cpal::default_host();
    if selection == "default" {
        return match direction {
            Direction::Input => host.default_input_device(),
            Direction::Output => host.default_output_device(),
        }
        .ok_or(Error::MissingDevice);
    }
    let id = selection
        .parse::<cpal::DeviceId>()
        .map_err(|_| Error::MissingDevice)?;
    host.device_by_id(&id).ok_or(Error::MissingDevice)
}
// Small pure selection function also tested without opening a native device.
pub fn choose_config(
    ranges: impl Iterator<Item = SupportedStreamConfigRange>,
    direction: Direction,
) -> Result<SupportedStreamConfig, Error> {
    ranges
        .filter(|r| (1..=2).contains(&r.channels()))
        .filter(|r| {
            matches!(
                r.sample_format(),
                SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
            )
        })
        .filter_map(|r| r.try_with_sample_rate(48_000))
        .max_by_key(|r| {
            let preferred_channels = match direction {
                Direction::Input => 1,
                Direction::Output => 2,
            };
            let format = match r.sample_format() {
                SampleFormat::F32 => 4,
                SampleFormat::I16 => 2,
                _ => 0,
            };
            format + u8::from(r.channels() == preferred_channels)
        })
        .ok_or(Error::UnsupportedConfig)
}
fn stream_config(supported: &SupportedStreamConfig) -> StreamConfig {
    let mut config = supported.config();
    // Request 10 ms only if the backend advertises that exact size. Otherwise
    // use its default; callbacks enforce a separate 100 ms upper bound.
    if let cpal::SupportedBufferSize::Range { min, max } = *supported.buffer_size()
        && min <= bridge::BLOCK_SAMPLES as u32
        && max >= bridge::BLOCK_SAMPLES as u32
    {
        config.buffer_size = cpal::BufferSize::Fixed(bridge::BLOCK_SAMPLES as u32);
    }
    config
}
fn format(config: &SupportedStreamConfig) -> Format {
    Format {
        channels: config.channels(),
        sample_rate: config.sample_rate(),
        sample_format: config.sample_format(),
    }
}

pub struct Input {
    stream: Stream,
    reader: CaptureReader,
    signal: StopSignal,
    format: Format,
}
impl Input {
    /// Build a paused stream; `start` explicitly activates the device. Capture
    /// samples are discarded until the caller obtains/renews a capture permit.
    pub fn open(selection: &str) -> Result<Self, Error> {
        let device = select(selection, Direction::Input)?;
        let supported = choose_config(device.supported_input_configs()?, Direction::Input)?;
        let config = stream_config(&supported);
        let (mut callback, reader, signal) =
            bridge::capture_bridge(config.channels, Instant::now())?;
        let error_signal = signal.clone();
        let on_error = move |_error: cpal::Error| error_signal.stop();
        let timeout = Some(Duration::from_secs(3));
        let stream = match supported.sample_format() {
            SampleFormat::F32 => device.build_input_stream(
                config,
                move |data: &[f32], _| callback.process(data, Instant::now()),
                on_error,
                timeout,
            ),
            SampleFormat::I16 => device.build_input_stream(
                config,
                move |data: &[i16], _| callback.process(data, Instant::now()),
                on_error,
                timeout,
            ),
            SampleFormat::U16 => device.build_input_stream(
                config,
                move |data: &[u16], _| callback.process(data, Instant::now()),
                on_error,
                timeout,
            ),
            _ => return Err(Error::UnsupportedConfig),
        }?;
        Ok(Self {
            stream,
            reader,
            signal,
            format: format(&supported),
        })
    }
    pub fn start(&self) -> Result<(), Error> {
        if self.signal.is_stopped() {
            return Err(Error::Stopped);
        }
        self.stream.play().map_err(|error| {
            self.signal.stop();
            error.into()
        })
    }
    pub fn reader(&mut self) -> &mut CaptureReader {
        &mut self.reader
    }
    pub fn signal(&self) -> StopSignal {
        self.signal.clone()
    }
    pub fn format(&self) -> Format {
        self.format
    }
}
impl Drop for Input {
    fn drop(&mut self) {
        self.signal.stop();
    }
}

pub struct Output {
    stream: Stream,
    writer: PlaybackWriter,
    signal: StopSignal,
    format: Format,
}
impl Output {
    pub fn open(selection: &str) -> Result<Self, Error> {
        let device = select(selection, Direction::Output)?;
        let supported = choose_config(device.supported_output_configs()?, Direction::Output)?;
        let config = stream_config(&supported);
        let (writer, mut callback, signal) = bridge::playback_bridge(config.channels)?;
        let error_signal = signal.clone();
        let on_error = move |_error: cpal::Error| error_signal.stop();
        let timeout = Some(Duration::from_secs(3));
        let stream = match supported.sample_format() {
            SampleFormat::F32 => device.build_output_stream(
                config,
                move |data: &mut [f32], _| callback.process(data, Instant::now()),
                on_error,
                timeout,
            ),
            SampleFormat::I16 => device.build_output_stream(
                config,
                move |data: &mut [i16], _| callback.process(data, Instant::now()),
                on_error,
                timeout,
            ),
            SampleFormat::U16 => device.build_output_stream(
                config,
                move |data: &mut [u16], _| callback.process(data, Instant::now()),
                on_error,
                timeout,
            ),
            _ => return Err(Error::UnsupportedConfig),
        }?;
        Ok(Self {
            stream,
            writer,
            signal,
            format: format(&supported),
        })
    }
    pub fn start(&self) -> Result<(), Error> {
        if self.signal.is_stopped() {
            return Err(Error::Stopped);
        }
        self.stream.play().map_err(|error| {
            self.signal.stop();
            error.into()
        })
    }
    pub fn writer(&mut self) -> &mut PlaybackWriter {
        &mut self.writer
    }
    pub fn signal(&self) -> StopSignal {
        self.signal.clone()
    }
    pub fn format(&self) -> Format {
        self.format
    }
}
impl Drop for Output {
    fn drop(&mut self) {
        self.signal.stop();
    }
}
