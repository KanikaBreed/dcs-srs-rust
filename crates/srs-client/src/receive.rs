//! Standalone receive policy follows desktop UDPClientAudioProcessor.cs and
//! SyncedServerSettings.cs, Ciribob and contributors, GPLv3, upstream d01f540c.
//! The initial mono mixer is new code. No terrain model, effects, or device
//! callbacks. See docs/RECEIVE.md.
use crate::{
    Error, RADIO_COUNT, Session,
    voice::{ReceivedVoice, VoiceConnection, VoiceError},
};
use srs_audio::{
    OUTPUT_SAMPLES,
    jitter::{FrameKind, JitterBuffer, Push},
};
use srs_core::{ReceiveRadio, Reception, blocks_while_transmitting, match_radio};
use srs_protocol::voice::{ClientId, Modulation};
use std::{sync::Arc, time::Duration};
use tokio::time::Instant;

pub const MAX_SPEAKERS: usize = 16;
pub const MAX_GLOBAL_FREQUENCIES: usize = 64;
const PACKET_MAX_AGE: Duration = Duration::from_millis(120);
/// Discrete local headphone routing. Both preserves full gain in each channel.
/// Mono playout ignores routing so all radios retain their existing mono level.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Routing {
    #[default]
    Both,
    Left,
    Right,
}
/// Local receive level only; never serialized onto the SRS control/voice wire.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RadioLevel {
    percent: u8,
    muted: bool,
    routing: Routing,
}
impl RadioLevel {
    pub const FULL: Self = Self {
        percent: 100,
        muted: false,
        routing: Routing::Both,
    };
    pub fn new(percent: u8, muted: bool) -> Option<Self> {
        (percent <= 100).then_some(Self {
            percent,
            muted,
            routing: Routing::Both,
        })
    }
    pub fn percent(self) -> u8 {
        self.percent
    }
    pub fn muted(self) -> bool {
        self.muted
    }
    pub const fn with_routing(mut self, routing: Routing) -> Self {
        self.routing = routing;
        self
    }
    pub const fn routing(self) -> Routing {
        self.routing
    }
    fn gain(self) -> i32 {
        if self.muted {
            0
        } else {
            i32::from(self.percent)
        }
    }
}
pub type Monitor = [RadioLevel; RADIO_COUNT];
pub const DEFAULT_MONITOR: Monitor = [RadioLevel::FULL; RADIO_COUNT];
const EVICT_IDLE_AFTER: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub(crate) struct Policy {
    pub radios: [ReceiveRadio; RADIO_COUNT],
    pub(crate) unit: u32,
    pub(crate) strict: bool,
    pub(crate) irl_tx: bool,
    pub(crate) global: Vec<f64>,
}
impl Session {
    pub(crate) fn receive_policy(&self) -> Result<Arc<Policy>, Error> {
        let mut global = Vec::new();
        if let Some(list) = self.settings.get("GLOBAL_LOBBY_FREQUENCIES") {
            for value in list.split(',') {
                if let Ok(mhz) = value.trim().parse::<f64>() {
                    let hz = mhz * 1e6;
                    if !hz.is_finite() || hz < 0.0 {
                        continue;
                    }
                    if global.len() == MAX_GLOBAL_FREQUENCIES {
                        return Err(Error::ResourceLimit);
                    }
                    global.push(hz);
                }
            }
        }
        Ok(Arc::new(Policy {
            radios: std::array::from_fn(|slot| {
                let radio = &self.radios[slot];
                ReceiveRadio {
                    frequency_hz: radio.frequency_hz,
                    guard_hz: radio.guard_hz,
                    modulation: radio.modulation,
                    encryption_key: radio.encryption_key,
                    intercom_unit_id: radio.intercom_unit_id,
                }
            }),
            unit: self.identity.unit_id,
            strict: self.setting_bool("STRICT_RADIO_ENCRYPTION", false)?,
            irl_tx: self.setting_bool("IRL_RADIO_TX", false)?,
            global,
        }))
    }
}
impl Policy {
    fn route(&self, packet: &ReceivedVoice, blocked: u16) -> Option<Reception> {
        // TOC keeps the existing receive order independently of local levels. Use the
        // first clear eligible frequency, one copy per original speaker/frame.
        for incoming in &packet.frequencies {
            let global = self.global.contains(&incoming.hz); // desktop uses exact equality
            let mut incoming = *incoming;
            if global {
                incoming.encryption = 0;
            }
            if let Some(route) = match_radio(
                &self.radios,
                self.unit,
                incoming,
                packet.unit_id,
                self.strict,
                blocked,
            ) {
                let modulation = self.radios[route.radio].modulation;
                if route.decryptable
                    && (blocked & (1 << route.radio) == 0
                        || modulation == Modulation::INTERCOM
                        || modulation == Modulation::MIDS
                        || global)
                {
                    return Some(route);
                }
            }
        }
        None
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueOutcome {
    Queued,
    Resynchronized,
    Duplicate,
    Late,
    Ineligible,
    Stale,
    Capacity,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Playout {
    pub speaker: ClientId,
    pub radio: usize,
    pub secondary: bool,
    pub packet_number: u64,
    pub kind: FrameKind,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Mix {
    pub speakers: usize,
    pub clipped_samples: usize,
    pub audible_speakers: usize,
    /// Radios with decoded network audio this tick, including muted radios; no PLC.
    pub received_radios: u16,
}
struct Stream {
    speaker: ClientId,
    last_received: Instant,
    buffer: JitterBuffer<Reception>,
}
pub(crate) struct ReceiveState {
    origin: Instant,
    stamp: Option<(u64, u16)>,
    streams: Vec<Stream>,
    monitor: Monitor,
}
impl Default for ReceiveState {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
            stamp: None,
            streams: Vec::new(),
            monitor: DEFAULT_MONITOR,
        }
    }
}
impl ReceiveState {
    pub(crate) fn clear(&mut self) {
        self.streams.clear();
        self.stamp = None;
    }
}
impl VoiceConnection {
    /// Change local receive gains without retuning, changing PTT or resetting
    /// decoders. The caller must flush any downstream output queue at this boundary.
    pub fn set_monitor(&mut self, monitor: Monitor) {
        self.receive.monitor = monitor;
    }

    /// Worker-only 40 ms mono mix: at most one due frame per original speaker.
    /// Per-radio 0–100% gain, integer sum and final saturation; no speaker backlog.
    /// Call on a 40 ms cadence. Each speaker retains its own decoder and PLC.
    /// Mono output preserves the level of every radio, ignoring headphone routing.
    pub fn poll_mix(&mut self, pcm: &mut [i16; OUTPUT_SAMPLES]) -> Result<Mix, VoiceError> {
        self.mix_channels(std::array::from_mut(pcm))
    }
    /// Planar stereo output. Both routes at full gain to each ear; Left/Right
    /// route only to that ear. This is discrete routing, not upstream's continuous
    /// half-gain-center balance law. Decoding and RX accounting happen once.
    pub fn poll_stereo(&mut self, pcm: &mut [[i16; OUTPUT_SAMPLES]; 2]) -> Result<Mix, VoiceError> {
        self.mix_channels(pcm)
    }
    fn mix_channels<const CHANNELS: usize>(
        &mut self,
        pcm: &mut [[i16; OUTPUT_SAMPLES]; CHANNELS],
    ) -> Result<Mix, VoiceError> {
        for channel in pcm.iter_mut() {
            channel.fill(0);
        }
        self.sync_receive()?;
        let now = Instant::now().duration_since(self.receive.origin);
        let mut sum = [[0i32; OUTPUT_SAMPLES]; CHANNELS];
        let mut decoded = [0; OUTPUT_SAMPLES];
        let mut mix = Mix::default();
        for stream in &mut self.receive.streams {
            if let Some(frame) = stream
                .buffer
                .poll(now, &mut decoded)
                .map_err(VoiceError::Codec)?
            {
                mix.speakers += 1;
                let level = self.receive.monitor[frame.metadata.radio];
                let gain = level.gain();
                mix.audible_speakers += usize::from(gain > 0);
                if frame.kind == FrameKind::Decoded {
                    mix.received_radios |= 1 << frame.metadata.radio;
                }
                for (channel, sum) in sum.iter_mut().enumerate() {
                    if CHANNELS == 1
                        || level.routing == Routing::Both
                        || (channel == 0 && level.routing == Routing::Left)
                        || (channel == 1 && level.routing == Routing::Right)
                    {
                        for (total, sample) in sum.iter_mut().zip(decoded) {
                            *total += i32::from(sample) * gain;
                        }
                    }
                }
            }
        }
        for (output, sum) in pcm.iter_mut().zip(sum) {
            for (output, total) in output.iter_mut().zip(sum) {
                let total = total / 100;
                let limited = total.clamp(i16::MIN as i32, i16::MAX as i32);
                mix.clipped_samples += usize::from(limited != total);
                *output = limited as i16;
            }
        }
        Ok(mix)
    }

    /// Forward `Event::ClientDisconnected(id)` here. SRS restarts its
    /// sequence on reconnect, sometimes retaining identity. Drop that speaker's
    /// decoder/high-water mark without interrupting local PTT or other speakers.
    pub fn forget_speaker(&mut self, speaker: ClientId) {
        self.receive
            .streams
            .retain(|stream| stream.speaker != speaker);
    }

    fn sync_receive(&mut self) -> Result<Option<Arc<Policy>>, VoiceError> {
        if let Err(error) = self.check() {
            self.receive.clear();
            return Err(error);
        }
        let auth = self.authorization.borrow();
        let policy = auth.receive.clone();
        let mut blocked = 0;
        if let Some(policy) = &policy
            && policy.irl_tx
            && let Some((slot, revision)) = self.ptt
            && revision == auth.revision
            && blocks_while_transmitting(policy.radios[slot].modulation)
        {
            blocked = 1 << slot;
        }
        let stamp = (auth.revision, blocked);
        if self.receive.stamp != Some(stamp) {
            self.receive.clear();
            self.receive.stamp = Some(stamp);
        }
        Ok(policy)
    }
    /// Worker-only. Accept only a fresh datagram from this connection/revision;
    /// deduplicate relays by original identity and sequence before Opus decoding.
    pub fn buffer_voice(&mut self, packet: &ReceivedVoice) -> Result<QueueOutcome, VoiceError> {
        let Some(policy) = self.sync_receive()? else {
            return Ok(QueueOutcome::Ineligible);
        };
        let now = Instant::now();
        let Some((revision, blocked)) = self.receive.stamp else {
            return Ok(QueueOutcome::Ineligible);
        };
        if packet.generation != revision
            || packet.receiver != self.id
            || packet.received_at > now
            || now.duration_since(packet.received_at) >= PACKET_MAX_AGE
        {
            return Ok(QueueOutcome::Stale);
        }
        let Some(route) = policy.route(packet, blocked) else {
            return Ok(QueueOutcome::Ineligible);
        };
        srs_audio::validate_packet(&packet.audio).map_err(VoiceError::Codec)?;
        let position = self
            .receive
            .streams
            .iter()
            .position(|s| s.speaker == packet.original_client_id);
        let index = if let Some(index) = position {
            index
        } else {
            if self.receive.streams.len() == MAX_SPEAKERS {
                if let Some(index) = self
                    .receive
                    .streams
                    .iter()
                    .position(|s| now.duration_since(s.last_received) >= EVICT_IDLE_AFTER)
                {
                    self.receive.streams.swap_remove(index);
                } else {
                    return Ok(QueueOutcome::Capacity);
                }
            }
            self.receive.streams.push(Stream {
                speaker: packet.original_client_id,
                last_received: now,
                buffer: JitterBuffer::new().map_err(VoiceError::Codec)?,
            });
            self.receive.streams.len() - 1
        };
        let stream = &mut self.receive.streams[index];
        let outcome = stream
            .buffer
            .push(
                packet.packet_number,
                &packet.audio,
                route,
                now.duration_since(self.receive.origin),
            )
            .map_err(VoiceError::Codec)?;
        if matches!(outcome, Push::Queued | Push::Resynchronized) {
            stream.last_received = now;
        }
        Ok(match outcome {
            Push::Queued => QueueOutcome::Queued,
            Push::Resynchronized => QueueOutcome::Resynchronized,
            Push::Duplicate => QueueOutcome::Duplicate,
            Push::Late => QueueOutcome::Late,
        })
    }
    /// Poll at this deadline alongside `next_event`; both APIs are caller-driven.
    pub fn audio_deadline(&mut self) -> Result<Option<Instant>, VoiceError> {
        self.sync_receive()?;
        Ok(self
            .receive
            .streams
            .iter()
            .filter_map(|s| s.buffer.deadline())
            .min()
            .and_then(|d| self.receive.origin.checked_add(d)))
    }
    /// One 48 kHz mono frame into caller storage. No frame/error leaves zeroes.
    /// Rechecks session, tuning, settings and PTT before returning queued audio.
    /// The caller must bound/flush any downstream device queue on the same events.
    pub fn poll_audio(
        &mut self,
        pcm: &mut [i16; OUTPUT_SAMPLES],
    ) -> Result<Option<Playout>, VoiceError> {
        pcm.fill(0);
        self.sync_receive()?;
        let now = Instant::now().duration_since(self.receive.origin);
        let next = self
            .receive
            .streams
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.buffer.deadline().map(|d| (i, d)))
            .filter(|(_, d)| *d <= now)
            .min_by_key(|(_, d)| *d);
        let Some((index, _)) = next else {
            return Ok(None);
        };
        let stream = &mut self.receive.streams[index];
        Ok(stream
            .buffer
            .poll(now, pcm)
            .map_err(VoiceError::Codec)?
            .map(|frame| {
                let gain = self.receive.monitor[frame.metadata.radio].gain();
                for sample in pcm {
                    *sample = (i32::from(*sample) * gain / 100) as i16;
                }
                Playout {
                    speaker: stream.speaker,
                    radio: frame.metadata.radio,
                    secondary: frame.metadata.secondary,
                    packet_number: frame.sequence,
                    kind: frame.kind,
                }
            }))
    }
}
