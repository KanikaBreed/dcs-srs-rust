#![no_main]
use libfuzzer_sys::fuzz_target;
use srs_protocol::control::{ControlDecoder, ControlError, ControlMessage};

fuzz_target!(|bytes: &[u8]| {
    if let Ok(message) = ControlMessage::decode_json(bytes) {
        if let Ok(encoded) = message.encode() {
            assert_eq!(ControlMessage::decode_json(&encoded).unwrap(), message);
        }
    }
    let mut decoder = ControlDecoder::with_limit(4096).unwrap();
    for mut chunk in bytes.chunks(17) {
        while !chunk.is_empty() {
            let before = chunk.len();
            match decoder.decode(&mut chunk) {
                Ok(_) => assert!(chunk.len() < before),
                Err(_) => {
                    assert_eq!(decoder.buffered_len(), 0);
                    assert_eq!(
                        decoder.decode(&mut &b""[..]).unwrap_err(),
                        ControlError::DecoderFailed
                    );
                    return;
                }
            }
            assert!(decoder.buffered_len() <= 4096);
        }
    }
    let _ = decoder.finish();
});
