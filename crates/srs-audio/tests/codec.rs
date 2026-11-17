use srs_audio::*;

#[test]
fn continuous_frames_decode_to_48khz_with_generated_tone_energy() {
    let mut encoder = Encoder::new().unwrap();
    let mut decoder = Decoder::new().unwrap();
    let mut packet = [0; MAX_OPUS_BYTES];
    let mut output = [0; OUTPUT_SAMPLES];
    for frame in 0..10 {
        let input = std::array::from_fn(|i| {
            (12_000.0
                * (std::f64::consts::TAU * 700.0 * (frame * INPUT_SAMPLES + i) as f64
                    / INPUT_RATE as f64)
                    .sin()) as i16
        });
        let len = encoder.encode(&input, &mut packet).unwrap();
        decoder.decode(&packet[..len], &mut output).unwrap();
        if frame > 0 {
            assert!(
                output.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / OUTPUT_SAMPLES as f64
                    > 4_000_000.0
            );
        }
    }
    assert!(!codec_version().is_empty());
}

#[test]
fn empty_invalid_oversized_and_wrong_duration_packets_are_rejected() {
    let mut decoder = Decoder::new().unwrap();
    let mut output = [0; OUTPUT_SAMPLES];
    assert_eq!(
        decoder.decode(&[], &mut output),
        Err(CodecError::InvalidPacket)
    );
    assert_eq!(
        decoder.decode(&vec![0; MAX_OPUS_BYTES + 1], &mut output),
        Err(CodecError::InvalidPacket)
    );
    assert!(decoder.decode(&[0xff], &mut output).is_err());
    let mut encoder =
        opus::Encoder::new(INPUT_RATE, opus::Channels::Mono, opus::Application::Voip).unwrap();
    let mut packet = [0; MAX_OPUS_BYTES];
    let len = encoder.encode(&[0; 320], &mut packet).unwrap();
    assert_eq!(
        decoder.decode(&packet[..len], &mut output),
        Err(CodecError::WrongDuration)
    );
}

#[test]
fn explicit_loss_concealment_and_reset_keep_the_fixed_output_contract() {
    let mut encoder = Encoder::new().unwrap();
    let mut decoder = Decoder::new().unwrap();
    let mut packet = [0; MAX_OPUS_BYTES];
    let mut output = [0; OUTPUT_SAMPLES];
    let len = encoder.encode(&[0; INPUT_SAMPLES], &mut packet).unwrap();
    decoder.decode(&packet[..len], &mut output).unwrap();
    decoder.conceal(&mut output).unwrap();
    decoder.reset().unwrap();
    encoder.reset().unwrap();
    let len = encoder.encode(&[0; INPUT_SAMPLES], &mut packet).unwrap();
    decoder.decode(&packet[..len], &mut output).unwrap();
    assert!(output.iter().all(|sample| sample.abs() < 10));
}
