//! Offline single-voice bridge/codec load. No native devices or sockets.
use srs_audio::{Decoder, Encoder, MAX_OPUS_BYTES, OUTPUT_SAMPLES};
use srs_device::bridge::{BLOCK_SAMPLES, capture_bridge, playback_bridge};
use std::{
    error::Error,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn Error>> {
    let start = Instant::now();
    let (mut capture, mut reader, input_signal) = capture_bridge(1, start)?;
    let (mut writer, mut playback, output_signal) = playback_bridge(2)?;
    let permit = reader.begin(start)?;
    let mut encoder = Encoder::new()?;
    let mut decoder = Decoder::new()?;
    let input: [f32; BLOCK_SAMPLES] =
        std::array::from_fn(|i| 0.25 * (std::f32::consts::TAU * 700.0 * i as f32 / 48000.0).sin());
    let mut bytes = [0; MAX_OPUS_BYTES];
    let mut decoded = [0; OUTPUT_SAMPLES];
    let mut output = [0.0f32; BLOCK_SAMPLES * 2];
    let mut frames = 0;
    let mut peak_frame_age = Duration::ZERO;
    let began = Instant::now();
    for tick in 1..=40_000 {
        let now = start + Duration::from_millis(tick * 10);
        reader.renew(&permit, now)?;
        capture.process(&input, now);
        if let Some(frame) = reader.next_frame(&permit, now)? {
            let len = encoder.encode(&frame.samples, &mut bytes)?;
            reader.validate_frame(&frame, now)?;
            decoder.decode(&bytes[..len], &mut decoded)?;
            writer.enqueue(&decoded, now)?;
            peak_frame_age = peak_frame_age.max(now.duration_since(frame.captured_at));
            frames += 1;
        }
        playback.process(&mut output, now);
    }
    let elapsed = began.elapsed();
    let input_stats = input_signal.statistics();
    let output_stats = output_signal.statistics();
    if frames != 10_000
        || input_stats.dropped != 0
        || output_stats.dropped != 0
        || input_stats.stopped
        || output_stats.stopped
    {
        return Err("unexpected bridge workload result".into());
    }
    println!(
        "{{\"codec_frames\":{frames},\"capture_blocks\":40000,\"dropped\":0,\"startup_underrun_callbacks\":{},\"max_frame_age_ms\":{},\"worker_wall_seconds\":{:.3}}}",
        output_stats.underruns,
        peak_frame_age.as_millis(),
        elapsed.as_secs_f64()
    );
    Ok(())
}
