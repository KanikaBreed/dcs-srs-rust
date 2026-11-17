# Notices

Copyright (C) 2026 DCS SRS Rust contributors.

This project is distributed under the GNU General Public License version 3 only (GPL-3.0-only). See [LICENSE](LICENSE). It is provided without warranty as described there.

## SRS compatibility reference

[DCS SimpleRadio Standalone](https://github.com/ciribob/DCS-SimpleRadioStandalone) by Ciribob and contributors is the upstream protocol and behavior reference. The initial reviewed revision is recorded in [docs/PROTOCOL.md](docs/PROTOCOL.md). Upstream carries GPLv3 licensing. `crates/srs-protocol/src/voice.rs` ports its packet layout and numeric modulation values; `control.rs` ports its control envelope and numeric message types. The Rust implementation adds explicit bounds/error handling and preserves unknown numeric values. The [fixture corpus](crates/srs-protocol/tests/fixtures/README.md) includes upstream test-vector bytes and synthetic output from its actual encoder/serializer.

Source revision: `d01f540ce5e363e9ac841775c8f7e2253138cd81`. Relevant source files are `Common/Models/UDPVoicePacket.cs`, `Common/Models/NetworkMessage.cs`, `Common/Models/Player/Modulation.cs`, and `DCS-SR-CommonTests/Network/UDPVoicePacketTests.cs`, copyright Ciribob and contributors. The independent .NET harness links upstream client/radio models and JSON helpers from an external checkout unchanged; the exact file list is in `tools/reference-fixtures/ReferenceFixtures.csproj`. No Lua scripts, sounds, artwork, or upstream application binaries are bundled.

For future source-derived additions, retain original copyright/license notices and record exact files/revisions and modifications here and near the code. Review separately licensed assets before copying them.

## Aurora workflow adaptation

The contributor guides, task workflow, and review/testing principles are adapted from [Aurora Viewer](https://github.com/Aurora-Viewer/Aurora-Viewer), copyright (C) 2026 Aurora Viewer contributors, whose notices license that material under GPL-3.0-or-later. This adaptation is distributed here under GPL-3.0-only, an allowed choice of version 3. It rewrites viewer-specific requirements for SRS compatibility, real-time audio, standalone TOC operation, and mobile lifecycle work.

The reviewed source revision and file-by-file disposition are in [docs/AURORA_ADAPTATION.md](docs/AURORA_ADAPTATION.md). Aurora application code, logos, art, fonts, models, animations, and third-party assets are not bundled. This project is not an Aurora Viewer build.

## Dependencies and names

The wire crate uses Serde/serde_json and their locked Rust dependencies. Serde, serde_json, serde_core, serde_derive, itoa, proc-macro2, quote, and syn are MIT OR Apache-2.0; memchr is Unlicense OR MIT; zmij is MIT; unicode-ident is (MIT OR Apache-2.0) AND Unicode-3.0. See Cargo.lock and the corresponding crate distributions for versions and complete license/copyright texts.

The isolated fuzz workspace additionally uses libfuzzer-sys under (MIT OR Apache-2.0) AND NCSA; its runtime is a development tool, not an application dependency. Other locked fuzz/build dependencies are MIT/Apache-compatible, with r-efi offering MIT OR Apache-2.0 OR LGPL-2.1-or-later. See fuzz/Cargo.lock and dependency distributions for complete notices. Preserve these texts with future binary distributions. Tokio, codec/device bindings, and UI libraries remain design candidates and are not bundled.

DCS, SRS, and Aurora names identify references and interoperability targets. This project is independent and does not claim endorsement by their maintainers or by Eagle Dynamics. Copyright licensing does not grant third-party trademark rights.
