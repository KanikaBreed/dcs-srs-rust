use serde_json::Value;
use srs_core::{ReceiveRadio, Reception, frequency_matches, match_radio};
use srs_protocol::voice::{Frequency, Modulation};

#[test]
fn independent_upstream_matcher_cases() {
    let cases: Value = serde_json::from_str(include_str!(
        "../../srs-protocol/tests/fixtures/radio-matching.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let u = |v: &Value| v.as_u64().unwrap();
        let radios: Vec<_> = case["radios"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| ReceiveRadio {
                frequency_hz: r["hz"].as_f64().unwrap(),
                guard_hz: r["guard"].as_f64().unwrap(),
                modulation: Modulation(u(&r["modulation"]) as u8),
                encryption_key: r["key"].as_u64().map(|n| n as u8),
                intercom_unit_id: u(&r["intercom"]) as u32,
            })
            .collect();
        let incoming = Frequency {
            hz: case["hz"].as_f64().unwrap(),
            modulation: Modulation(u(&case["modulation"]) as u8),
            encryption: u(&case["key"]) as u8,
        };
        let blocked = case["blocked"]
            .as_array()
            .unwrap()
            .iter()
            .fold(0, |b, slot| b | (1 << u(slot)));
        let actual = match_radio(
            &radios,
            u(&case["local"]) as u32,
            incoming,
            u(&case["sender"]) as u32,
            case["strict"].as_bool().unwrap(),
            blocked,
        );
        let expected = &case["expected"];
        let mut expected = (!expected.is_null()).then(|| Reception {
            radio: u(&expected["radio"]) as usize,
            secondary: expected["secondary"].as_bool().unwrap(),
            decryptable: expected["decryptable"].as_bool().unwrap(),
        });
        if case["name"] == "guard-fallback-inherited-key" {
            assert!(
                expected.as_ref().unwrap().decryptable,
                "reference bug changed; review correction"
            );
            expected.as_mut().unwrap().decryptable = false;
        }
        assert_eq!(actual, expected, "case {}", case["name"]);
    }
}
#[test]
fn invalid_frequencies_never_match() {
    for invalid in [f64::NAN, f64::INFINITY, -1.0] {
        assert!(!frequency_matches(invalid, invalid));
    }
}
