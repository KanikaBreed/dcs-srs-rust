//! Explicit offline diagnostics. Owns native streams on one worker; never records
//! samples or opens a network connection. The caller must renew its UI lease.
use crate::{
    Error,
    native::{Input, Output},
};
use srs_audio::{Decoder, Encoder, MAX_OPUS_BYTES, OUTPUT_SAMPLES};
use std::{
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant, SystemTime},
};

pub const LIMIT: Duration = Duration::from_secs(15);
pub const LEASE: Duration = Duration::from_millis(250);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Capture,
    Loopback,
    Stereo,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    #[default]
    Opening,
    Running,
    Complete,
    Cancelled,
    Interrupted,
    Failed,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Snapshot {
    pub state: State,
    pub elapsed: u32,
    pub peak_percent: u32,
    pub clipped: u64,
    pub frames: u64,
    /// 0: capture/loopback, 1: left, 2: right.
    pub channel: u8,
    pub mono: bool,
}
struct Control {
    at: Instant,
    wall: SystemTime,
    cancelled: bool,
}
pub struct Probe {
    control: Arc<Mutex<Control>>,
    view: Arc<Mutex<Snapshot>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Probe {
    pub fn start(mode: Mode, input: String, output: String) -> std::io::Result<Self> {
        let control = Arc::new(Mutex::new(Control {
            at: Instant::now(),
            wall: SystemTime::now(),
            cancelled: false,
        }));
        let requests = control.clone();
        let view = Arc::new(Mutex::new(Snapshot::default()));
        let snapshot = view.clone();
        let thread = thread::Builder::new()
            .name("srs-audio-test".into())
            .spawn(move || {
                let outcome = exercise(mode, &input, &output, requests, &snapshot);
                if let Ok(mut value) = snapshot.lock() {
                    value.state = outcome.unwrap_or(State::Failed);
                    value.peak_percent = 0;
                }
            })?;
        Ok(Self {
            control,
            view,
            thread: Some(thread),
        })
    }
    pub fn renew(&self) {
        if let Ok(mut control) = self.control.lock() {
            control.at = Instant::now();
            control.wall = SystemTime::now();
        }
    }
    pub fn snapshot(&self) -> Snapshot {
        self.view.lock().map(|s| *s).unwrap_or(Snapshot {
            state: State::Failed,
            ..Snapshot::default()
        })
    }
    pub fn finished(&self) -> bool {
        self.thread.as_ref().is_none_or(|t| t.is_finished())
    }
    pub fn cancel(&mut self) {
        if let Ok(mut control) = self.control.lock() {
            control.cancelled = true;
        }
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        self.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn exercise(
    mode: Mode,
    input: &str,
    output: &str,
    requests: Arc<Mutex<Control>>,
    view: &Mutex<Snapshot>,
) -> Result<State, Error> {
    let mut input = if mode == Mode::Stereo {
        None
    } else {
        Some(Input::open(input)?)
    };
    let mut output = if mode == Mode::Capture {
        None
    } else {
        Some(Output::open(output)?)
    };
    let mut encoder = Encoder::new().map_err(|_| Error::Stopped)?;
    let mut decoder = Decoder::new().map_err(|_| Error::Stopped)?;
    let mut encoded = [0; MAX_OPUS_BYTES];
    let mut decoded = [0; OUTPUT_SAMPLES];
    let silence = [0; OUTPUT_SAMPLES];
    let began = Instant::now();
    let mut permit = None;
    let mut started = false;
    let mut last = (Instant::now(), SystemTime::now());
    let mut next_tone = began;
    let mut frame = 0;
    let mut current = Snapshot {
        mono: output.as_ref().is_some_and(|o| o.format().channels == 1),
        ..Snapshot::default()
    };
    loop {
        let now = Instant::now();
        let wall = SystemTime::now();
        if now.duration_since(last.0) >= LEASE
            || !wall.duration_since(last.1).is_ok_and(|gap| gap < LEASE)
        {
            return Ok(State::Interrupted);
        }
        last = (now, wall);
        {
            let control = requests.lock().map_err(|_| Error::Stopped)?;
            if control.cancelled {
                return Ok(State::Cancelled);
            }
            if now.saturating_duration_since(control.at) >= LEASE
                || !SystemTime::now()
                    .duration_since(control.wall)
                    .is_ok_and(|age| age < LEASE)
            {
                return Ok(State::Interrupted);
            }
        }
        if now.duration_since(began) >= LIMIT {
            return Ok(if current.frames > 0 {
                State::Complete
            } else {
                State::Failed
            });
        }
        if !started {
            if let Some(input) = &mut input {
                permit = Some(input.reader().begin(now)?);
                input.start()?;
            }
            if let Some(output) = &output {
                output.start()?;
            }
            started = true;
            current.state = State::Running;
        }
        if let Some(output) = &output
            && output.signal().is_stopped()
        {
            return Err(Error::Stopped);
        }
        current.elapsed = now.duration_since(began).as_secs() as u32;
        if let (Some(input), Some(permit)) = (&mut input, &permit) {
            input.reader().renew(permit, now)?;
            for _ in 0..2 {
                let Some(captured) = input.reader().next_frame(permit, now)? else {
                    break;
                };
                current.peak_percent = captured
                    .samples
                    .iter()
                    .map(|s| u32::from(s.unsigned_abs()) * 100 / 32768)
                    .max()
                    .unwrap_or(0);
                current.clipped += captured
                    .samples
                    .iter()
                    .filter(|s| s.unsigned_abs() >= 32760)
                    .count() as u64;
                current.frames += 1;
                if let Some(output) = &mut output {
                    let count = encoder
                        .encode(&captured.samples, &mut encoded)
                        .map_err(|_| Error::Stopped)?;
                    input.reader().validate_frame(&captured, Instant::now())?;
                    decoder
                        .decode(&encoded[..count], &mut decoded)
                        .map_err(|_| Error::Stopped)?;
                    output.writer().enqueue(&decoded, Instant::now())?;
                }
            }
        } else if now >= next_tone {
            let (tone, channel) = stereo_frame(frame);
            current.channel = channel;
            if let Some(output) = &mut output {
                let (left, right) = if channel == 1 {
                    (&tone, &silence)
                } else {
                    (&silence, &tone)
                };
                output.writer().enqueue_stereo(left, right, now)?;
            }
            frame += 1;
            current.frames += 1;
            next_tone = now + Duration::from_millis(40);
        }
        if let Ok(mut snapshot) = view.lock() {
            *snapshot = current;
        }
        thread::sleep(Duration::from_millis(5));
    }
}
fn stereo_frame(frame: u64) -> ([i16; OUTPUT_SAMPLES], u8) {
    let position = frame % 50; // two seconds per ear, with a 10 ms fade at each edge
    let tone = std::array::from_fn(|i| {
        let sample = position * OUTPUT_SAMPLES as u64 + i as u64;
        let fade = (sample as f64 / 480.0)
            .min((96_000 - 1 - sample) as f64 / 480.0)
            .min(1.0);
        (3276.0 * fade * (std::f64::consts::TAU * 700.0 * sample as f64 / 48_000.0).sin()) as i16
    });
    (tone, if (frame / 50).is_multiple_of(2) { 1 } else { 2 })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires test_null_audio.py complete private ALSA null configuration"]
    fn private_null_diagnostics_cancel_stall_and_deadline() {
        assert_eq!(
            std::fs::read_to_string(std::env::var("ALSA_CONFIG_PATH").unwrap()).unwrap(),
            "pcm.!default { type null }\n"
        );
        for mode in [Mode::Capture, Mode::Loopback, Mode::Stereo] {
            let mut probe = Probe::start(mode, "default".into(), "default".into()).unwrap();
            let began = Instant::now();
            while probe.snapshot().state == State::Opening && !probe.finished() {
                assert!(began.elapsed() < Duration::from_secs(5));
                probe.renew();
                thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(probe.snapshot().state, State::Running);
            // ALSA null callbacks are unpaced and can continuously overflow the
            // capture bridge. This test checks stream ownership/cancellation;
            // paced generated bridges separately verify codec/frame delivery.
            for _ in 0..5 {
                probe.renew();
                thread::sleep(Duration::from_millis(20));
            }
            assert_eq!(probe.snapshot().state, State::Running);
            probe.cancel();
            while !probe.finished() {
                assert!(began.elapsed() < Duration::from_secs(5));
                thread::sleep(Duration::from_millis(5));
            }
            assert_eq!(probe.snapshot().state, State::Cancelled);
        }
        let probe = Probe::start(Mode::Stereo, "default".into(), "default".into()).unwrap();
        thread::sleep(Duration::from_millis(600));
        assert!(probe.finished());
        assert_eq!(probe.snapshot().state, State::Interrupted);
        probe.renew();
        assert_eq!(
            probe.snapshot().state,
            State::Interrupted,
            "renewal cannot reopen audio"
        );
        let probe = Probe::start(Mode::Stereo, "default".into(), "default".into()).unwrap();
        let began = Instant::now();
        while !probe.finished() {
            assert!(began.elapsed() < LIMIT + Duration::from_secs(3));
            probe.renew();
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(probe.snapshot().state, State::Complete);
        assert!(began.elapsed() >= LIMIT);
    }
    #[test]
    fn stereo_cues_alternate_and_fade_without_clipping() {
        for (frame, channel) in [(0, 1), (49, 1), (50, 2), (99, 2), (100, 1)] {
            let (tone, ear) = stereo_frame(frame);
            assert_eq!(ear, channel);
            assert!(tone.iter().all(|s| s.unsigned_abs() <= 3276));
            assert!(tone.iter().any(|s| s.unsigned_abs() > 3000));
            if frame % 50 == 0 {
                assert_eq!(tone[0], 0);
            }
            if frame % 50 == 49 {
                assert_eq!(tone[OUTPUT_SAMPLES - 1], 0);
            }
        }
    }
}
