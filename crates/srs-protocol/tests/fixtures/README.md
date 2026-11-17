# Independent upstream wire and radio fixtures

Source: [Ciribob and contributors' DCS-SimpleRadioStandalone at d01f540ce5e363e9ac841775c8f7e2253138cd81](https://github.com/ciribob/DCS-SimpleRadioStandalone/tree/d01f540ce5e363e9ac841775c8f7e2253138cd81), GPLv3. These files contain synthetic inputs only, not captured user traffic or real credentials. See [NOTICE.md](../../../../NOTICE.md).

## Provenance

| Files | Independent source and coverage |
|---|---|
| `single-voice.bin`, `single-relayed.bin`, `multi-voice.bin` | Literal expected arrays in upstream `DCS-SR-CommonTests/Network/UDPVoicePacketTests.cs`; also reproduced by its actual `UDPVoicePacket.EncodePacket`. All four published arrays are checked, including the duplicate multi-frequency decode vector. |
| `distinct-identities.bin` | Actual upstream encoder with different sender/original IDs, unsigned integer maxima, unknown modulation 255, key 255, and zero frequency. |
| `keepalive.bin` | The synthetic 22-byte identity used by upstream tests; upstream UDP handlers use this form for keepalive/endpoint discovery. |
| `ping.jsonl`, `sync.jsonl`, `eam.jsonl`, `radio-update.jsonl`, `version-mismatch.jsonl` | Actual upstream `NetworkMessage.Encode`, JSON resolver, and client/radio models; covers field casing, null omission, enum numbers, 11 radio slots, Unicode, and a deliberately fake EAM password. |
| `message-types.json`, `modulations.json` | Values read from the actual upstream C# enums. |
| `radio-matching.json` | 46 synthetic decisions from unchanged `PlayerRadioInfoBase.CanHearTransmission`; Rust checks them in `srs-core`, with one explicit guard-key correction documented in the receive guide. |

The harness links upstream packet/model code unchanged. Its only non-wire shims are logging and the updater/version provider. The provider uses the same entry-assembly version expression as upstream; the harness assembly pins `2.3.8.0` as a deterministic fixture value. That value is not a claim that the newer reference checkout is release 2.3.8.0. Stock-server control/generated voice tests are documented in [VOICE.md](../../../../docs/VOICE.md); full stock desktop-client voice testing remains pending.

The generator requires the exact clean reference revision and compares generated UDP bytes with the literal upstream test arrays. Rust-generated JSON is compared semantically because member order and equivalent escaping differ across serializers; UDP output must match byte for byte.

## Regenerate or independently check

From the repository root, with .NET SDK 10 and a clean pinned reference checkout:

```sh
python3 scripts/reference_fixtures.py --reference /path/to/DCS-SRS-reference
python3 scripts/reference_fixtures.py --reference /path/to/DCS-SRS-reference --write
```

The first command verifies committed bytes without replacing them. The second deliberately regenerates them. The generator writes build products only in this repository and uses an ignored temporary directory; it does not modify the reference checkout.

To verify Rust encoding through the original C# decoder/serializer:

```sh
cargo run --locked -p srs-protocol --example roundtrip_fixtures -- crates/srs-protocol/tests/fixtures .scratch/rust-wire
python3 scripts/reference_fixtures.py --reference /path/to/DCS-SRS-reference --verify-dir .scratch/rust-wire
```

This verifies 13 fixture files (the radio decision JSON is copied unchanged by the wire example and tested separately by `srs-core`) without opening a network socket or audio device. Linux CI repeats the check from a freshly fetched pinned reference.
