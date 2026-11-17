//! Fixed-size SPSC exchanges. Data callbacks do format conversion, bounded
//! copying, atomic loads/stores, and queue operations only. No codec, I/O,
//! allocation, logging, mutex, or runtime entry occurs in these callbacks.
use crate::{Error, resample::Decimator};
use cpal::{FromSample, Sample};
use rtrb::{Consumer, Producer, RingBuffer};
use srs_audio::{INPUT_SAMPLES, OUTPUT_SAMPLES};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

pub const BLOCK_SAMPLES: usize = 480;
pub const CAPTURE_BLOCKS: usize = 8;
pub const PLAYBACK_FRAMES: usize = 3;
pub const MAX_CALLBACK_FRAMES: usize = 4800;
pub const CAPTURE_STALL: Duration = Duration::from_millis(250);
pub const CAPTURE_LEASE: Duration = Duration::from_millis(250);
pub const CAPTURE_MAX_AGE: Duration = Duration::from_millis(80);
pub const PLAYBACK_MAX_AGE: Duration = Duration::from_millis(80);

#[derive(Default)]
struct Shared {
    stopped: AtomicBool,
    generation: AtomicU64,
    lease_ms: AtomicU64,
    last_callback_ms: AtomicU64,
    dropped: AtomicU64,
    underruns: AtomicU64,
}
/// Terminal signal, usable by native error callbacks or a platform interruption.
/// Recovery always constructs new streams/bridges; it never resumes capture.
#[derive(Clone)]
pub struct StopSignal(Arc<Shared>);
impl StopSignal {
    pub fn stop(&self) {
        self.0.stopped.store(true, Ordering::Release);
    }
    pub fn is_stopped(&self) -> bool {
        self.0.stopped.load(Ordering::Acquire)
    }
    pub fn statistics(&self) -> Statistics {
        Statistics {
            dropped: self.0.dropped.load(Ordering::Relaxed),
            underruns: self.0.underruns.load(Ordering::Relaxed),
            stopped: self.is_stopped(),
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Statistics {
    pub dropped: u64,
    pub underruns: u64,
    pub stopped: bool,
}
#[derive(Clone)]
pub struct CapturePermit {
    generation: u64,
    owner: Arc<Shared>,
}
impl std::fmt::Debug for CapturePermit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CapturePermit(..)")
    }
}
impl PartialEq for CapturePermit {
    fn eq(&self, other: &Self) -> bool {
        self.generation == other.generation && Arc::ptr_eq(&self.owner, &other.owner)
    }
}
impl Eq for CapturePermit {}
/// Audio is never valid for a later press. Recheck with `validate_frame` after
/// encoding and immediately before handing a captured frame to the network.
pub struct CapturedFrame {
    pub samples: [i16; INPUT_SAMPLES],
    pub captured_at: Instant,
    permit: CapturePermit,
}
#[derive(Clone, Copy)]
struct CaptureBlock {
    samples: [f32; BLOCK_SAMPLES],
    at: Instant,
    generation: u64,
    sequence: u64,
}
pub struct CaptureCallback {
    producer: Producer<CaptureBlock>,
    shared: Arc<Shared>,
    origin: Instant,
    channels: usize,
    partial: [f32; BLOCK_SAMPLES],
    used: usize,
    generation: u64,
    sequence: u64,
    first_at: Instant,
}
pub struct CaptureReader {
    consumer: Consumer<CaptureBlock>,
    shared: Arc<Shared>,
    origin: Instant,
    generation: u64,
    armed: bool,
    began_at_ms: u64,
    decimator: Decimator,
    samples: [i16; INPUT_SAMPLES],
    used: usize,
    expected: Option<u64>,
    first_at: Instant,
}
fn millis(origin: Instant, now: Instant) -> Result<u64, Error> {
    let elapsed = now
        .checked_duration_since(origin)
        .ok_or(Error::ClockReversed)?;
    u64::try_from(elapsed.as_millis()).map_err(|_| Error::GenerationExhausted)
}
fn bump(shared: &Shared) -> Result<u64, Error> {
    let Some(next) = shared.generation.load(Ordering::Acquire).checked_add(1) else {
        shared.stopped.store(true, Ordering::Release);
        return Err(Error::GenerationExhausted);
    };
    shared.generation.store(next, Ordering::Release);
    Ok(next)
}
fn clean(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

pub fn capture_bridge(
    channels: u16,
    now: Instant,
) -> Result<(CaptureCallback, CaptureReader, StopSignal), Error> {
    if !(1..=2).contains(&channels) {
        return Err(Error::UnsupportedConfig);
    }
    let (producer, consumer) = RingBuffer::new(CAPTURE_BLOCKS);
    let shared = Arc::new(Shared::default());
    Ok((
        CaptureCallback {
            producer,
            shared: shared.clone(),
            origin: now,
            channels: channels as usize,
            partial: [0.0; BLOCK_SAMPLES],
            used: 0,
            generation: 0,
            sequence: 0,
            first_at: now,
        },
        CaptureReader {
            consumer,
            shared: shared.clone(),
            origin: now,
            generation: 0,
            armed: false,
            began_at_ms: 0,
            decimator: Decimator::new(),
            samples: [0; INPUT_SAMPLES],
            used: 0,
            expected: None,
            first_at: now,
        },
        StopSignal(shared),
    ))
}
impl CaptureCallback {
    pub fn process<T: Sample>(&mut self, input: &[T], now: Instant)
    where
        f32: FromSample<T>,
    {
        if self.shared.stopped.load(Ordering::Acquire) {
            self.used = 0;
            return;
        }
        if !input.len().is_multiple_of(self.channels)
            || input.len() / self.channels > MAX_CALLBACK_FRAMES
        {
            self.shared.stopped.store(true, Ordering::Release);
            self.used = 0;
            return;
        }
        // Empty notifications do not prove that a stalled device is producing data.
        if input.is_empty() {
            return;
        }
        let Ok(now_ms) = millis(self.origin, now) else {
            self.shared.stopped.store(true, Ordering::Release);
            return;
        };
        self.shared
            .last_callback_ms
            .store(now_ms, Ordering::Release);
        let generation = self.shared.generation.load(Ordering::Acquire);
        let lease = self.shared.lease_ms.load(Ordering::Acquire);
        if lease == 0 || now_ms >= lease {
            self.used = 0;
            return;
        }
        if generation != self.generation {
            self.used = 0;
            self.generation = generation;
        }
        for frame in input.chunks_exact(self.channels) {
            if self.used == 0 {
                self.first_at = now;
            }
            self.partial[self.used] = frame
                .iter()
                .map(|&x| clean(x.to_sample::<f32>()))
                .sum::<f32>()
                / self.channels as f32;
            self.used += 1;
            if self.used == BLOCK_SAMPLES {
                let block = CaptureBlock {
                    samples: self.partial,
                    at: self.first_at,
                    generation,
                    sequence: self.sequence,
                };
                if self.producer.push(block).is_err() {
                    self.shared.dropped.fetch_add(1, Ordering::Relaxed);
                }
                let Some(next) = self.sequence.checked_add(1) else {
                    self.shared.stopped.store(true, Ordering::Release);
                    return;
                };
                self.sequence = next;
                self.used = 0;
            }
        }
    }
}
impl CaptureReader {
    fn reset_partial(&mut self) {
        self.decimator.reset();
        self.used = 0;
        self.expected = None;
    }
    fn clear_queue(&mut self) {
        for _ in 0..CAPTURE_BLOCKS {
            if self.consumer.pop().is_err() {
                break;
            }
        }
        self.reset_partial();
    }
    pub fn stop_signal(&self) -> StopSignal {
        StopSignal(self.shared.clone())
    }
    pub fn begin(&mut self, now: Instant) -> Result<CapturePermit, Error> {
        self.release();
        if self.shared.stopped.load(Ordering::Acquire) {
            return Err(Error::Stopped);
        }
        let lease = millis(self.origin, now)?
            .checked_add(CAPTURE_LEASE.as_millis() as u64)
            .ok_or(Error::GenerationExhausted)?;
        self.generation = bump(&self.shared)?;
        self.armed = true;
        self.began_at_ms = millis(self.origin, now)?;
        self.shared.lease_ms.store(lease, Ordering::Release);
        Ok(CapturePermit {
            generation: self.generation,
            owner: self.shared.clone(),
        })
    }
    pub fn renew(&mut self, permit: &CapturePermit, now: Instant) -> Result<(), Error> {
        self.check(permit, now)?;
        let lease = millis(self.origin, now)?
            .checked_add(CAPTURE_LEASE.as_millis() as u64)
            .ok_or(Error::GenerationExhausted)?;
        self.shared.lease_ms.store(lease, Ordering::Release);
        Ok(())
    }
    pub fn release(&mut self) {
        self.shared.lease_ms.store(0, Ordering::Release);
        self.armed = false;
        self.clear_queue();
    }
    fn check(&mut self, permit: &CapturePermit, now: Instant) -> Result<(), Error> {
        if self.shared.stopped.load(Ordering::Acquire) {
            self.release();
            return Err(Error::Stopped);
        }
        if !self.armed
            || permit.generation != self.generation
            || !Arc::ptr_eq(&permit.owner, &self.shared)
        {
            return Err(Error::StaleFrame);
        }
        let now_ms = match millis(self.origin, now) {
            Ok(now) => now,
            Err(error) => {
                self.shared.stopped.store(true, Ordering::Release);
                self.release();
                return Err(error);
            }
        };
        let last_callback = self
            .shared
            .last_callback_ms
            .load(Ordering::Acquire)
            .max(self.began_at_ms);
        if now_ms.saturating_sub(last_callback) >= CAPTURE_STALL.as_millis() as u64 {
            self.shared.stopped.store(true, Ordering::Release);
            self.release();
            return Err(Error::Stopped);
        }
        if now_ms >= self.shared.lease_ms.load(Ordering::Acquire) {
            self.release();
            return Err(Error::LeaseExpired);
        }
        Ok(())
    }
    pub fn validate_frame(&mut self, frame: &CapturedFrame, now: Instant) -> Result<(), Error> {
        self.check(&frame.permit, now)?;
        if now
            .checked_duration_since(frame.captured_at)
            .is_none_or(|age| age >= CAPTURE_MAX_AGE)
        {
            return Err(Error::StaleFrame);
        }
        Ok(())
    }
    pub fn next_frame(
        &mut self,
        permit: &CapturePermit,
        now: Instant,
    ) -> Result<Option<CapturedFrame>, Error> {
        self.check(permit, now)?;
        // Limit work even if the producer runs continuously during this call.
        for _ in 0..CAPTURE_BLOCKS {
            let Ok(block) = self.consumer.pop() else {
                break;
            };
            if block.generation != self.generation
                // A concurrent producer may timestamp a block after this
                // call sampled its clock. Treat that age as zero for assembly;
                // the public pre-send check uses the caller's fresh clock.
                || now.saturating_duration_since(block.at) >= CAPTURE_MAX_AGE
            {
                self.reset_partial();
                self.shared.dropped.fetch_add(1, Ordering::Relaxed);
                continue;
            }
            if self.expected.is_some_and(|n| n != block.sequence) {
                self.reset_partial();
            }
            self.expected = block.sequence.checked_add(1);
            if self.used == 0 {
                self.first_at = block.at;
            }
            for sample in block.samples {
                if let Some(sample) = self.decimator.sample(sample) {
                    self.samples[self.used] = sample;
                    self.used += 1;
                }
            }
            if self.used == INPUT_SAMPLES {
                self.used = 0;
                let frame = CapturedFrame {
                    samples: self.samples,
                    captured_at: self.first_at,
                    permit: permit.clone(),
                };
                self.check(permit, now)?;
                if now.saturating_duration_since(frame.captured_at) >= CAPTURE_MAX_AGE {
                    return Err(Error::StaleFrame);
                }
                return Ok(Some(frame));
            }
        }
        Ok(None)
    }
}
impl Drop for CaptureReader {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::Release);
    }
}

#[derive(Clone, Copy)]
struct PlaybackFrame {
    samples: [[i16; OUTPUT_SAMPLES]; 2],
    at: Instant,
    generation: u64,
}
pub struct PlaybackWriter {
    channels: u16,
    producer: Producer<PlaybackFrame>,
    shared: Arc<Shared>,
}
pub struct PlaybackCallback {
    consumer: Consumer<PlaybackFrame>,
    shared: Arc<Shared>,
    channels: usize,
    current: Option<PlaybackFrame>,
    position: usize,
}
pub fn playback_bridge(
    channels: u16,
) -> Result<(PlaybackWriter, PlaybackCallback, StopSignal), Error> {
    if !(1..=2).contains(&channels) {
        return Err(Error::UnsupportedConfig);
    }
    let (producer, consumer) = RingBuffer::new(PLAYBACK_FRAMES);
    let shared = Arc::new(Shared::default());
    Ok((
        PlaybackWriter {
            channels,
            producer,
            shared: shared.clone(),
        },
        PlaybackCallback {
            consumer,
            shared: shared.clone(),
            channels: channels as usize,
            current: None,
            position: 0,
        },
        StopSignal(shared),
    ))
}
impl PlaybackWriter {
    pub fn channels(&self) -> u16 {
        self.channels
    }
    pub fn stop_signal(&self) -> StopSignal {
        StopSignal(self.shared.clone())
    }
    pub fn flush(&mut self) -> Result<(), Error> {
        bump(&self.shared).map(|_| ())
    }
    /// All-or-nothing 40 ms frame enqueue; overflow invalidates the backlog.
    pub fn enqueue(&mut self, samples: &[i16; OUTPUT_SAMPLES], now: Instant) -> Result<(), Error> {
        self.enqueue_stereo(samples, samples, now)
    }
    /// Planar stereo queued atomically, with the same stale/flush bounds as mono.
    /// A one-channel callback averages stereo input; enqueue(mono) is unchanged.
    pub fn enqueue_stereo(
        &mut self,
        left: &[i16; OUTPUT_SAMPLES],
        right: &[i16; OUTPUT_SAMPLES],
        now: Instant,
    ) -> Result<(), Error> {
        if self.shared.stopped.load(Ordering::Acquire) {
            return Err(Error::Stopped);
        }
        let frame = PlaybackFrame {
            samples: [*left, *right],
            at: now,
            generation: self.shared.generation.load(Ordering::Acquire),
        };
        if self.producer.push(frame).is_err() {
            self.shared.dropped.fetch_add(1, Ordering::Relaxed);
            self.flush()?;
            return Err(Error::QueueFull);
        }
        Ok(())
    }
}
impl Drop for PlaybackWriter {
    fn drop(&mut self) {
        self.shared.stopped.store(true, Ordering::Release);
    }
}
impl PlaybackCallback {
    pub fn process<T: Sample + FromSample<f32>>(&mut self, output: &mut [T], now: Instant) {
        output.fill(T::EQUILIBRIUM);
        if self.shared.stopped.load(Ordering::Acquire) {
            self.current = None;
            return;
        }
        if !output.len().is_multiple_of(self.channels)
            || output.len() / self.channels > MAX_CALLBACK_FRAMES
        {
            self.shared.stopped.store(true, Ordering::Release);
            self.current = None;
            return;
        }
        let mut pops = 0;
        let mut missing = false;
        for frame in output.chunks_exact_mut(self.channels) {
            loop {
                // Observe concurrent flush/stop before consuming another sample,
                // including a fresh frame enqueued while this callback runs.
                if self.shared.stopped.load(Ordering::Acquire) {
                    self.current = None;
                    return;
                }
                let generation = self.shared.generation.load(Ordering::Acquire);
                if self.current.as_ref().is_some_and(|f| {
                    f.generation != generation
                        // The writer can enqueue after the callback samples
                        // its clock; this is fresh audio, not a clock failure.
                        || now.saturating_duration_since(f.at) >= PLAYBACK_MAX_AGE
                }) {
                    self.current = None;
                    self.shared.dropped.fetch_add(1, Ordering::Relaxed);
                }
                if self.current.is_some()
                    || pops == PLAYBACK_FRAMES + MAX_CALLBACK_FRAMES / OUTPUT_SAMPLES + 1
                {
                    break;
                }
                pops += 1;
                match self.consumer.pop() {
                    Ok(packet) => {
                        self.current = Some(packet);
                        self.position = 0;
                    }
                    Err(_) => break,
                }
            }
            if let Some(current) = &self.current {
                let left = i32::from(current.samples[0][self.position]);
                let right = i32::from(current.samples[1][self.position]);
                if self.channels == 1 {
                    frame[0] = T::from_sample((left + right) as f32 / 65536.0);
                } else {
                    frame[0] = T::from_sample(left as f32 / 32768.0);
                    frame[1] = T::from_sample(right as f32 / 32768.0);
                }
                self.position += 1;
                if self.position == OUTPUT_SAMPLES {
                    self.current = None;
                }
            } else {
                missing = true;
                break; // Remaining output was prefilled with silence.
            }
        }
        if missing {
            self.shared.underruns.fetch_add(1, Ordering::Relaxed);
        }
    }
}
