# DCS SRS Rust

A Rust client project for [DCS SimpleRadio Standalone](https://github.com/ciribob/DCS-SimpleRadioStandalone), targeting Windows, Linux, Android, and iOS.

The first product is a standalone **TOC (tactical operations center) radio console**: monitor several radio nets and transmit with push-to-talk through an existing SRS server, without running DCS on the operator's device.

**Status: generated voice and receive pipeline.** Rust connects to SRS, authenticates coalition access, exchanges radio state, and sends/receives generated Opus audio through the unmodified SRS 2.4.1.0 server. UDP keepalives and session-bound PTT gating are implemented. Original C# codec wrappers verify Opus in both directions. Radio/key eligibility, bounded packet ordering, jitter buffering, and loss concealment are implemented and tested with generated audio. Microphone/speaker I/O, hardware PTT, UI, and mobile applications remain pending; this is not yet an installable voice app.

## Product direction

- One shared Rust engine for SRS protocol, radio state, voice processing, and reconnect behavior.
- Windows and Linux desktop clients with keyboard and hardware PTT.
- Android phones and tablets with full voice, touch PTT, and background listening.
- iPhone and iPad clients, with background reception and system PTT validated as a separate milestone.
- Existing SRS servers first. A compatible Rust server is a later, optional deliverable.
- DCS cockpit synchronization and the broader desktop feature set follow the standalone TOC release.

Standalone coalition operation uses SRS External AWACS Mode. Server administrators must enable it and supply the appropriate coalition credentials. SRS simulated radio encryption is not end-to-end transport encryption.

## Development

Install [Rust through rustup](https://rustup.rs), a native toolchain with a C compiler and CMake for your host, and Python 3.11 or newer. The Opus dependency builds its bundled native library. The checked-in toolchain file selects stable Rust with rustfmt and clippy.

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
