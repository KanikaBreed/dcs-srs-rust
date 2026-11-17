use srs_audio::{
    CodecError, Decoder, Encoder, INPUT_SAMPLES, MAX_OPUS_BYTES, OUTPUT_SAMPLES,
    jitter::{FrameKind, JitterBuffer, MAX_PENDING, Push},
};
use std::time::Duration;
fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}
fn packets(count: usize) -> Vec<Vec<u8>> {
    let mut encoder = Encoder::new().unwrap();
    (0..count)
        .map(|frame| {
            let samples = std::array::from_fn(|i| {
                (12000.0
                    * (std::f64::consts::TAU * 700.0 * (frame * INPUT_SAMPLES + i) as f64
                        / 16000.0)
                        .sin()) as i16
            });
            let mut bytes = [0; MAX_OPUS_BYTES];
            let len = encoder.encode(&samples, &mut bytes).unwrap();
            bytes[..len].to_vec()
        })
        .collect()
}
#[test]
fn reordering_and_duplicates_decode_in_sender_order() {
    let packets = packets(3);
    let mut jitter = JitterBuffer::new().unwrap();
    // First arrival is not necessarily the first frame in the burst.
    assert_eq!(
        jitter.push(11, &packets[1], 11, ms(0)).unwrap(),
        Push::Queued
    );
    jitter.push(10, &packets[0], 10, ms(20)).unwrap();
    jitter.push(12, &packets[2], 12, ms(30)).unwrap();
    assert_eq!(
        jitter.push(11, &packets[1], 11, ms(40)).unwrap(),
        Push::Duplicate
    );
    let mut actual = [99; OUTPUT_SAMPLES];
    assert!(jitter.poll(ms(79), &mut actual).unwrap().is_none());
    assert!(actual.iter().all(|&n| n == 0));
    let mut decoder = Decoder::new().unwrap();
    for (i, packet) in packets.iter().enumerate() {
        let frame = jitter
            .poll(ms(80 + i as u64 * 40), &mut actual)
            .unwrap()
            .unwrap();
        assert_eq!(
            (frame.sequence, frame.metadata, frame.kind),
            (10 + i as u64, 10 + i, FrameKind::Decoded)
        );
        let mut expected = [0; OUTPUT_SAMPLES];
        decoder.decode(packet, &mut expected).unwrap();
        assert_eq!(
            actual, expected,
            "decoder must see reordered encoded frames"
        );
    }
    assert_eq!(
        jitter.push(10, &packets[0], 10, ms(170)).unwrap(),
        Push::Late
    );
    assert_eq!(jitter.statistics().duplicates, 1);
}
#[test]
fn loss_is_concealed_at_most_twice_then_resynchronizes() {
    let packets = packets(5);
    let mut jitter = JitterBuffer::new().unwrap();
    jitter.push(1, &packets[0], 1, ms(0)).unwrap();
    jitter.push(5, &packets[4], 5, ms(40)).unwrap();
    let mut pcm = [0; OUTPUT_SAMPLES];
    for (time, sequence, kind) in [
        (80, 1, FrameKind::Decoded),
        (120, 2, FrameKind::Concealed),
        (160, 3, FrameKind::Concealed),
    ] {
        let frame = jitter.poll(ms(time), &mut pcm).unwrap().unwrap();
        assert_eq!((frame.sequence, frame.kind), (sequence, kind));
    }
    assert!(jitter.poll(ms(200), &mut pcm).unwrap().is_none());
    let frame = jitter.poll(ms(280), &mut pcm).unwrap().unwrap();
    assert_eq!(
        (frame.sequence, frame.metadata, frame.kind),
        (5, 5, FrameKind::Decoded)
    );
    for time in [320, 360] {
        assert_eq!(
            jitter.poll(ms(time), &mut pcm).unwrap().unwrap().kind,
            FrameKind::Concealed
        );
    }
    assert!(jitter.poll(ms(400), &mut pcm).unwrap().is_none());
    assert_eq!(jitter.deadline(), None);
}
#[test]
fn stalled_worker_drops_backlog_and_never_replays_it() {
    let packet = packets(1).remove(0);
    let mut jitter = JitterBuffer::new().unwrap();
    for sequence in 1..=8 {
        jitter.push(sequence, &packet, (), ms(0)).unwrap();
    }
    assert_eq!(jitter.pending(), MAX_PENDING);
    let mut pcm = [99; OUTPUT_SAMPLES];
    assert!(jitter.poll(ms(200), &mut pcm).unwrap().is_none());
    assert_eq!(jitter.pending(), 0);
    assert_eq!(jitter.push(8, &packet, (), ms(201)).unwrap(), Push::Late);
    jitter.push(9, &packet, (), ms(202)).unwrap();
    assert_eq!(jitter.poll(ms(282), &mut pcm).unwrap().unwrap().sequence, 9);
    // One overdue frame may play, but a second poll at the same time cannot
    // compress the next 40 ms frame into a catch-up burst.
    jitter.push(10, &packet, (), ms(290)).unwrap();
    jitter.push(11, &packet, (), ms(300)).unwrap();
    assert_eq!(
        jitter.poll(ms(350), &mut pcm).unwrap().unwrap().sequence,
        10
    );
    assert!(jitter.poll(ms(350), &mut pcm).unwrap().is_none());
}
#[test]
fn huge_gaps_sequence_exhaustion_and_idle_restart_are_bounded() {
    let packet = packets(1).remove(0);
    let mut jitter = JitterBuffer::new().unwrap();
    jitter.push(1, &packet, (), ms(0)).unwrap();
    assert_eq!(
        jitter.push(1_000_000, &packet, (), ms(10)).unwrap(),
        Push::Resynchronized
    );
    assert_eq!(jitter.pending(), 1);
    assert_eq!(jitter.push(2, &packet, (), ms(20)).unwrap(), Push::Late);
    let mut pcm = [0; OUTPUT_SAMPLES];
    assert_eq!(
        jitter.poll(ms(90), &mut pcm).unwrap().unwrap().sequence,
        1_000_000
    );
    jitter.push(u64::MAX, &packet, (), ms(1000)).unwrap();
    assert_eq!(
        jitter.poll(ms(1080), &mut pcm).unwrap().unwrap().sequence,
        u64::MAX
    );
    assert_eq!(jitter.deadline(), None);
    assert_eq!(jitter.push(0, &packet, (), ms(1100)).unwrap(), Push::Late);
}
#[test]
fn bad_packets_and_clock_regressions_do_not_enter_queue() {
    let packet = packets(1).remove(0);
    let mut jitter = JitterBuffer::new().unwrap();
    assert_eq!(
        jitter.push(1, &[], (), ms(0)),
        Err(CodecError::InvalidPacket)
    );
    assert_eq!(
        jitter.push(1, &[0; 4001], (), ms(0)),
        Err(CodecError::InvalidPacket)
    );
    jitter.push(1, &packet, (), ms(100)).unwrap();
    assert_eq!(
        jitter.push(2, &packet, (), ms(99)),
        Err(CodecError::ClockReversed)
    );
    assert_eq!(jitter.pending(), 1);
    let mut pcm = [99; OUTPUT_SAMPLES];
    assert_eq!(
        jitter.poll(ms(99), &mut pcm),
        Err(CodecError::ClockReversed)
    );
    assert!(pcm.iter().all(|&n| n == 0));
}
#[test]
fn deterministic_network_impairment_preserves_playout_cadence() {
    let packets = packets(100);
    let mut arrivals: Vec<_> = packets
        .iter()
        .enumerate()
        .filter(|(i, _)| i % 10 != 5)
        .flat_map(|(i, p)| {
            // A 0/50/5 ms pattern causes reordering; every seventh packet repeats.
            let at = i as u64 * 40 + [0, 50, 5][i % 3];
            let mut v = vec![(at, i as u64, p)];
            if i % 7 == 0 {
                v.push((at + 1, i as u64, p));
            }
            v
        })
        .collect();
    arrivals.sort_by_key(|(at, _, _)| *at);
    let mut jitter = JitterBuffer::new().unwrap();
    let mut pcm = [0; OUTPUT_SAMPLES];
    let mut cursor = 0;
    let mut decoded = 0;
    let mut concealed = 0;
    for tick in 0..=4040 {
        while cursor < arrivals.len() && arrivals[cursor].0 == tick {
            let (_, sequence, p) = arrivals[cursor];
            jitter.push(sequence, p, (), ms(tick)).unwrap();
            cursor += 1;
            assert!(jitter.pending() <= MAX_PENDING);
        }
        if let Some(frame) = jitter.poll(ms(tick), &mut pcm).unwrap() {
            assert_eq!(tick, 80 + frame.sequence * 40);
            match frame.kind {
                FrameKind::Decoded => decoded += 1,
                FrameKind::Concealed => concealed += 1,
            }
        }
    }
    assert_eq!((decoded, concealed), (90, 10));
    assert_eq!(jitter.statistics().codec_errors, 0);
    assert!(jitter.statistics().duplicates > 0);
}

#[test]
fn timer_overshoot_does_not_accumulate_latency() {
    let packets = packets(100);
    let mut jitter = JitterBuffer::new().unwrap();
    let mut pcm = [0; OUTPUT_SAMPLES];
    for tick in 0..=4041 {
        if tick % 40 == 0 && tick / 40 < 100 {
            let index = (tick / 40) as usize;
            jitter
                .push(index as u64, &packets[index], (), ms(tick))
                .unwrap();
        }
        if tick >= 81 && (tick - 81) % 40 == 0 {
            let frame = jitter.poll(ms(tick), &mut pcm).unwrap().unwrap();
            assert_eq!(frame.sequence, (tick - 81) / 40);
            assert_eq!(frame.kind, FrameKind::Decoded);
        }
    }
    assert_eq!(jitter.statistics().resets, 0);
}

#[test]
fn new_ptt_burst_does_not_lose_sequences_predicted_by_tail_concealment() {
    let packet = packets(1).remove(0);
    for pause in [201, 1000] {
        let mut jitter = JitterBuffer::new().unwrap();
        let mut pcm = [0; OUTPUT_SAMPLES];
        jitter.push(1, &packet, (), ms(0)).unwrap();
        assert_eq!(jitter.poll(ms(80), &mut pcm).unwrap().unwrap().sequence, 1);
        for time in [120, 160] {
            assert_eq!(
                jitter.poll(ms(time), &mut pcm).unwrap().unwrap().kind,
                FrameKind::Concealed
            );
        }
        if pause == 201 {
            assert!(jitter.poll(ms(200), &mut pcm).unwrap().is_none());
        }
        // The next actual sequence is 2, even though PLC predicted 2 and 3.
        assert!(matches!(
            jitter.push(2, &packet, (), ms(pause)).unwrap(),
            Push::Queued | Push::Resynchronized
        ));
        assert_eq!(
            jitter
                .poll(ms(pause + 80), &mut pcm)
                .unwrap()
                .unwrap()
                .sequence,
            2
        );
        assert_eq!(
            jitter.push(1, &packet, (), ms(pause + 81)).unwrap(),
            Push::Late
        );
    }
}
