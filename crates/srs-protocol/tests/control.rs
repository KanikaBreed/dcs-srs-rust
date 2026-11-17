use serde_json::{Value, json};
use srs_protocol::control::*;

const PING: &[u8] = include_bytes!("fixtures/ping.jsonl");
const SYNC: &[u8] = include_bytes!("fixtures/sync.jsonl");
const EAM: &[u8] = include_bytes!("fixtures/eam.jsonl");
const RADIO: &[u8] = include_bytes!("fixtures/radio-update.jsonl");
const VERSION: &[u8] = include_bytes!("fixtures/version-mismatch.jsonl");

fn decode_chunks(bytes: &[u8], chunk_size: usize) -> Vec<ControlMessage> {
    let mut decoder = ControlDecoder::default();
    let mut messages = Vec::new();
    for mut input in bytes.chunks(chunk_size) {
        while !input.is_empty() {
            let before = input.len();
            if let Some(message) = decoder.decode(&mut input).unwrap() {
                messages.push(message);
            }
            assert!(input.len() < before);
        }
    }
    decoder.finish().unwrap();
    messages
}

#[test]
fn upstream_json_fixtures_roundtrip_semantically_with_exact_field_names() {
    for (bytes, kind) in [
        (PING, MessageType::PING),
        (SYNC, MessageType::SYNC),
        (EAM, MessageType::EXTERNAL_AWACS_MODE_PASSWORD),
        (RADIO, MessageType::RADIO_UPDATE),
        (VERSION, MessageType::VERSION_MISMATCH),
    ] {
        let messages = decode_chunks(bytes, 7);
        assert_eq!(messages.len(), 1);
        let message = &messages[0];
        assert_eq!(message.message_type, kind);
        assert_eq!(message.version.as_deref(), Some("2.3.8.0"));
        let encoded = message.encode().unwrap();
        assert_eq!(encoded.last(), Some(&b'\n'));
        assert_eq!(encoded.iter().filter(|b| **b == b'\n').count(), 1);
        assert_eq!(
            serde_json::from_slice::<Value>(&encoded).unwrap(),
            serde_json::from_slice::<Value>(bytes).unwrap()
        );
    }
    let message = ControlMessage::decode_json(SYNC).unwrap();
    let client = &message.clients.as_ref().unwrap()[0];
    assert_eq!(client["RadioInfo"]["radios"].as_array().unwrap().len(), 11);
    assert_eq!(client["RadioInfo"]["radios"][1]["freq"], 251000000);
    assert_eq!(client["RadioInfo"]["radios"][1]["modulation"], 0);
    assert_eq!(client["Name"], "Fixture TOC <&> é 🚁");
}

#[test]
fn arbitrary_tcp_fragmentation_and_coalescing_preserve_messages() {
    let bytes = [PING, SYNC, EAM].concat();
    let expected = decode_chunks(&bytes, bytes.len());
    for chunk_size in [1, 2, 3, 19, 64, 2048] {
        assert_eq!(decode_chunks(&bytes, chunk_size), expected);
    }
    for split in 0..SYNC.len() {
        let mut decoder = ControlDecoder::default();
        let mut first = &SYNC[..split];
        assert!(decoder.decode(&mut first).unwrap().is_none());
        let mut second = &SYNC[split..];
        assert_eq!(decoder.decode(&mut second).unwrap().unwrap(), expected[1]);
        assert!(first.is_empty() && second.is_empty());
        decoder.finish().unwrap();
    }
}

#[test]
fn utf8_split_and_crlf_are_accepted_without_adding_extra_frames() {
    let input = "{\"MsgType\":1,\"Extra\":\"é🚁\"}\r\n";
    let messages = decode_chunks(input.as_bytes(), 1);
    assert_eq!(messages[0].extensions()["Extra"], "é🚁");
    let mut decoder = ControlDecoder::default();
    assert!(decoder.decode(&mut &b""[..]).unwrap().is_none());
    decoder.finish().unwrap();
}

#[test]
fn future_control_fields_and_unknown_numeric_types_are_preserved() {
    let data = br#"{"MsgType":1234,"Future":{"arr":[1,true,null]},"Client":{"custom":3}}"#;
    let message = ControlMessage::decode_json(data).unwrap();
    assert_eq!(message.message_type, MessageType(1234));
    assert_eq!(
        message.extensions()["Future"],
        json!({"arr": [1, true, null]})
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&message.encode().unwrap()).unwrap(),
        serde_json::from_slice::<Value>(data).unwrap()
    );
}

#[test]
fn constructor_uses_explicit_compatibility_version_and_omits_absent_fields() {
    let message = ControlMessage::new(MessageType::SYNC, "9.8.7.6");
    let value: Value = serde_json::from_slice(&message.encode().unwrap()).unwrap();
    assert_eq!(value, json!({"MsgType":2,"Version":"9.8.7.6"}));
    let decoded =
        ControlMessage::decode_json(br#"{"MsgType":1,"Version":null,"Client":null}"#).unwrap();
    assert!(decoded.version.is_none() && decoded.client.is_none());
}

#[test]
fn frame_limit_includes_json_whitespace_but_excludes_lf() {
    let frame = b"{\"MsgType\":1}";
    let mut decoder = ControlDecoder::with_limit(frame.len()).unwrap();
    assert!(decoder.decode(&mut &frame[..]).unwrap().is_none());
    assert_eq!(decoder.buffered_len(), frame.len());
    assert!(decoder.decode(&mut &b"\n"[..]).unwrap().is_some());
    decoder.finish().unwrap();
    for bytes in [frame.to_vec(), [frame.as_slice(), b"\n"].concat()] {
        let mut decoder = ControlDecoder::with_limit(frame.len() - 1).unwrap();
        assert_eq!(
            decoder.decode(&mut bytes.as_slice()).unwrap_err(),
            ControlError::FrameTooLarge
        );
        assert_eq!(decoder.buffered_len(), 0);
        let mut next = PING;
        assert_eq!(
            decoder.decode(&mut next).unwrap_err(),
            ControlError::DecoderFailed
        );
    }
    assert!(ControlDecoder::with_limit(0).is_err());
    assert!(ControlDecoder::with_limit(MAX_FRAME_BYTES + 1).is_err());
}

#[test]
fn large_unterminated_frames_and_large_encoded_messages_are_bounded() {
    let bytes = vec![b' '; MAX_FRAME_BYTES + 1];
    let mut decoder = ControlDecoder::default();
    assert_eq!(
        decoder.decode(&mut bytes.as_slice()).unwrap_err(),
        ControlError::FrameTooLarge
    );
    assert_eq!(decoder.buffered_len(), 0);
    assert_eq!(
        ControlMessage::decode_json(&bytes).unwrap_err(),
        ControlError::FrameTooLarge
    );
    let mut message = ControlMessage::new(MessageType::EXTERNAL_AWACS_MODE_PASSWORD, "2.3.8.0");
    message.external_awacs_mode_password = Some("x".repeat(MAX_FRAME_BYTES));
    assert_eq!(message.encode().unwrap_err(), ControlError::FrameTooLarge);
}

#[test]
fn eof_detects_truncated_frames_and_errors_fail_closed() {
    let mut decoder = ControlDecoder::default();
    assert!(
        decoder
            .decode(&mut &PING[..PING.len() - 1])
            .unwrap()
            .is_none()
    );
    assert_eq!(decoder.finish().unwrap_err(), ControlError::TruncatedFrame);
    let mut decoder = ControlDecoder::default();
    let input = [PING, b"bad-json\n", PING].concat();
    let mut input = input.as_slice();
    assert!(decoder.decode(&mut input).unwrap().is_some());
    assert!(matches!(
        decoder.decode(&mut input),
        Err(ControlError::InvalidJson { .. })
    ));
    assert_eq!(
        decoder.decode(&mut input).unwrap_err(),
        ControlError::DecoderFailed
    );
    assert_eq!(decoder.finish().unwrap_err(), ControlError::DecoderFailed);
}

#[test]
fn malformed_json_invalid_types_and_excessive_nesting_are_rejected() {
    for bytes in [
        b"".as_slice(),
        b"null",
        b"[]",
        b"{}",
        b"{\"MsgType\":\"1\"}",
        b"{\"MsgType\":2147483648}",
        b"{\"MsgType\":1,\"MsgType\":2}",
        b"{\"MsgType\":1,\"Client\":[]}",
        b"{\"MsgType\":1,\"ServerSettings\":{\"x\":true}}",
        b"{\"MsgType\":1} trailing",
        b"\xff",
    ] {
        assert!(matches!(
            ControlMessage::decode_json(bytes),
            Err(ControlError::InvalidJson { .. })
        ));
    }
    let deep = format!(
        "{{\"MsgType\":1,\"extra\":{}0{}}}",
        "[".repeat(200),
        "]".repeat(200)
    );
    assert!(ControlMessage::decode_json(deep.as_bytes()).is_err());
}

#[test]
fn diagnostics_do_not_echo_passwords_or_untrusted_json_values() {
    let message = ControlMessage::decode_json(EAM).unwrap();
    assert!(!format!("{message:?}").contains("fixture-only-password"));
    let err = ControlMessage::decode_json(br#"{"MsgType":"sensitive-value"}"#).unwrap_err();
    assert!(!format!("{err:?} {err}").contains("sensitive-value"));
}

#[test]
fn message_type_constants_match_the_actual_csharp_enum() {
    let values: Value = serde_json::from_str(include_str!("fixtures/message-types.json")).unwrap();
    for (name, value) in [
        ("UPDATE", MessageType::UPDATE),
        ("PING", MessageType::PING),
        ("SYNC", MessageType::SYNC),
        ("RADIO_UPDATE", MessageType::RADIO_UPDATE),
        ("SERVER_SETTINGS", MessageType::SERVER_SETTINGS),
        ("CLIENT_DISCONNECT", MessageType::CLIENT_DISCONNECT),
        ("VERSION_MISMATCH", MessageType::VERSION_MISMATCH),
        (
            "EXTERNAL_AWACS_MODE_PASSWORD",
            MessageType::EXTERNAL_AWACS_MODE_PASSWORD,
        ),
        (
            "EXTERNAL_AWACS_MODE_DISCONNECT",
            MessageType::EXTERNAL_AWACS_MODE_DISCONNECT,
        ),
        (
            "GATEWAY_CLIENT_FULL_UPDATE",
            MessageType::GATEWAY_CLIENT_FULL_UPDATE,
        ),
        (
            "GATEWAY_CLIENT_METADATA_UPDATE",
            MessageType::GATEWAY_CLIENT_METADATA_UPDATE,
        ),
        (
            "GATEWAY_CLIENT_DISCONNECT",
            MessageType::GATEWAY_CLIENT_DISCONNECT,
        ),
    ] {
        assert_eq!(values[name].as_i64().unwrap(), i64::from(value.0));
    }
}

#[test]
fn maximum_sized_encoded_frame_can_be_decoded_with_its_terminator() {
    let mut message = ControlMessage::new(MessageType::PING, "");
    let overhead = message.encode().unwrap().len() - 1;
    message.version = Some("x".repeat(MAX_FRAME_BYTES - overhead));
    let encoded = message.encode().unwrap();
    assert_eq!(encoded.len(), MAX_FRAME_BYTES + 1);
    assert_eq!(ControlMessage::decode_json(&encoded).unwrap(), message);
    assert_eq!(decode_chunks(&encoded, 8192), vec![message]);
}
