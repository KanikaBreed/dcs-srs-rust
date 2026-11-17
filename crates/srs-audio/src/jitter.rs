//! Bounded, worker-thread playout of one original speaker's encoded packets.
//! This is a new fixed-delay scheduler, not a port of upstream's Speex buffer.
//! All times are monotonic elapsed durations supplied by the owner for simulation.

use crate::{CodecError, Decoder, OUTPUT_SAMPLES, validate_packet};
use std::{collections::BTreeMap, time::Duration};

pub const START_DELAY: Duration = Duration::from_millis(80);
pub const FRAME_DURATION: Duration = Duration::from_millis(40);
pub const STALE_AFTER: Duration = Duration::from_millis(120);
pub const IDLE_AFTER: Duration = Duration::from_millis(400);
pub const MAX_PENDING: usize = 8;
pub const MAX_CONCEALMENT: u8 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Push {
    Queued,
    Duplicate,
    Late,
    Resynchronized,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameKind {
    Decoded,
    Concealed,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Frame<T> {
    pub sequence: u64,
    pub kind: FrameKind,
    pub metadata: T,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Statistics {
    pub decoded: u64,
    pub concealed: u64,
    pub duplicates: u64,
    pub late: u64,
    pub resets: u64,
    pub codec_errors: u64,
}
struct Packet<T> {
    bytes: Vec<u8>,
    metadata: T,
}

pub struct JitterBuffer<T: Copy> {
    decoder: Decoder,
    packets: BTreeMap<u64, Packet<T>>,
    next: Option<u64>,
    deadline: Option<Duration>,
    highest: Option<u64>,
    played: Option<u64>,
    last_arrival: Option<Duration>,
    clock: Duration,
    started: bool,
    missing: u8,
    metadata: Option<T>,
    statistics: Statistics,
}
impl<T: Copy> JitterBuffer<T> {
    pub fn new() -> Result<Self, CodecError> {
        Ok(Self {
            decoder: Decoder::new()?,
            packets: BTreeMap::new(),
            next: None,
            deadline: None,
            highest: None,
            played: None,
            last_arrival: None,
            clock: Duration::ZERO,
            started: false,
            missing: 0,
            metadata: None,
            statistics: Statistics::default(),
        })
    }
    pub fn deadline(&self) -> Option<Duration> {
        self.deadline
    }
    pub fn pending(&self) -> usize {
        self.packets.len()
    }
    pub fn statistics(&self) -> Statistics {
        self.statistics
    }
    fn reset(&mut self) -> Result<(), CodecError> {
        self.packets.clear();
        self.next = None;
        self.deadline = None;
        self.started = false;
        self.missing = 0;
        self.metadata = None;
        // Retain the high-water mark so late relays cannot restart old audio.
        // PLC can predict sequence numbers the sender never transmitted. At a
        // burst boundary retain only observed packets, not those predictions.
        self.played = self.highest;
        self.statistics.resets = self.statistics.resets.saturating_add(1);
        self.decoder.reset()
    }
    fn tick(&mut self, now: Duration) -> Result<(), CodecError> {
        if now < self.clock {
            return Err(CodecError::ClockReversed);
        }
        self.clock = now;
        if self
            .deadline
            .is_some_and(|d| now.saturating_sub(d) >= STALE_AFTER)
        {
            self.reset()?;
        }
        Ok(())
    }
    pub fn push(
        &mut self,
        sequence: u64,
        bytes: &[u8],
        metadata: T,
        now: Duration,
    ) -> Result<Push, CodecError> {
        validate_packet(bytes)?;
        self.tick(now)?;
        let mut reset = false;
        if self
            .last_arrival
            .is_some_and(|t| now.saturating_sub(t) >= IDLE_AFTER)
        {
            self.reset()?;
            reset = true;
        }
        if self.packets.contains_key(&sequence) {
            self.statistics.duplicates = self.statistics.duplicates.saturating_add(1);
            return Ok(Push::Duplicate);
        }
        if self.played.is_some_and(|n| sequence <= n) {
            self.statistics.late = self.statistics.late.saturating_add(1);
            return Ok(Push::Late);
        }
        if self
            .next
            .is_some_and(|n| sequence > n && sequence - n >= MAX_PENDING as u64)
        {
            self.reset()?;
            reset = true;
            if self.played.is_some_and(|n| sequence <= n) {
                self.statistics.late = self.statistics.late.saturating_add(1);
                return Ok(Push::Late);
            }
        }
        if let Some(next) = self.next
            && sequence < next
        {
            if self.started
                || next - sequence >= MAX_PENDING as u64
                || self
                    .highest
                    .is_some_and(|n| n - sequence >= MAX_PENDING as u64)
            {
                self.statistics.late = self.statistics.late.saturating_add(1);
                return Ok(Push::Late);
            }
            self.next = Some(sequence);
        }
        if self.packets.len() >= MAX_PENDING {
            return Err(CodecError::BufferFull);
        }
        if self.deadline.is_none() {
            self.next = Some(sequence);
            self.deadline = now.checked_add(START_DELAY);
            if self.deadline.is_none() {
                return Err(CodecError::ClockOverflow);
            }
            self.started = false;
            self.missing = 0;
        }
        self.highest = Some(self.highest.map_or(sequence, |n| n.max(sequence)));
        self.last_arrival = Some(now);
        self.packets.insert(
            sequence,
            Packet {
                bytes: bytes.to_vec(),
                metadata,
            },
        );
        Ok(if reset {
            Push::Resynchronized
        } else {
            Push::Queued
        })
    }
    /// At most one frame. No catch-up burst after a worker stall. Output is zeroed
    /// on no frame/error. PLC is limited to two frames, including the burst tail.
    pub fn poll(
        &mut self,
        now: Duration,
        output: &mut [i16; OUTPUT_SAMPLES],
    ) -> Result<Option<Frame<T>>, CodecError> {
        output.fill(0);
        self.tick(now)?;
        let (Some(deadline), Some(sequence)) = (self.deadline, self.next) else {
            return Ok(None);
        };
        if now < deadline {
            return Ok(None);
        }
        let (metadata, kind, result) = if let Some(packet) = self.packets.remove(&sequence) {
            self.missing = 0;
            self.metadata = Some(packet.metadata);
            (
                packet.metadata,
                FrameKind::Decoded,
                self.decoder.decode(&packet.bytes, output),
            )
        } else if self.missing < MAX_CONCEALMENT && self.metadata.is_some() {
            self.missing += 1;
            // Metadata exists in this branch, but keep the external path fallible.
            let Some(metadata) = self.metadata else {
                return Ok(None);
            };
            (metadata, FrameKind::Concealed, self.decoder.conceal(output))
        } else {
            // A long hole starts a fresh decoder at the earliest queued frame.
            self.decoder.reset()?;
            self.metadata = None;
            self.missing = 0;
            self.started = false;
            if let Some((&next, _)) = self.packets.first_key_value() {
                self.next = Some(next);
                self.deadline = now.checked_add(START_DELAY);
            } else {
                self.next = None;
                self.deadline = None;
                self.played = self.highest;
            }
            return Ok(None);
        };
        if let Err(error) = result {
            output.fill(0);
            self.statistics.codec_errors = self.statistics.codec_errors.saturating_add(1);
            self.reset()?;
            return Err(error);
        }
        self.started = true;
        self.played = Some(sequence);
        self.next = sequence.checked_add(1);
        // Keep cadence through ordinary timer overshoot without accumulating
        // latency. If an entire frame was missed, rebase instead of bursting.
        self.deadline = self.next.and_then(|_| {
            deadline.checked_add(FRAME_DURATION).and_then(|next| {
                if next <= now {
                    now.checked_add(FRAME_DURATION)
                } else {
                    Some(next)
                }
            })
        });
        match kind {
            FrameKind::Decoded => {
                self.statistics.decoded = self.statistics.decoded.saturating_add(1)
            }
            FrameKind::Concealed => {
                self.statistics.concealed = self.statistics.concealed.saturating_add(1)
            }
        }
        Ok(Some(Frame {
            sequence,
            kind,
            metadata,
        }))
    }
}
