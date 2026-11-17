# SRS protocol compatibility

## Reference baseline

The implementation and independent fixture oracle pin [DCS-SimpleRadioStandalone at d01f540ce5e363e9ac841775c8f7e2253138cd81](https://github.com/ciribob/DCS-SimpleRadioStandalone/tree/d01f540ce5e363e9ac841775c8f7e2253138cd81), fetched from upstream master for this first implementation. The earlier scope reviewed `cc07367a` (2.3.8.0); the voice-packet layout is unchanged. The new source derives its version from the entry assembly. The fixture harness explicitly supplies `2.3.8.0` for reproducible JSON, not as a claim about the newer source release. A live server/client version remains to be validated in P03.

Create a separate clean reference checkout, preserving any existing SRS modifications:

```sh
git init ../DCS-SRS-reference
git -C ../DCS-SRS-reference remote add origin https://github.com/ciribob/DCS-SimpleRadioStandalone.git
git -C ../DCS-SRS-reference sparse-checkout set Common DCS-SR-CommonTests
git -C ../DCS-SRS-reference fetch --depth 1 --filter=blob:none origin d01f540ce5e363e9ac841775c8f7e2253138cd81
git -C ../DCS-SRS-reference checkout --detach FETCH_HEAD
```

Use a new directory for these initialization commands. Source and fixture details are in the [corpus guide](../crates/srs-protocol/tests/fixtures/README.md).

Useful upstream files:

- `Common/Models/NetworkMessage.cs`: TCP message model and encoding.
- `Common/Models/UDPVoicePacket.cs`: binary voice packet layout.
- `Common/Network/Client/TCPClientHandler.cs`: session/version/EAM behavior.
- `Common/Network/Client/UDPVoiceHandler.cs`: voice socket and keepalive.
- `Common/Network/Server/ServerSync.cs`: coalition/EAM authentication.
- `Common/Network/Server/UDPVoiceRouter.cs`: routing and eligibility.
- `Common/Models/Player/PlayerRadioInfoBase.cs`: radio matching.
- `Common/Constants.cs`: radio count and audio configuration.
- `DCS-SR-CommonTests/Network/UDPVoicePacketTests.cs`: independent byte fixtures.
- `DCS-SR-Client/Audio/Managers/UDPClientAudioProcessor.cs`: receive/transmit behavior.
- `DCS-SR-Client/Network/DCS/DCSRadioSyncHandler.cs`: external mode and cockpit updates.
- `Scripts/DCS-SRS/Scripts/`: Lua exports and aircraft-specific behavior.

## Observed wire format

TCP control messages are JSON terminated by a newline. Message types are numeric enum values. Fields include client state, server settings, compatibility version, and External AWACS Mode authentication. Preserve wire names, field/default behavior, units, and enum numbers; internal Rust naming may differ.

UDP voice packets use a custom format, not RTP:

| Segment | Contents |
|---|---|
| Header | Three 16-bit lengths: complete packet, audio payload, frequency segment |
| Audio | Opus bytes |
| Frequency entries | 64-bit floating frequency, 8-bit modulation, 8-bit simulated encryption key per entry |
| Fixed tail | 32-bit unit ID, 64-bit sequence, 8-bit relay count, 22-byte original identity, 22-byte sender identity |

The reviewed C# implementation uses `BitConverter`; explicitly reproduce the little-endian representation used by the reference builds, independent of host architecture. Reject inconsistent lengths and invalid array relationships before indexing or allocating. Distinguish the 22-byte UDP identity keepalive from audio packets.

The reference constants use 16 kHz mono microphone input, 40 ms input frames, 48 kHz output, and 11 radio slots. Match this baseline initially. Codec behavior, jitter buffering, effects, and packet timing must be verified against actual SRS audio.

## External AWACS Mode

A server enables EAM and configures coalition passwords. A connection alone does not establish coalition authorization. Reproduce success/failure and disabled-mode behavior, radio presets, identity, and server policy. EAM authentication is separate from any future application account system.

Radio encryption fields simulate radio key matching. They do not provide cryptographic transport confidentiality. Do not describe this mode as secure operational communications.

## Conformance plan

1. Attribute and import independent upstream byte fixtures.
2. Test encode/decode against those bytes, including multiple frequencies and relays.
3. Test TCP fragmentation/coalescing, bounded frames, unknown optional fields, and version handling.
4. Test malformed/truncated packets, overflowing lengths, invalid numeric data, and bounded resource use.
5. Run differential behavior against pinned stock SRS clients/servers using synthetic identities/audio.
6. Verify mixed-client coalition, guard, modulation, key, and radio eligibility behavior.
7. Add fuzzing and network impairment scenarios before public beta.

## Implemented checks and deliberate limits

- `srs-protocol` passes independent upstream UDP vectors and C#-serialized control fixtures. Rust outputs are accepted by the original C# model/decoder harness; UDP bytes match exactly and JSON values match semantically. This is offline wire evidence, not a connected-session result.
- UDP lengths are checked before slicing/allocation; audio is borrowed. Frequency entries are bounded by the 16-bit packet size, not artificially capped to the UI radio count. Unknown modulation bytes and numeric control-message types are preserved.
- Identities must have the standard 22-character URL-safe ASCII shape. Invalid lengths/identity characters and negative/non-finite frequencies are rejected, even if the permissive C# decoder would accept them. Empty audio/frequency segments are structurally allowed; a future session layer decides whether to use them.
- TCP emits one message per decoder call. It enforces a local 1 MiB frame limit (excluding LF), accepts CRLF, bounds serialized output, and fails closed after malformed or oversized frames. EOF with a partial frame is an error. This size cap is a Rust resource policy, not a limit promised by SRS; very large SYNC messages need evaluation.
- MsgType is required and numeric. Unlike C# default initialization, an absent type is not silently treated as UPDATE. Null optional fields are omitted on re-encoding; unknown fields are retained. JSON recursion limits remain enabled.
- Client/radio payloads are generic JSON objects in this increment. Authentication, version negotiation, domain validation, timing behavior, and Opus/audio processing remain future work.
- Fuzz targets compile; no coverage-guided fuzz campaign is claimed. See [fuzz instructions](../fuzz/README.md).
