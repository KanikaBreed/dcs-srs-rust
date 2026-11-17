use srs_protocol::voice::*;

const SINGLE: &[u8] = include_bytes!("fixtures/single-voice.bin");
const RELAYED: &[u8] = include_bytes!("fixtures/single-relayed.bin");
const MULTI: &[u8] = include_bytes!("fixtures/multi-voice.bin");
const DISTINCT: &[u8] = include_bytes!("fixtures/distinct-identities.bin");
const KEEPALIVE: &[u8] = include_bytes!("fixtures/keepalive.bin");

fn id() -> ClientId {
    ClientId::from_bytes(KEEPALIVE).unwrap()
}

#[test]
fn published_upstream_single_packet_matches_fields_and_encoding() {
    let expected = VoicePacket {
        audio: &[0, 1, 2, 3, 4, 5],
        frequencies: vec![Frequency {
            hz: 100.0,
            modulation: Modulation::HAVEQUICK,
            encryption: 0,
        }],
        unit_id: 1,
        packet_number: 1,
        retransmission_count: 4,
        original_client_id: id(),
        client_id: id(),
    };
    assert_eq!(VoicePacket::decode(SINGLE).unwrap(), expected);
    assert_eq!(expected.encode().unwrap(), SINGLE);
    assert_eq!(SINGLE.len(), 79);
    assert_eq!(
        VoicePacket::decode(RELAYED).unwrap().retransmission_count,
        231
    );
}

#[test]
fn published_upstream_multiple_frequency_packet_matches() {
    let expected = VoicePacket {
        audio: &[0, 1, 2, 3, 4, 5],
        frequencies: vec![
            Frequency {
                hz: 251000000.0,
                modulation: Modulation::AM,
                encryption: 0,
            },
            Frequency {
                hz: 30000000.0,
                modulation: Modulation::FM,
                encryption: 0,
            },
            Frequency {
                hz: 251000000.0,
                modulation: Modulation::AM,
                encryption: 1,
            },
        ],
        unit_id: 1,
        packet_number: 1,
        retransmission_count: 254,
        original_client_id: id(),
        client_id: id(),
    };
    assert_eq!(VoicePacket::decode(MULTI).unwrap(), expected);
    assert_eq!(expected.encode().unwrap(), MULTI);
    assert_eq!(MULTI.len(), 99);
}

#[test]
fn independent_fixture_preserves_identity_order_unsigned_widths_and_unknown_modulation() {
    let packet = VoicePacket::decode(DISTINCT).unwrap();
    assert_eq!(packet.client_id.as_bytes(), b"_____________________w");
    assert_eq!(
        packet.original_client_id.as_bytes(),
        b"AAAAAAAAAAAAAAAAAAAAAA"
    );
    assert_eq!(packet.unit_id, u32::MAX);
    assert_eq!(packet.packet_number, u64::MAX);
    assert_eq!(packet.retransmission_count, 255);
    assert_eq!(packet.frequencies[0].modulation.0, 255);
    assert_eq!(packet.frequencies[0].encryption, 255);
    assert_eq!(packet.frequencies[0].hz, 0.0);
    assert_eq!(packet.encode().unwrap(), DISTINCT);
}

#[test]
fn keepalive_is_an_identity_datagram() {
    assert_eq!(
        Datagram::decode(KEEPALIVE).unwrap(),
        Datagram::Keepalive(id())
    );
    assert_eq!(Datagram::Keepalive(id()).encode().unwrap(), KEEPALIVE);
    assert!(matches!(Datagram::decode(SINGLE), Ok(Datagram::Voice(_))));
    for bad in [
        vec![b'a'; 21],
        vec![b'a'; 23],
        vec![b' '; 22],
        vec![255; 22],
    ] {
        assert!(ClientId::from_bytes(&bad).is_err());
    }
}

#[test]
fn all_truncations_and_inconsistent_lengths_are_rejected() {
    for fixture in [SINGLE, RELAYED, MULTI, DISTINCT] {
        for len in 0..fixture.len() {
            assert!(
                VoicePacket::decode(&fixture[..len]).is_err(),
                "prefix {len}"
            );
        }
        let mut extra = fixture.to_vec();
        extra.push(0);
        assert_eq!(
            VoicePacket::decode(&extra).unwrap_err(),
            PacketError::LengthMismatch
        );
        for header in [0, 2, 4] {
            let mut corrupt = fixture.to_vec();
            corrupt[header..header + 2].copy_from_slice(&u16::MAX.to_le_bytes());
            assert!(VoicePacket::decode(&corrupt).is_err());
        }
    }
    let mut corrupt = SINGLE.to_vec();
    corrupt[4..6].copy_from_slice(&9_u16.to_le_bytes());
    assert_eq!(
        VoicePacket::decode(&corrupt).unwrap_err(),
        PacketError::InvalidFrequencyLength
    );
    corrupt = SINGLE.to_vec();
    corrupt[2..4].copy_from_slice(&7_u16.to_le_bytes());
    assert_eq!(
        VoicePacket::decode(&corrupt).unwrap_err(),
        PacketError::LengthMismatch
    );
}

#[test]
fn invalid_numeric_frequencies_and_identities_are_rejected_in_both_directions() {
    for hz in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        let mut corrupt = SINGLE.to_vec();
        corrupt[12..20].copy_from_slice(&hz.to_le_bytes());
        assert_eq!(
            VoicePacket::decode(&corrupt).unwrap_err(),
            PacketError::InvalidFrequency
        );
        let mut packet = VoicePacket::decode(SINGLE).unwrap();
        packet.frequencies[0].hz = hz;
        assert_eq!(packet.encode().unwrap_err(), PacketError::InvalidFrequency);
    }
    for offset in [SINGLE.len() - 1, SINGLE.len() - 23] {
        let mut corrupt = SINGLE.to_vec();
        corrupt[offset] = 0;
        assert_eq!(
            VoicePacket::decode(&corrupt).unwrap_err(),
            PacketError::InvalidId
        );
    }
}

#[test]
fn packet_bounds_do_not_confuse_radio_ui_capacity_with_wire_capacity() {
    let mut packet = VoicePacket::decode(SINGLE).unwrap();
    packet.frequencies = vec![packet.frequencies[0]; 12];
    assert_eq!(
        VoicePacket::decode(&packet.encode().unwrap())
            .unwrap()
            .frequencies
            .len(),
        12
    );
    packet.frequencies.truncate(1);
    let audio = vec![0; MAX_PACKET_BYTES - HEADER_BYTES - TAIL_BYTES - FREQUENCY_BYTES];
    packet.audio = &audio;
    let encoded = packet.encode().unwrap();
    assert_eq!(encoded.len(), MAX_PACKET_BYTES);
    assert_eq!(VoicePacket::decode(&encoded).unwrap().audio, audio);
    let too_much_audio = vec![0; audio.len() + 1];
    packet.audio = &too_much_audio;
    assert_eq!(packet.encode().unwrap_err(), PacketError::TooLarge);
    assert_eq!(
        VoicePacket::decode(&vec![0; MAX_PACKET_BYTES + 1]).unwrap_err(),
        PacketError::TooLarge
    );
    packet.audio = &[];
    packet.frequencies = vec![packet.frequencies[0]; 6554];
    assert_eq!(packet.encode().unwrap_err(), PacketError::TooLarge);
}

#[test]
fn audio_payload_is_borrowed_and_empty_segments_are_structurally_valid() {
    let mut packet = VoicePacket::decode(SINGLE).unwrap();
    assert_eq!(packet.audio.as_ptr(), SINGLE[HEADER_BYTES..].as_ptr());
    packet.audio = &[];
    packet.frequencies.clear();
    let encoded = packet.encode().unwrap();
    assert_eq!(encoded.len(), HEADER_BYTES + TAIL_BYTES);
    assert_eq!(VoicePacket::decode(&encoded).unwrap(), packet);
}

#[test]
fn every_single_byte_mutation_is_rejected_or_preserved_without_panicking() {
    for fixture in [SINGLE, MULTI, DISTINCT] {
        let mut input = fixture.to_vec();
        for index in 0..fixture.len() {
            for value in 0..=255 {
                input[index] = value;
                if let Ok(packet) = Datagram::decode(&input) {
                    assert_eq!(packet.encode().unwrap(), input);
                }
            }
            input[index] = fixture[index];
        }
    }
}

#[test]
fn modulation_constants_match_the_actual_csharp_enum() {
    let values: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/modulations.json")).unwrap();
    for (name, value) in [
        ("AM", Modulation::AM),
        ("FM", Modulation::FM),
        ("INTERCOM", Modulation::INTERCOM),
        ("DISABLED", Modulation::DISABLED),
        ("HAVEQUICK", Modulation::HAVEQUICK),
        ("SATCOM", Modulation::SATCOM),
        ("MIDS", Modulation::MIDS),
        ("SINCGARS", Modulation::SINCGARS),
    ] {
        assert_eq!(values[name].as_u64().unwrap(), u64::from(value.0));
    }
}
