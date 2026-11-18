//! Desktop session orchestration. Native callbacks remain in srs-device; all
//! codecs, control/network I/O and mixing run here, outside device callbacks.
pub mod held;
mod input;
pub mod observation;
pub use input::{Controller, INPUT_LEASE, InputEvents, controls};
use srs_audio::{Encoder, MAX_OPUS_BYTES, OUTPUT_SAMPLES};
use srs_client::{
    Event, Session,
    receive::Monitor,
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
    Interrupted,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Control(e) => e.fmt(f),
            Self::Voice(e) => e.fmt(f),
            Self::Device(e) => e.fmt(f),
            Self::Codec(e) => e.fmt(f),
            Self::Interrupted => f.write_str("audio worker interrupted; reconnect explicitly"),
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
    /// Mix ticks containing decoded voice per radio, including muted input.
    /// Several speakers on one radio in the same tick increment it only once.
    pub received_frames: [u64; srs_client::RADIO_COUNT],
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
        mut monitor: watch::Receiver<Monitor>,
    ) -> Result<Status, Error> {
        let capture_signal = self.capture.stop_signal();
        let playback_signal = self.playback.stop_signal();
        let mut encoder = Encoder::new()?;
        let mut encoded = [0; MAX_OPUS_BYTES];
        let mut pcm = [[0; OUTPUT_SAMPLES]; 2];
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
        let mut levels = *monitor.borrow_and_update();
        self.voice
            .as_mut()
            .ok_or(VoiceError::NotReady)?
            .set_monitor(levels);
        let mut wall = std::time::SystemTime::now();
        loop {
            let current_wall = std::time::SystemTime::now();
            if !current_wall
                .duration_since(wall)
                .is_ok_and(|gap| gap < INPUT_LEASE)
            {
                return Err(Error::Interrupted);
            }
            wall = current_wall;
            if inputs.has_changed().is_err() {
                return Ok(self.stats);
            }
            capture_signal.check_health(Instant::now().into_std())?;
            playback_signal.check_health(Instant::now().into_std())?;
            let latest = *monitor.borrow_and_update();
            if latest != levels {
                levels = latest;
                self.playback.flush()?;
                self.voice
                    .as_mut()
                    .ok_or(VoiceError::NotReady)?
                    .set_monitor(levels);
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
                let stereo = self.playback.channels() == 2;
                let mix = if stereo {
                    voice.poll_stereo(&mut pcm)?
                } else {
                    voice.poll_mix(&mut pcm[0])?
                };
                if voice.authorization_revision()? == authorization {
                    for (slot, count) in self.stats.received_frames.iter_mut().enumerate() {
                        if mix.received_radios & (1 << slot) != 0 {
                            *count = count.saturating_add(1);
                        }
                    }
                }
                if voice.authorization_revision()? == authorization && mix.audible_speakers > 0 {
                    let output = if stereo {
                        self.playback
                            .enqueue_stereo(&pcm[0], &pcm[1], now.into_std())
                    } else {
                        self.playback.enqueue(&pcm[0], now.into_std())
                    };
                    match output {
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
                _ = monitor.changed() => (),
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
    let began = std::time::SystemTime::now();
    // Recheck wall age before polling a pending socket send after suspension,
    // including platforms whose monotonic clock does not advance during sleep.
    let guarded = std::future::poll_fn(|cx| {
        if !began.elapsed().is_ok_and(|age| age < CAPTURE_MAX_AGE) {
            return std::task::Poll::Ready(Err(VoiceError::SendTimeout));
        }
        std::future::Future::poll(future.as_mut(), cx)
    });
    tokio::pin!(guarded);
    let mut health = interval(WORKER_TICK);
    health.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        signals.0.check_health(Instant::now().into_std())?;
        signals.1.check_health(Instant::now().into_std())?;
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
            sent = &mut guarded => match sent {
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
    radios: Option<watch::Sender<[srs_client::Radio; srs_client::RADIO_COUNT]>>,
    details: Option<watch::Sender<observation::SessionDetails>>,
) -> Result<(), Error> {
    loop {
        let event = session
            .next_event_with_updates(&mut updates)
            .await
            .map_err(Error::Control)?;
        if let Some(radios) = &radios {
            radios.send_if_modified(|current| {
                if current == session.radios() {
                    false
                } else {
                    *current = session.radios().clone();
                    true
                }
            });
        }
        if let Some(details) = &details {
            details.send_if_modified(|old| {
                let new = observation::SessionDetails::read(&session);
                if *old == new {
                    false
                } else {
                    *old = new;
                    true
                }
            });
        }
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
    run_observed(session, capture, playback, inputs, status, None).await
}
/// As `run`, with a bounded latest-value view of authoritative radio state.
/// Includes server-policy changes, not only locally acknowledged edits.
pub async fn run_observed(
    session: Session,
    capture: &mut CaptureReader,
    playback: &mut PlaybackWriter,
    inputs: InputEvents,
    status: watch::Sender<Status>,
    radios: Option<watch::Sender<[srs_client::Radio; srs_client::RADIO_COUNT]>>,
) -> Result<Status, Error> {
    run_with_views(session, capture, playback, inputs, status, radios, None).await
}
/// Adds a bounded, sanitized client/server-policy view for desktop menus.
pub async fn run_with_views(
    session: Session,
    capture: &mut CaptureReader,
    playback: &mut PlaybackWriter,
    inputs: InputEvents,
    status: watch::Sender<Status>,
    radios: Option<watch::Sender<[srs_client::Radio; srs_client::RADIO_COUNT]>>,
    details: Option<watch::Sender<observation::SessionDetails>>,
) -> Result<Status, Error> {
    if let Some(details) = &details {
        details.send_replace(observation::SessionDetails::read(&session));
    }
    if let Some(radios) = &radios {
        radios.send_replace(session.radios().clone());
    }
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
        result = pump(session, sender, inputs.1, radios, details) => { result?; Err(VoiceError::ControlDisconnected.into()) },
        result = worker.drive(inputs.0, receiver, inputs.2) => result,
    }
}
