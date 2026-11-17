# Architecture

## Present implementation

`crates/srs-protocol` implements UDP voice packets and identity keepalives, a TCP control envelope, and bounded incremental newline framing. It has independent C# fixtures, malformed-input/regression tests, and fuzz entry points. Nested peer client/radio control payloads are preserved as JSON. Local receive rules live in `srs-core`.

`crates/srs-client` implements a caller-driven Tokio TCP session, version checks, External AWACS Mode authentication, bounded peer/settings snapshots, and an 11-slot standalone radio state. Its stock-server probe verifies control interoperability with SRS 2.4.1.0. `crates/srs-core` implements pure receive-radio eligibility with independent C# cases. `srs-client::voice` adds UDP registration/keepalives, bounded receive, and PTT invalidation tied to the exact TCP connection and radio authorization revision. `srs-audio` wraps Opus encoding/decoding with fixed buffers. Generated audio passes through the stock server and the original C# codec wrappers. `srs-client::receive` applies current radio/key/PTT policy and owns up to 16 original-speaker jitter/decoder states. `srs-audio::jitter` reorders before decoding and bounds latency, queues, and concealment. `srs-device` now provides an explicit CPAL desktop adapter, bounded callback bridges, capture permits, a 48-to-16 kHz FIR, and local tone/capture/codec-loopback probes. `srs-toc` now owns the control/audio session, expiring input ownership, a 40 ms mono/stereo mix and a development console with live radio edits and timed/foreground held-key PTT. Generated end-to-end and null-device console checks are implemented. The user reported basic two-way speech between the Linux desktop and stock Windows SRS on 2026-11-17; detailed device, PTT and interruption validation remains pending. `srs-input` supplies owned desktop keyboard/joystick adapters with release calibration and monotonic/wall-clock freshness. `srs-desktop` adds the first graphical Windows/Linux TOC client; mobile follows desktop validation. See [desktop ownership and validation](docs/DESKTOP.md). See [console contract](docs/TOC_CONSOLE.md). See [device contract](docs/DEVICES.md). See [receive behavior](docs/RECEIVE.md). See [the session contract](docs/SESSION.md) and [voice contract](docs/VOICE.md).

## Boundaries

```mermaid
flowchart TB
    UI[Desktop and mobile UI] --> Session[Client session and radio state]
    Platform[Native lifecycle, device, and PTT adapters] <--> Session
    DCS[Optional DCS Lua adapter] --> Session
    Session <--> Audio[Audio engine and bounded queues]
    Session <--> Protocol[SRS control and voice protocol]
    Audio <--> Protocol
    Protocol <--> Server[Existing SRS server]
```

| Planned component | Responsibility | Exclusions |
|---|---|---|
| `srs-core` (receive matching implemented) | Pure radio/key/intercom eligibility; further domain models planned | UI, native device handles, network I/O |
| `srs-protocol` (implemented wire layer) | Bounded TCP JSON envelope/framing, UDP encoding/decoding, independent fixtures | Typed radio rules, device capture, sockets |
| `srs-client` (TCP/EAM/UDP implemented) | Control state, UDP transport, session-bound PTT and receive gating; reconnect planned | UI framework dependencies, audio devices, automatic playback |
| `srs-audio` (Opus and jitter implemented) | Fixed-frame codec and bounded sequence/jitter/PLC; effects planned | Device handles, UI state mutation, unbounded buffering |
| `srs-device` (desktop probe implemented) | Explicit native input/output, bounded callback queues, capture permits and conversion; offline diagnostics own finite native sessions; mobile permissions/lifecycle adapters pending; session orchestration lives in srs-toc | Duplicated radio/protocol rules |
| `srs-toc` (integrated worker/console) | Owned session/audio orchestration, input leases, mono/stereo mixing, live radio edits and timed/foreground keyboard console | Global/HID PTT, UI, reconnect and mobile lifecycle |
| `srs-desktop` (desktop preview) | Slint setup menus, refreshed devices and hostname connections, saved servers/presets and radio-only transfers, foreground function-key/focused Space/pointer and explicit background PTT, offline diagnostics, per-radio receive volume/mute/routing/RX, read-only session details and bounded opt-in recovery; mobile UI later | Codec and routing decisions in callbacks |
| `srs-input` (desktop adapters) | Windows/X11 selected-key state, Wayland GlobalShortcuts, SDL 3 joystick state; owned bounded worker, no stored enabled/held state | Audio, text input, broad keyboard-device access, mobile input |
| DCS adapter | Existing Lua export/update interfaces, later cockpit support | Rewriting every aircraft script at bootstrap |
| Optional server/gateway | Later compatible Rust relay; separate iOS wake-up service if required | Dependency for the first desktop/Android client |

Add crates when they have a concrete responsibility and implementation. Do not create empty packages for every planned box.

## Candidate technologies

- Rust with Tokio and Serde for asynchronous control/network work.
- libopus through a narrow audited binding for the established voice codec.
- CPAL 0.18.2 for the first desktop device adapter; physical native routing validation remains pending.
- Slint 1.19.0 with winit, software rendering and accessibility for the Windows/Linux desktop preview.
- Kotlin/Java and Swift/Objective-C adapters where native lifecycle APIs require them.

Serde/serde_json and Tokio are used and locked in Cargo.lock. Tokio's enabled features cover TCP/UDP, buffered I/O, bounded latest-value watch channels, timing, and a runtime; the library does not spawn background tasks. Opus uses the safe `opus` binding and bundled libopus 1.6.1 through opusic-sys. The device adapter locks CPAL 0.18.2 and rtrb 0.4.1; allocation-counter 0.8.1 is test-only. Crossterm 0.29.0 supplies the foreground terminal adapter; Slint is pinned to 1.19.0; its generated module is the only desktop-code exception to the unsafe lint. Handwritten modules forbid unsafe. getrandom supplies ephemeral session identities. The desktop profiles worker uses Serde JSON, directories 6.0.0 for user configuration paths and tempfile 3.27.0 for atomic replacement. Its version-4 schema includes the chosen foreground PTT key, but never passwords or held input state. Strict v1/v2/v3 migration supplies missing receive levels/preferences/routing in memory and writes the new schema only on explicit save. A separate version-2 radio-only interchange format (with strict v1 import) validates whole imports before saving and exports to new files. rfd 0.17.2 supplies native choosers on the profile worker. A third joinable worker refreshes device metadata without opening streams; the session worker resolves bounded hostname endpoints. Bounded, sanitized session observations feed the read-only roster/policy screen. Complete radio presets use the same bounded session command path and current-policy validation as single-field edits. Confirm device/UI choices through a physical-device spike. Shared Rust does not remove platform-specific audio-session work. libfuzzer-sys is isolated in a separate development-only fuzz workspace.

## Threading and ownership

The client session owns connection state and applies typed commands/events. UI and platform adapters send commands rather than mutating shared global state. Audio callbacks consume/produce preallocated buffers through bounded queues. Encoding, decoding, network operations, logging, and configuration persistence execute outside hardware callbacks. Receive gain/mute settings travel through latest-value watch channels and are applied by the mono/stereo mix, flushing downstream playback without retuning. Per-radio RX counters represent decoded frames, including muted reception. Stereo mixing decodes each speaker once and accumulates/clips independently per output channel. Mono devices preserve full-level reception of every radio. A bounded three-frame planar stereo queue interleaves only in the allocation-free callback. Define queue limits and drop policy before wiring the audio path.

PTT is an explicit state machine. Disconnect, device failure, lifecycle interruption, and lost input ownership release transmission. Desktop opt-in recovery allows three network retries with 1/2/4 second backoffs, fresh identities/devices and retained acknowledged radio/monitor settings. Credential/policy/protocol/device errors are terminal. Reconnect starts receive-only until a new valid transmit action. Distinguish microphone capture, local TX, server connectivity, and any available delivery evidence.

## Compatibility

The [protocol guide](docs/PROTOCOL.md) pins the initial reference and records known wire details. Use independent upstream fixtures and differential tests. Separate application version, settings schema, and SRS compatibility declaration. Preserve server policy; do not assume all servers permit external coalition operators.

DCS cockpit/terrain state belongs to a later adapter. A phone in TOC mode does not have a local DCS terrain engine. Desktop DCS integration and mobile standalone radio operation have different acceptance criteria.

## Repository layout

- Root Markdown: onboarding, agent rules, architecture, tasks, notices.
- `docs/`: scope, protocol, testing, platform, UI, reporting, and release guides.
- `crates/srs-core/`: pure receive matching and independent C# matcher cases.
- `crates/srs-protocol/`: wire codecs, tests, fixtures, and an offline round-trip example.
- `crates/srs-client/`: TCP/EAM session, UDP voice/PTT gating, local radio state, and stock-server probes.
- `crates/srs-audio/`: fixed-frame Opus, bounded jitter buffer, impairment simulations, and offline codec/load probes.
- `crates/srs-device/`: native device ownership, bounded callback bridges, FIR conversion, local audio probe, and simulated load probe.
- `crates/srs-toc/`: integrated session/audio worker, input ownership, live radio commands, timed/foreground keyboard console and synthetic stock-server probe.
- `crates/srs-input/`: owned native desktop keyboard/joystick adapters and independent freshness gate.
- `crates/srs-desktop/`: desktop UI, localization strings, session/profile worker ownership, bounded configuration persistence and synthetic preview.
- `scripts/test_desktop.py`: private-display/native-null GUI-session integration check.
- `scripts/test_keyboard.py`: simulated terminal key/focus/stall/restore checks inside the stock-server namespace.
- `scripts/test_toc_console.py`: actual console/native null-PCM check inside the stock-server namespace.
- `scripts/test_null_audio.py`: process-local ALSA null device verification without hardware.
- `tools/voice-reference/`: unchanged upstream C# codec wrappers linked into a generated-signal harness.
- `scripts/test_reference_codec.py`: bidirectional Rust/C# codec verification.
- `scripts/test_stock_server.py`: checksum-pinned official server verification in an isolated Linux network.
- `tools/reference-fixtures/`: .NET harness linking unchanged external upstream sources.
- `scripts/reference_fixtures.py`: pinned-source and independent-fixture verification.
- `fuzz/`: isolated voice/control libFuzzer targets.
- `scripts/check.py`: standard-library Python driver for documentation and Cargo checks.
- `.github/`: CI and issue/PR templates.

Keep this inventory synchronized when actual modules are added.

Desktop background input, diagnostics, suspension policy and native test boundaries are described in [BACKGROUND_PTT.md](docs/BACKGROUND_PTT.md) and [VOICE_VALIDATION.md](docs/VOICE_VALIDATION.md). The audio worker stops after a 250 ms wall-clock discontinuity; native error/interruption is terminal and does not trigger network retry.
