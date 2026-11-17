# DCS SRS Rust

A Rust client project for [DCS SimpleRadio Standalone](https://github.com/ciribob/DCS-SimpleRadioStandalone), targeting Windows, Linux, Android, and iOS.

The first product is a standalone **TOC (tactical operations center) radio console**: monitor several radio nets and transmit with push-to-talk through an existing SRS server, without running DCS on the operator's device.

**Status: desktop client preview.** A [graphical Windows/Linux TOC client](docs/DESKTOP.md) now wraps the Rust voice engine with connection/device selection, ten radio slots, saved servers/radio presets, offline tuning, radio names/guard/key controls, per-radio receive volume/mute/RX, foreground F8 and on-screen hold-to-talk. The shared engine exchanges generated Opus audio through stock SRS 2.4.1.0 and is checked against independent upstream fixtures and C# codec wrappers. Automated GUI/session checks use a private server/display and native null audio; physical headsets, listening, accessibility and actual input devices still need validation. Run `cargo run --locked -p srs-desktop` to open the setup window. This is a source-built preview, not an installer.

**Delivery order: Windows/Linux desktop first, then mobile.** The [development console](docs/TOC_CONSOLE.md) remains available for diagnostics.

## Product direction

- One shared Rust engine for SRS protocol, radio state, voice processing, and reconnect behavior.
- Windows and Linux desktop clients with keyboard and hardware PTT.
- Android phones and tablets with full voice, touch PTT, and background listening.
- iPhone and iPad clients, with background reception and system PTT validated as a separate milestone.
- Existing SRS servers first. A compatible Rust server is a later, optional deliverable.
- DCS cockpit synchronization and the broader desktop feature set follow the standalone TOC release.

Standalone coalition operation uses SRS External AWACS Mode. Server administrators must enable it and supply the appropriate coalition credentials. SRS simulated radio encryption is not end-to-end transport encryption.

## Development

Install [Rust through rustup](https://rustup.rs), a native toolchain with a C compiler and CMake for your host, and Python 3.11 or newer. The Opus dependency builds its bundled native library. Linux also needs ALSA development headers and pkg-config (`libasound2-dev pkg-config libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev libx11-xcb-dev libx11-dev libxcursor-dev xinput` on Debian/Ubuntu). The checked-in toolchain file selects stable Rust with rustfmt and clippy.

```sh
git clone https://github.com/KanikaBreed/dcs-srs-rust.git
cd dcs-srs-rust
python3 scripts/check.py
```

On Windows, use `py -3 scripts/check.py`. The script checks local Markdown file links, workspace/fuzz formatting, clippy, and protocol/session tests. Tests cover upstream vectors, loopback TCP sessions, authentication, cancellation, policy changes, UDP keepalives/PTT, codecs, and resource bounds. No installed SRS server, microphone, or audio hardware is required by these checks; Cargo may download dependencies on the first build.

The same Cargo checks can be run directly:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

## Try the protocol implementation

Export Rust-encoded fixtures without connecting to a server:

```sh
cargo run --locked -p srs-protocol --example roundtrip_fixtures -- crates/srs-protocol/tests/fixtures .scratch/rust-wire
```

The [fixture guide](crates/srs-protocol/tests/fixtures/README.md) explains how to verify those outputs with the original C# code. Linux CI performs that independent check against a pinned upstream checkout. [Fuzz targets](fuzz/README.md) are also available.

On Linux, run the actual-server conformance test:

```sh
python3 scripts/test_stock_server.py
```

It downloads the pinned official server to `.scratch`, checks its SHA-256, builds a Rust probe, and runs control and generated-voice scenarios in a private network namespace. Requires `unshare`, `ip`, and permission to create a user/network namespace. No microphone is opened. See [session usage](docs/SESSION.md) and [voice/codec behavior and limits](docs/VOICE.md).

## Try local audio

The [device guide](docs/DEVICES.md) describes intentional tone, capture, and headphone-loopback probes. Start with help; it opens no devices:

```sh
cargo run --locked -p srs-device --bin audio_probe -- --help
```

The probe does not connect to SRS or record audio. Physical device validation is pending.

## Project guides

- [Scope and milestones](docs/SCOPE.md)
- [Architecture](ARCHITECTURE.md)
- [Current tasks](TASKS.md)
- [Agent instructions](AGENTS.md) and [human contributor guide](HUMANS.md)
- [Protocol compatibility](docs/PROTOCOL.md) and [receive pipeline](docs/RECEIVE.md)
- [Testing and acceptance](docs/TESTING.md)
- [Platform work](docs/PLATFORMS.md)
- [Interface design](docs/BRANDING.md) and [translation policy](docs/TRANSLATING.md)
- [Bug reports](docs/BUG_REPORTING.md), [security](SECURITY.md), and [release policy](docs/RELEASES.md)
- [Aurora documentation adaptation record](docs/AURORA_ADAPTATION.md)

## License and attribution

GPL-3.0-only; see [LICENSE](LICENSE) and [NOTICE.md](NOTICE.md). SRS is the compatibility reference. The contributor workflow is adapted from Aurora Viewer with attribution. The protocol layout/model port and synthetic fixtures are attributed in the notices. Upstream application binaries, audio assets, artwork, and credentials are not bundled.

This is an independent project, not an official SRS or Eagle Dynamics release.
