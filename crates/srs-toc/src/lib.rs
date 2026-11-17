//! Desktop session orchestration. Native callbacks remain in srs-device; all
//! codecs, control/network I/O and mixing run here, outside device callbacks.
pub mod held;
mod input;
pub use input::{Controller, INPUT_LEASE, InputEvents, controls};
use srs_audio::{Encoder, MAX_OPUS_BYTES, OUTPUT_SAMPLES};
use srs_client::{
    Event, Session,
    voice::{VoiceConnection, VoiceError, VoiceEvent},
};
use srs_device::bridge::{
    CAPTURE_MAX_AGE, CapturePermit, CaptureReader, PlaybackWriter, StopSignal,
};
use srs_protocol::voice::ClientId;
use std::{fmt, time::Duration};
use tokio::{
    sync::{mpsc, watch},
    time::{Instant, MissedTickBehavior, interval, sleep_until},
};

const WORKER_TICK: Duration = Duration::from_millis(5);
const MIX_PERIOD: Duration = Duration::from_millis(40);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Control(srs_client::Error),
    Voice(VoiceError),
    Device(srs_device::Error),
    Codec(srs_audio::CodecError),
    ControlBacklog,
    InvalidInput,
    InputExpired,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Control(e) => e.fmt(f),
            Self::Voice(e) => e.fmt(f),
            Self::Device(e) => e.fmt(f),
            Self::Codec(e) => e.fmt(f),
            Self::ControlBacklog => f.write_str("TOC control event backlog exceeded"),
            Self::InvalidInput => f.write_str("invalid TOC input action"),
            Self::InputExpired => {
                f.write_str("TOC input ownership expired; release and press again")
            }
        }
    }
}
impl std::error::Error for Error {}
impl From<srs_device::Error> for Error {
    fn from(e: srs_device::Error) -> Self {
        Self::Device(e)
    }
}
impl From<VoiceError> for Error {
    fn from(e: VoiceError) -> Self {
        Self::Voice(e)
    }
}
impl From<srs_audio::CodecError> for Error {
    fn from(e: srs_audio::CodecError) -> Self {
        Self::Codec(e)
    }
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Phase {
    #[default]
    Connecting,
    Receiving,
    Transmitting,
    Stopped,
}
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Status {
    pub phase: Phase,
    pub sent_frames: u64,
    pub mixed_frames: u64,
    pub peak_speakers: usize,
    pub clipped_samples: u64,
    pub rejected_packets: u64,
    pub playback_overflows: u64,
}
pub fn status_channel() -> (watch::Sender<Status>, watch::Receiver<Status>) {
    watch::channel(Status::default())
}

struct Worker<'a> {
    voice: Option<VoiceConnection>,
    capture: &'a mut CaptureReader,
    playback: &'a mut PlaybackWriter,
    permit: Option<CapturePermit>,
    status: watch::Sender<Status>,
    stats: Status,
}
impl Worker<'_> {
    fn release(&mut self) -> Result<(), Error> {
        self.capture.release();
        self.permit = None;
        if let Some(voice) = &mut self.voice {
            voice.release_ptt();
        }
        self.playback.flush()?;
        self.stats.phase = Phase::Receiving;
        self.publish();
        Ok(())
    }
    fn publish(&self) {
        self.status.send_if_modified(|s| {
            if *s == self.stats {
                false
            } else {
                *s = self.stats;
                true
            }
        });
    }
    async fn drive(
        &mut self,
        mut inputs: watch::Receiver<input::Intent>,
        mut departed: mpsc::Receiver<ClientId>,
    ) -> Result<Status, Error> {
        let capture_signal = self.capture.stop_signal();
        let playback_signal = self.playback.stop_signal();
        let mut encoder = Encoder::new()?;
        let mut encoded = [0; MAX_OPUS_BYTES];
        let mut pcm = [0; OUTPUT_SAMPLES];
        // Ignore input already held when registration completes. Start receive-only.
        let mut generation = inputs.borrow_and_update().generation;
        let mut authorization = self
            .voice
            .as_ref()
            .ok_or(VoiceError::NotReady)?
            .authorization_revision()?;
        let mut ticker = interval(WORKER_TICK);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut mix_at = Instant::now();
        self.release()?;
        loop {
            if inputs.has_changed().is_err() {
                return Ok(self.stats);
            }
            if capture_signal.is_stopped() || playback_signal.is_stopped() {
                return Err(srs_device::Error::Stopped.into());
            }
            let revision = self
                .voice
                .as_ref()
                .ok_or(VoiceError::NotReady)?
                .authorization_revision()?;
            if revision != authorization {
                authorization = revision;
                self.release()?;
                // Renewing a held input cannot restart a policy-revoked transmit.
                let intent = *inputs.borrow_and_update();
                // A press following an acknowledged local edit already names
                // this exact revision. Do not swallow that new intentional press.
                if intent.authorization != Some(revision) {
                    generation = intent.generation;
                }
            }
            let intent = *inputs.borrow_and_update();
            let now = Instant::now();
            if intent.generation != generation {
                generation = intent.generation;
                self.release()?;
                if let Some((radio, _)) = intent.held.filter(|(_, until)| *until > now) {
                    self.voice
                        .as_mut()
                        .ok_or(VoiceError::NotReady)?
                        .press_ptt(radio)?;
                    self.permit = Some(self.capture.begin(now.into_std())?);
                    encoder.reset()?;
                    self.stats.phase = Phase::Transmitting;
                    self.publish();
                }
            }
            if self.permit.is_some()
                && (intent.held.is_none_or(|(_, until)| until <= now)
                    || !self
                        .voice
                        .as_ref()
                        .is_some_and(VoiceConnection::is_transmitting))
            {
                self.release()?;
            }
            if let Some(permit) = &self.permit {
                self.capture.renew(permit, now.into_std())?;
                if let Some(frame) = self.capture.next_frame(permit, now.into_std())? {
                    let n = encoder.encode(&frame.samples, &mut encoded)?;
                    self.capture
                        .validate_frame(&frame, Instant::now().into_std())?;
                    let expires = Instant::from_std(frame.captured_at + CAPTURE_MAX_AGE);
                    let sent = send_current(
                        self.voice.as_mut().ok_or(VoiceError::NotReady)?,
                        &mut inputs,
                        (&capture_signal, &playback_signal),
                        generation,
                        expires,
                        &encoded[..n],
                    )
                    .await?;
                    if sent {
                        self.stats.sent_frames += 1;
                    } else {
                        self.release()?;
                    }
                }
            }
            let now = Instant::now();
            if now >= mix_at {
                if now.duration_since(mix_at) >= MIX_PERIOD {
                    self.playback.flush()?;
                }
                mix_at += MIX_PERIOD;
                if mix_at <= now {
                    mix_at = now + MIX_PERIOD;
                }
                let voice = self.voice.as_mut().ok_or(VoiceError::NotReady)?;
                let mix = voice.poll_mix(&mut pcm)?;
                if voice.authorization_revision()? == authorization && mix.speakers > 0 {
                    match self.playback.enqueue(&pcm, now.into_std()) {
                        Ok(()) => {
                            self.stats.mixed_frames += 1;
                        }
                        Err(srs_device::Error::QueueFull) => {
                            self.stats.playback_overflows += 1;
                        }
                        Err(e) => return Err(e.into()),
                    }
                    self.stats.peak_speakers = self.stats.peak_speakers.max(mix.speakers);
                    self.stats.clipped_samples += mix.clipped_samples as u64;
                }
            }
            self.publish();
            let voice = self.voice.as_mut().ok_or(VoiceError::NotReady)?;
            tokio::select! {
                biased;
                _ = inputs.changed() => (),
                peer = departed.recv() => match peer {
                    Some(peer) => { voice.forget_speaker(peer); self.playback.flush()?; }
                    None => return Err(VoiceError::ControlDisconnected.into()),
                },
                _ = ticker.tick() => (),
                event = voice.next_event() => match event? {
                    VoiceEvent::Voice(packet) => {
                        match voice.buffer_voice(&packet) {
                            Ok(_) => (),
                            Err(VoiceError::Codec(_)) => self.stats.rejected_packets += 1,
                            Err(e) => return Err(e.into()),
                        }
                    }
                    VoiceEvent::Rejected => self.stats.rejected_packets += 1,
                    VoiceEvent::Keepalive | VoiceEvent::AuthorizationChanged => (),
                },
            }
        }
    }
}
impl Drop for Worker<'_> {
    fn drop(&mut self) {
        self.capture.release();
        self.capture.stop_signal().stop();
        self.playback.stop_signal().stop();
        if let Some(voice) = &mut self.voice {
            voice.close();
        }
        self.stats.phase = Phase::Stopped;
        self.publish();
    }
}

async fn send_current(
    voice: &mut VoiceConnection,
    inputs: &mut watch::Receiver<input::Intent>,
    signals: (&StopSignal, &StopSignal),
    generation: u64,
    frame_expires: Instant,
    encoded: &[u8],
) -> Result<bool, Error> {
    let future = voice.send_frame(encoded);
    tokio::pin!(future);
    let mut health = interval(WORKER_TICK);
    health.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        if signals.0.is_stopped() || signals.1.is_stopped() {
            return Err(srs_device::Error::Stopped.into());
        }
        if inputs.has_changed().is_err() {
            return Ok(false);
        }
        let intent = *inputs.borrow_and_update();
        let Some((_, until)) = intent.held.filter(|(_, until)| *until > Instant::now()) else {
            return Ok(false);
        };
        if intent.generation != generation || Instant::now() >= frame_expires {
            return Ok(false);
        }
        tokio::select! {
            biased;
            _ = inputs.changed() => (),
            _ = sleep_until(until.min(frame_expires)) => return Ok(false),
            _ = health.tick() => (),
            sent = &mut future => match sent {
                Ok(_) => return Ok(true),
                Err(VoiceError::NotTransmitting | VoiceError::SendTimeout) => return Ok(false),
                Err(e) => return Err(e.into()),
            },
        }
    }
}
async fn pump(
    mut session: Session,
    departed: mpsc::Sender<ClientId>,
    mut updates: srs_client::commands::RadioUpdates,
) -> Result<(), Error> {
    loop {
        let event = session
            .next_event_with_updates(&mut updates)
            .await
            .map_err(Error::Control)?;
        if event == Event::EamDisabled {
            return Err(Error::Control(srs_client::Error::EamDisabled));
        }
        if let Event::ClientDisconnected(peer) = event {
            departed.try_send(peer).map_err(|_| Error::ControlBacklog)?;
        }
    }
}
/// Owns the authenticated, tuned control session until exit. No detached tasks.
/// Input-owner drop exits; all errors and cancellation stop both bridges and PTT.
/// Native stream owners must outlive this future. Reconnect needs new bridges,
/// a new session and a fresh input action. Never reuse a previously held input.
pub async fn run(
    session: Session,
    capture: &mut CaptureReader,
    playback: &mut PlaybackWriter,
    inputs: InputEvents,
    status: watch::Sender<Status>,
) -> Result<Status, Error> {
    let mut worker = Worker {
        voice: None,
        capture,
        playback,
        permit: None,
        status,
        stats: Status::default(),
    };
    worker.capture.release();
    worker.playback.flush()?;
    worker.voice = Some(session.open_voice().await?);
    let (sender, receiver) = mpsc::channel(32);
    tokio::select! {
        biased;
        result = pump(session, sender, inputs.1) => { result?; Err(VoiceError::ControlDisconnected.into()) },
        result = worker.drive(inputs.0, receiver) => result,
    }
}
