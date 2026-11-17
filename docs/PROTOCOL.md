# SRS protocol compatibility

## Reference baseline

Initial source review: [DCS-SimpleRadioStandalone at cc07367ac89c62a02297c8f58339dce837d40485](https://github.com/ciribob/DCS-SimpleRadioStandalone/tree/cc07367ac89c62a02297c8f58339dce837d40485), identifying itself as 2.3.8.0. This is the reviewed baseline, not a claim that it is the newest release or that interoperability has been demonstrated. Pin the actual server/client builds in each test report.

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

Preserve SRS behavior where it is needed for interoperability, but do not reproduce unsafe parser behavior. Record intentional differences and the supported-version matrix here when evidence exists. There is no conformance result yet.
