# DCS SRS Rust

A Rust client project for [DCS SimpleRadio Standalone](https://github.com/ciribob/DCS-SimpleRadioStandalone), targeting Windows, Linux, Android, and iOS.

The first product is a standalone **TOC (tactical operations center) radio console**: monitor several radio nets and transmit with push-to-talk through an existing SRS server, without running DCS on the operator's device.

**Status: project foundation.** This repository contains the development guides, delivery plan, CI configuration, and a dependency-free Rust workspace scaffold. It does not yet connect to SRS, capture audio, provide a UI, or contain installable applications. A successful scaffold build is not a platform or interoperability certification.

## Product direction

- One shared Rust engine for SRS protocol, radio state, voice processing, and reconnect behavior.
- Windows and Linux desktop clients with keyboard and hardware PTT.
- Android phones and tablets with full voice, touch PTT, and background listening.
- iPhone and iPad clients, with background reception and system PTT validated as a separate milestone.
- Existing SRS servers first. A compatible Rust server is a later, optional deliverable.
- DCS cockpit synchronization and the broader desktop feature set follow the standalone TOC release.

Standalone coalition operation uses SRS External AWACS Mode. Server administrators must enable it and supply the appropriate coalition credentials. SRS simulated radio encryption is not end-to-end transport encryption.

## Development

Install [Rust through rustup](https://rustup.rs), a native toolchain for your host, and Python 3.11 or newer. The checked-in toolchain file selects stable Rust with rustfmt and clippy.

```sh
git clone https://github.com/KanikaBreed/dcs-srs-rust.git
cd dcs-srs-rust
python3 scripts/check.py
```

On Windows, use `py -3 scripts/check.py`. The script checks local Markdown file links, formatting, clippy, and workspace tests. The initial crate intentionally has no runtime implementation or behavioral tests. No network, microphone, or audio hardware is required by these checks.

The same Cargo checks can be run directly:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

## Project guides

- [Scope and milestones](docs/SCOPE.md)
- [Architecture](ARCHITECTURE.md)
- [Current tasks](TASKS.md)
- [Agent instructions](AGENTS.md) and [human contributor guide](HUMANS.md)
- [Protocol compatibility](docs/PROTOCOL.md)
- [Testing and acceptance](docs/TESTING.md)
- [Platform work](docs/PLATFORMS.md)
- [Interface design](docs/BRANDING.md) and [translation policy](docs/TRANSLATING.md)
- [Bug reports](docs/BUG_REPORTING.md), [security](SECURITY.md), and [release policy](docs/RELEASES.md)
- [Aurora documentation adaptation record](docs/AURORA_ADAPTATION.md)

## License and attribution

GPL-3.0-only; see [LICENSE](LICENSE) and [NOTICE.md](NOTICE.md). SRS is the compatibility reference. The contributor workflow is adapted from Aurora Viewer with attribution. No upstream application code, audio assets, artwork, or credentials are bundled in this bootstrap.

This is an independent project, not an official SRS or Eagle Dynamics release.
