//! Offline worker-load measurement: synthetic time and audio, no devices/sockets.
use srs_audio::{Encoder, INPUT_SAMPLES, MAX_OPUS_BYTES, OUTPUT_SAMPLES, jitter::JitterBuffer};
use std::{
    error::Error,
    time::{Duration, Instant},
};

fn main() -> Result<(), Box<dyn Error>> {
    const SPEAKERS: usize = 16;
    const FRAMES: u64 = 10_000;
    let mut encoder = Encoder::new()?;
    let pcm: [i16; INPUT_SAMPLES] = std::array::from_fn(|i| {
        (12000.0 * (std::f64::consts::TAU * 700.0 * i as f64 / 16000.0).sin()) as i16
    });
    let mut encoded = [0; MAX_OPUS_BYTES];
    let len = encoder.encode(&pcm, &mut encoded)?;
    let mut streams = (0..SPEAKERS)
        .map(|_| JitterBuffer::new())
        .collect::<Result<Vec<_>, _>>()?;
    let mut output = [0; OUTPUT_SAMPLES];
    let mut peak_packets = 0;
    let before = Instant::now();
    for tick in 0..FRAMES + 2 {
        let now = Duration::from_millis(tick * 40);
        for stream in &mut streams {
            if tick < FRAMES && tick % 10 != 5 {
                stream.push(tick, &encoded[..len], (), now)?;
                if tick % 7 == 0 {
                    stream.push(tick, &encoded[..len], (), now)?;
                }
            }
        }
        peak_packets = peak_packets.max(streams.iter().map(JitterBuffer::pending).sum::<usize>());
        for stream in &mut streams {
            stream.poll(now, &mut output)?;
        }
    }
    let elapsed = before.elapsed();
    let decoded: u64 = streams.iter().map(|s| s.statistics().decoded).sum();
    let concealed: u64 = streams.iter().map(|s| s.statistics().concealed).sum();
    let resets: u64 = streams.iter().map(|s| s.statistics().resets).sum();
    if decoded != FRAMES * SPEAKERS as u64 * 9 / 10
        || concealed != FRAMES * SPEAKERS as u64 / 10
        || resets != 0
    {
        return Err("unexpected load-probe playout counts".into());
    }
    println!(
        "{{\"speakers\":{SPEAKERS},\"frames_per_speaker\":{FRAMES},\"decoded\":{decoded},\"concealed\":{concealed},\"peak_queued_packets\":{peak_packets},\"peak_encoded_bytes\":{},\"worker_wall_seconds\":{:.3}}}",
        peak_packets * len,
        elapsed.as_secs_f64()
    );
    Ok(())
}
