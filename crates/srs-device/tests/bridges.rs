use srs_audio::{Decoder, Encoder, INPUT_SAMPLES, MAX_OPUS_BYTES, OUTPUT_SAMPLES};
use srs_device::{
    Error,
    bridge::*,
    native::{Direction, choose_config},
};
use std::time::{Duration, Instant};

fn time(start: Instant, ms: u64) -> Instant {
    start + Duration::from_millis(ms)
}
fn converted_tone(hz: f32, chunks: &[usize]) -> [i16; INPUT_SAMPLES] {
    let start = Instant::now();
    let (mut callback, mut reader, _) = capture_bridge(2, start).unwrap();
    let permit = reader.begin(start).unwrap();
    let mut offset = 0;
    let mut index = 0;
    let mut result = None;
    while offset < 1920 {
        let count = chunks[index % chunks.len()].min(1920 - offset);
        let samples: Vec<_> = (offset..offset + count)
            .flat_map(|i| {
                let sample = 0.5 * (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin();
                [sample, sample]
            })
            .collect();
        let now = start + Duration::from_secs_f64((offset + count) as f64 / 48_000.0);
        callback.process(&samples, now);
        result = reader.next_frame(&permit, now).unwrap().or(result);
        offset += count;
        index += 1;
    }
    result.unwrap().samples
}
fn rms(samples: &[i16]) -> f64 {
    (samples
        .iter()
        .map(|&s| (s as f64 / 32768.0).powi(2))
        .sum::<f64>()
        / samples.len() as f64)
        .sqrt()
}

#[test]
fn callback_fragmentation_keeps_exact_frame_boundaries_and_antialias_filtering() {
    let regular = converted_tone(700.0, &[480]);
    let fragmented = converted_tone(700.0, &[7, 29, 81, 301, 2]);
    assert_eq!(regular, fragmented);
    assert!((0.34..0.36).contains(&rms(&regular[64..])));
    let high = converted_tone(12_000.0, &[79, 480, 333]);
    assert!(
        rms(&high[64..]) < 0.0004,
        "12 kHz must not alias into 4 kHz: {}",
        rms(&high[64..])
    );
    let passband = converted_tone(6000.0, &[480]);
    assert!((0.33..0.37).contains(&rms(&passband[64..])));
}
#[test]
fn capture_requires_a_fresh_permit_after_release_or_expiry() {
    let start = Instant::now();
    let (mut callback, mut reader, _) = capture_bridge(1, start).unwrap();
    callback.process(&[1.0f32; 1920], start);
    let permit = reader.begin(start).unwrap();
    assert!(reader.next_frame(&permit, start).unwrap().is_none());
    callback.process(&[0.5f32; 1920], time(start, 10));
    let frame = reader
        .next_frame(&permit, time(start, 10))
        .unwrap()
        .unwrap();
    reader.release();
    assert_eq!(
        reader.validate_frame(&frame, time(start, 11)),
        Err(Error::StaleFrame)
    );
    let fresh = reader.begin(time(start, 20)).unwrap();
    assert_eq!(
        reader.renew(&permit, time(start, 21)),
        Err(Error::StaleFrame)
    );
    callback.process(&[-0.5f32; 1920], time(start, 30));
    let fresh_frame = reader.next_frame(&fresh, time(start, 30)).unwrap().unwrap();
    assert!(fresh_frame.samples[100..].iter().all(|&s| s < -16_000));
    assert_eq!(
        reader.renew(&fresh, time(start, 270)),
        Err(Error::LeaseExpired)
    );
    assert_eq!(
        reader.renew(&fresh, time(start, 271)),
        Err(Error::StaleFrame)
    );
    assert!(reader.begin(time(start, 300)).is_ok());
}
#[test]
fn capture_overflow_discards_partial_frames_across_a_sequence_gap() {
    let start = Instant::now();
    let (mut callback, mut reader, signal) = capture_bridge(1, start).unwrap();
    let permit = reader.begin(start).unwrap();
    callback.process(&[0.5f32; 480], start);
    assert!(reader.next_frame(&permit, start).unwrap().is_none());
    for _ in 0..9 {
        callback.process(&[0.5f32; 480], time(start, 1));
    }
    assert_eq!(signal.statistics().dropped, 1);
    assert!(
        reader
            .next_frame(&permit, time(start, 2))
            .unwrap()
            .is_some()
    );
    assert!(
        reader
            .next_frame(&permit, time(start, 2))
            .unwrap()
            .is_some()
    );
    assert!(
        reader
            .next_frame(&permit, time(start, 2))
            .unwrap()
            .is_none()
    );
    for _ in 0..3 {
        callback.process(&[-0.5f32; 480], time(start, 3));
        assert!(
            reader
                .next_frame(&permit, time(start, 3))
                .unwrap()
                .is_none()
        );
    }
    callback.process(&[-0.5f32; 480], time(start, 4));
    let frame = reader.next_frame(&permit, time(start, 4)).unwrap().unwrap();
    assert!(frame.samples[100..].iter().all(|&s| s < -16_000));
}
#[test]
fn stale_capture_and_output_buffers_never_resume_after_a_worker_pause() {
    let start = Instant::now();
    let (mut callback, mut reader, _) = capture_bridge(1, start).unwrap();
    let permit = reader.begin(start).unwrap();
    callback.process(&[1.0f32; 1920], start);
    assert!(
        reader
            .next_frame(&permit, time(start, 80))
            .unwrap()
            .is_none()
    );
    let (mut writer, mut output, _) = playback_bridge(2).unwrap();
    writer.enqueue(&[16000; OUTPUT_SAMPLES], start).unwrap();
    let mut samples = [99.0f32; 960];
    output.process(&mut samples, time(start, 80));
    assert!(samples.iter().all(|&s| s == 0.0));
}
#[test]
fn output_overflow_and_flush_drop_old_audio_including_partial_frames() {
    let start = Instant::now();
    let (mut writer, mut callback, signal) = playback_bridge(2).unwrap();
    writer.enqueue(&[16384; OUTPUT_SAMPLES], start).unwrap();
    let mut pcm = [0.0f32; 222];
    callback.process(&mut pcm, start);
    assert!(pcm.iter().all(|&s| s == 0.5));
    writer.flush().unwrap();
    callback.process(&mut pcm, start);
    assert!(pcm.iter().all(|&s| s == 0.0));
    for _ in 0..PLAYBACK_FRAMES {
        writer.enqueue(&[16384; OUTPUT_SAMPLES], start).unwrap();
    }
    assert_eq!(
        writer.enqueue(&[100; OUTPUT_SAMPLES], start),
        Err(Error::QueueFull)
    );
    callback.process(&mut pcm, start);
    assert!(pcm.iter().all(|&s| s == 0.0));
    writer.enqueue(&[8192; OUTPUT_SAMPLES], start).unwrap();
    callback.process(&mut pcm, start);
    assert!(pcm.iter().all(|&s| s == 0.25));
    assert!(signal.statistics().dropped >= 4);
}
#[test]
fn sample_formats_are_bounded_and_nonfinite_input_is_silent() {
    let start = Instant::now();
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let (mut callback, mut reader, _) = capture_bridge(1, start).unwrap();
        let permit = reader.begin(start).unwrap();
        callback.process(&[value; 1920], start);
        assert!(
            reader
                .next_frame(&permit, start)
                .unwrap()
                .unwrap()
                .samples
                .iter()
                .all(|&s| s == 0)
        );
    }
    let (mut callback, mut reader, _) = capture_bridge(2, start).unwrap();
    let permit = reader.begin(start).unwrap();
    callback.process(&[u16::MAX; 3840], start);
    let frame = reader.next_frame(&permit, start).unwrap().unwrap();
    assert!(frame.samples[100..].iter().all(|&s| s > 32000));
    let (mut callback, mut reader, _) = capture_bridge(2, start).unwrap();
    let permit = reader.begin(start).unwrap();
    let stereo: [i16; 3840] = std::array::from_fn(|i| if i % 2 == 0 { 16384 } else { -16384 });
    callback.process(&stereo, start);
    assert!(
        reader
            .next_frame(&permit, start)
            .unwrap()
            .unwrap()
            .samples
            .iter()
            .all(|&s| s == 0)
    );
    let (mut writer, mut output, _) = playback_bridge(1).unwrap();
    let mut pcm = [0u16; 17];
    output.process(&mut pcm, start);
    assert_eq!(pcm, [32768; 17]);
    writer.enqueue(&[i16::MIN; OUTPUT_SAMPLES], start).unwrap();
    output.process(&mut pcm, start);
    assert_eq!(pcm, [0; 17]);
}
#[test]
fn failures_and_owner_drop_stop_capture_and_silence_output() {
    let start = Instant::now();
    let (mut input, mut reader, signal) = capture_bridge(1, start).unwrap();
    let permit = reader.begin(start).unwrap();
    input.process(&[0.5f32; 1920], start);
    signal.stop();
    assert!(matches!(
        reader.next_frame(&permit, start),
        Err(Error::Stopped)
    ));
    assert_eq!(reader.begin(start), Err(Error::Stopped));
    let (mut writer, mut output, signal) = playback_bridge(1).unwrap();
    writer.enqueue(&[16384; OUTPUT_SAMPLES], start).unwrap();
    drop(writer);
    let mut pcm = [99i16; 480];
    output.process(&mut pcm, start);
    assert!(signal.is_stopped());
    assert_eq!(pcm, [0; 480]);
    let (mut input, reader, signal) = capture_bridge(1, start).unwrap();
    drop(reader);
    input.process(&[1.0f32; 480], start);
    assert!(signal.is_stopped());
}
#[test]
fn malformed_callback_shapes_fail_closed() {
    let start = Instant::now();
    assert!(capture_bridge(0, start).is_err());
    assert!(playback_bridge(3).is_err());
    let (mut callback, mut reader, signal) = capture_bridge(2, start).unwrap();
    reader.begin(start).unwrap();
    callback.process(&[1.0f32; 3], start);
    assert!(signal.is_stopped());
    let (_writer, mut output, signal) = playback_bridge(1).unwrap();
    let mut too_large = [99.0f32; MAX_CALLBACK_FRAMES + 1];
    output.process(&mut too_large, start);
    assert!(signal.is_stopped());
    assert!(too_large.iter().all(|&s| s == 0.0));
}
#[test]
fn callback_normal_overflow_flush_and_stop_paths_do_not_allocate_or_free() {
    let start = Instant::now();
    let (mut input, mut reader, input_signal) = capture_bridge(2, start).unwrap();
    reader.begin(start).unwrap();
    let (mut writer, mut output, output_signal) = playback_bridge(2).unwrap();
    writer.enqueue(&[1000; OUTPUT_SAMPLES], start).unwrap();
    let mut pcm = [0.0f32; 960];
    let measure = allocation_counter::measure(|| {
        for _ in 0..1000 {
            input.process(&[0.25f32; 960], start);
            output.process(&mut pcm, start);
        }
    });
    assert_eq!((measure.count_total, measure.count_current), (0, 0));
    writer.enqueue(&[1000; OUTPUT_SAMPLES], start).unwrap();
    output.process(&mut pcm, start); // Retain a partly consumed frame.
    writer.flush().unwrap();
    let measure = allocation_counter::measure(|| {
        output.process(&mut pcm, start);
    });
    assert_eq!((measure.count_total, measure.count_current), (0, 0));
    input_signal.stop();
    output_signal.stop();
    let measure = allocation_counter::measure(|| {
        input.process(&[0.25f32; 960], start);
        output.process(&mut pcm, start);
    });
    assert_eq!((measure.count_total, measure.count_current), (0, 0));
}
#[test]
fn generated_audio_crosses_capture_conversion_opus_and_output_bridge() {
    let input = converted_tone(700.0, &[113, 480, 71]);
    let mut encoder = Encoder::new().unwrap();
    let mut decoder = Decoder::new().unwrap();
    let mut bytes = [0; MAX_OPUS_BYTES];
    let len = encoder.encode(&input, &mut bytes).unwrap();
    let mut decoded = [0; OUTPUT_SAMPLES];
    decoder.decode(&bytes[..len], &mut decoded).unwrap();
    let start = Instant::now();
    let (mut writer, mut callback, _) = playback_bridge(2).unwrap();
    writer.enqueue(&decoded, start).unwrap();
    let mut output = [0.0f32; 3840];
    callback.process(&mut output, start);
    let energy = output.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / output.len() as f64;
    assert!(energy > 0.05);
    assert!(output.as_chunks::<2>().0.iter().all(|f| f[0] == f[1]));
}
#[test]
fn device_config_selection_has_no_silent_rate_or_channel_fallback() {
    use cpal::{SampleFormat as F, SupportedBufferSize as B, SupportedStreamConfigRange as R};
    let range = |channels, rate, format| R::new(channels, rate, rate, B::Unknown, format);
    let configs = vec![
        range(8, 48000, F::F32),
        range(2, 44100, F::F32),
        range(1, 48000, F::I16),
        range(2, 48000, F::F32),
        range(1, 48000, F::F32),
    ];
    let input = choose_config(configs.clone().into_iter(), Direction::Input).unwrap();
    let output = choose_config(configs.into_iter(), Direction::Output).unwrap();
    assert_eq!(
        (input.channels(), input.sample_rate(), input.sample_format()),
        (1, 48000, F::F32)
    );
    assert_eq!(output.channels(), 2);
    assert!(
        choose_config(
            [
                range(2, 44100, F::F32),
                range(8, 48000, F::F32),
                range(1, 48000, F::I24)
            ]
            .into_iter(),
            Direction::Output
        )
        .is_err()
    );
}

#[test]
fn a_new_device_cannot_validate_an_old_devices_capture_frame() {
    let start = Instant::now();
    let (mut callback, mut old, _) = capture_bridge(1, start).unwrap();
    let permit = old.begin(start).unwrap();
    callback.process(&[0.5f32; 1920], start);
    let frame = old.next_frame(&permit, start).unwrap().unwrap();
    drop(old);
    let (_callback, mut new, _) = capture_bridge(1, start).unwrap();
    new.begin(start).unwrap();
    assert_eq!(new.validate_frame(&frame, start), Err(Error::StaleFrame));
}
#[test]
fn a_stalled_device_stops_even_if_the_controller_keeps_renewing() {
    let start = Instant::now();
    let (mut callback, mut reader, signal) = capture_bridge(1, start).unwrap();
    let permit = reader.begin(start).unwrap();
    callback.process(&[0.5f32; 480], start);
    callback.process::<f32>(&[], time(start, 100));
    reader.renew(&permit, time(start, 100)).unwrap();
    callback.process::<f32>(&[], time(start, 200));
    reader.renew(&permit, time(start, 200)).unwrap();
    assert_eq!(reader.renew(&permit, time(start, 250)), Err(Error::Stopped));
    assert!(signal.is_stopped());
    assert_eq!(reader.begin(time(start, 251)), Err(Error::Stopped));
}

#[test]
fn concurrent_producer_timestamps_newer_than_the_consumers_snapshot_are_fresh() {
    let start = Instant::now();
    let (mut input, mut reader, _) = capture_bridge(1, start).unwrap();
    let permit = reader.begin(start).unwrap();
    // Model a producer enqueue between the consumer's clock read and queue pop.
    input.process(&[0.5f32; 1920], time(start, 1));
    let frame = reader.next_frame(&permit, start).unwrap().unwrap();
    reader.validate_frame(&frame, time(start, 2)).unwrap();
    let (mut writer, mut output, _) = playback_bridge(1).unwrap();
    writer
        .enqueue(&[16384; OUTPUT_SAMPLES], time(start, 1))
        .unwrap();
    let mut pcm = [0.0f32; 480];
    output.process(&mut pcm, start);
    assert!(pcm.iter().all(|&s| s == 0.5));
}

#[test]
fn stereo_interleaves_and_flushes_both_channels_without_callback_allocations() {
    let start = Instant::now();
    let (mut writer, mut output, _) = playback_bridge(2).unwrap();
    assert_eq!(writer.channels(), 2);
    writer
        .enqueue_stereo(&[16384; OUTPUT_SAMPLES], &[-8192; OUTPUT_SAMPLES], start)
        .unwrap();
    let mut pcm = [0.0_f32; 222];
    let allocation = allocation_counter::measure(|| output.process(&mut pcm, start));
    assert_eq!(allocation.count_total, 0);
    assert_eq!(allocation.bytes_total, 0);
    for pair in pcm.chunks_exact(2) {
        assert_eq!(pair, [0.5, -0.25]);
    }
    writer.flush().unwrap();
    output.process(&mut pcm, start);
    assert!(pcm.iter().all(|s| *s == 0.0));
    writer
        .enqueue_stereo(&[0; OUTPUT_SAMPLES], &[16384; OUTPUT_SAMPLES], start)
        .unwrap();
    let mut pcm = [0_i16; 222];
    output.process(&mut pcm, start);
    for pair in pcm.chunks_exact(2) {
        assert_eq!(pair, [0, 16384]);
    }
    output.process(&mut pcm, time(start, 80));
    assert!(pcm.iter().all(|s| *s == 0), "stale stereo frame survived");
}

#[test]
fn mono_bridge_preserves_mono_gain_and_averages_explicit_stereo_input() {
    let start = Instant::now();
    let (mut writer, mut output, _) = playback_bridge(1).unwrap();
    assert_eq!(writer.channels(), 1);
    writer.enqueue(&[16384; OUTPUT_SAMPLES], start).unwrap();
    let mut pcm = [0_f32; 120];
    output.process(&mut pcm, start);
    assert!(pcm.iter().all(|s| *s == 0.5));
    writer.flush().unwrap();
    writer
        .enqueue_stereo(&[16384; OUTPUT_SAMPLES], &[-8192; OUTPUT_SAMPLES], start)
        .unwrap();
    output.process(&mut pcm, start);
    assert!(pcm.iter().all(|s| *s == 0.125));
}
