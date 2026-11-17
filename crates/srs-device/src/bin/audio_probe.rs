//! Explicit bounded-duration device validation. No network, recordings, or
//! device access on startup/help. Each active mode requires device selection.
use srs_audio::{Decoder, Encoder, MAX_OPUS_BYTES, OUTPUT_SAMPLES};
use srs_device::{
    Error as AudioError,
    native::{self, Direction, Input, Output},
};
use std::{
    error::Error,
    io::{self, Write},
    thread,
    time::{Duration, Instant},
};

const HELP: &str = "Usage:
  audio_probe devices
  audio_probe tone --output <device-id|default> --seconds <1..30>
  audio_probe capture --input <device-id|default> --seconds <1..30>
  audio_probe loopback --input <device-id|default> --output <device-id|default> --seconds <1..30>

Tone plays a generated 700 Hz signal. Capture reports level/counts without playback.
Loopback plays your microphone through the SRS codec; use headphones.
No mode records audio or contacts a server. Device IDs are printed only by devices.";
struct Options {
    mode: String,
    input: Option<String>,
    output: Option<String>,
    seconds: u64,
}
fn parse(args: &[String]) -> Result<Options, &'static str> {
    let Some(mode) = args.first() else {
        return Err("missing mode");
    };
    if !matches!(mode.as_str(), "tone" | "capture" | "loopback") {
        return Err("unknown mode");
    }
    let mut input = None;
    let mut output = None;
    let mut seconds = None;
    let (pairs, remainder) = args[1..].as_chunks::<2>();
    for pair in pairs {
        match pair[0].as_str() {
            "--input" if input.is_none() => input = Some(pair[1].clone()),
            "--output" if output.is_none() => output = Some(pair[1].clone()),
            "--seconds" if seconds.is_none() => {
                seconds = Some(pair[1].parse::<u64>().map_err(|_| "invalid duration")?)
            }
            _ => return Err("unknown or duplicate option"),
        }
    }
    if !remainder.is_empty() {
        return Err("option needs a value");
    }
    let seconds = seconds
        .filter(|n| (1..=30).contains(n))
        .ok_or("duration must be 1..30 seconds")?;
    if (mode == "tone" && (output.is_none() || input.is_some()))
        || (mode == "capture" && (input.is_none() || output.is_some()))
        || (mode == "loopback" && (input.is_none() || output.is_none()))
    {
        return Err("select exactly the devices needed by this mode");
    }
    Ok(Options {
        mode: mode.clone(),
        input,
        output,
        seconds,
    })
}
fn announce(message: &str) -> io::Result<()> {
    println!("{message}");
    io::stdout().flush()
}
fn tone(selection: &str, seconds: u64) -> Result<(), Box<dyn Error>> {
    announce("PLAYBACK: generated 700 Hz tone; no microphone is opened.")?;
    let mut output = Output::open(selection)?;
    println!("Output format: {:?}", output.format());
    let start = Instant::now();
    let frames = seconds * 25;
    let total_samples = seconds * 48_000;
    for index in 0..frames {
        let deadline = start + Duration::from_millis(index * 40);
        thread::sleep(deadline.saturating_duration_since(Instant::now()));
        let pcm = std::array::from_fn(|i| {
            let sample = index * OUTPUT_SAMPLES as u64 + i as u64;
            let fade = (sample as f64 / 480.0)
                .min((total_samples - 1 - sample) as f64 / 480.0)
                .min(1.0);
            (3276.0 * fade * (std::f64::consts::TAU * 700.0 * sample as f64 / 48_000.0).sin())
                as i16
        });
        output.writer().enqueue(&pcm, Instant::now())?;
        if index == 0 {
            output.start()?;
        }
    }
    thread::sleep((start + Duration::from_secs(seconds)).saturating_duration_since(Instant::now()));
    let stats = output.signal().statistics();
    println!(
        "Playback frames={frames} dropped={} underrun_callbacks={} stopped={}",
        stats.dropped, stats.underruns, stats.stopped
    );
    if stats.stopped {
        return Err(AudioError::Stopped.into());
    }
    Ok(())
}
fn capture(
    input_selection: &str,
    output_selection: Option<&str>,
    seconds: u64,
) -> Result<(), Box<dyn Error>> {
    announce(if output_selection.is_some() {
        "CAPTURE + PLAYBACK: microphone codec loopback is starting. No recording or network."
    } else {
        "CAPTURE: microphone level/count probe is starting. No playback, recording, or network."
    })?;
    let mut input = Input::open(input_selection)?;
    let mut output = output_selection.map(Output::open).transpose()?;
    let mut encoder = Encoder::new()?;
    let mut decoder = Decoder::new()?;
    let mut encoded = [0; MAX_OPUS_BYTES];
    let mut decoded = [0; OUTPUT_SAMPLES];
    println!("Input format: {:?}", input.format());
    if let Some(output) = &output {
        println!("Output format: {:?}", output.format());
        output.start()?;
    }
    let permit = input.reader().begin(Instant::now())?;
    input.start()?;
    let end = Instant::now() + Duration::from_secs(seconds);
    let mut count = 0;
    let mut energy = 0.0;
    let mut clipped = 0;
    while Instant::now() < end {
        input.reader().renew(&permit, Instant::now())?;
        for _ in 0..2 {
            let Some(frame) = input.reader().next_frame(&permit, Instant::now())? else {
                break;
            };
            let len = encoder.encode(&frame.samples, &mut encoded)?;
            input.reader().validate_frame(&frame, Instant::now())?;
            decoder.decode(&encoded[..len], &mut decoded)?;
            if let Some(output) = &mut output {
                output.writer().enqueue(&decoded, Instant::now())?;
            }
            count += 1;
            for sample in frame.samples {
                energy += (sample as f64 / 32768.0).powi(2);
                clipped += usize::from(sample.unsigned_abs() >= 32760);
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    input.reader().release();
    let stats = input.signal().statistics();
    let rms = if count == 0 {
        0.0
    } else {
        (energy / (count * srs_audio::INPUT_SAMPLES) as f64).sqrt()
    };
    println!(
        "Capture frames={count} rms={rms:.5} clipped_samples={clipped} dropped_blocks={} stopped={}",
        stats.dropped, stats.stopped
    );
    if stats.stopped || count == 0 {
        return Err(AudioError::Stopped.into());
    }
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] || args == ["help"] {
        println!("{HELP}");
        return Ok(());
    }
    if args == ["devices"] {
        for direction in [Direction::Input, Direction::Output] {
            println!("{direction:?} devices:");
            for device in native::devices(direction)? {
                println!("  {:?}  {:?}", device.id, device.name);
            }
        }
        return Ok(());
    }
    let options = parse(&args).map_err(io::Error::other)?;
    match options.mode.as_str() {
        "tone" => tone(
            options.output.as_deref().ok_or("missing output")?,
            options.seconds,
        ),
        _ => capture(
            options.input.as_deref().ok_or("missing input")?,
            options.output.as_deref(),
            options.seconds,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn active_modes_require_explicit_devices_and_finite_duration() {
        let args = |s: &str| s.split_whitespace().map(str::to_owned).collect::<Vec<_>>();
        for command in [
            "tone",
            "capture --seconds 1",
            "tone --output default --seconds 0",
            "loopback --input default --seconds 2",
            "tone --output default --seconds 31",
            "tone --output default --seconds 1 --input default",
            "capture --input default --seconds 1 --seconds 1",
        ] {
            assert!(parse(&args(command)).is_err(), "{command}");
        }
        assert!(parse(&args("tone --output default --seconds 1")).is_ok());
        assert!(
            parse(&args(
                "loopback --input default --output default --seconds 2"
            ))
            .is_ok()
        );
    }
}
