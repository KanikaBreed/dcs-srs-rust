# Notices

Copyright (C) 2026 DCS SRS Rust contributors.

This project is distributed under the GNU General Public License version 3 only (GPL-3.0-only). See [LICENSE](LICENSE). It is provided without warranty as described there.

## SRS compatibility reference

[DCS SimpleRadio Standalone](https://github.com/ciribob/DCS-SimpleRadioStandalone) by Ciribob and contributors is the upstream protocol and behavior reference. The initial reviewed revision is recorded in [docs/PROTOCOL.md](docs/PROTOCOL.md). Upstream carries GPLv3 licensing. This bootstrap includes no translated upstream application code, protocol fixture bytes, Lua scripts, sounds, or artwork.

For future source-derived additions, retain original copyright/license notices and record exact files/revisions and modifications here and near the code. Review separately licensed assets before copying them.

## Aurora workflow adaptation

The contributor guides, task workflow, and review/testing principles are adapted from [Aurora Viewer](https://github.com/Aurora-Viewer/Aurora-Viewer), copyright (C) 2026 Aurora Viewer contributors, whose notices license that material under GPL-3.0-or-later. This adaptation is distributed here under GPL-3.0-only, an allowed choice of version 3. It rewrites viewer-specific requirements for SRS compatibility, real-time audio, standalone TOC operation, and mobile lifecycle work.

The reviewed source revision and file-by-file disposition are in [docs/AURORA_ADAPTATION.md](docs/AURORA_ADAPTATION.md). Aurora application code, logos, art, fonts, models, animations, and third-party assets are not bundled. This project is not an Aurora Viewer build.

## Dependencies and names

The initial Rust crate has no third-party dependencies. Candidate libraries are design proposals, not bundled components. Add native/library and asset notices when those dependencies are introduced.

DCS, SRS, and Aurora names identify references and interoperability targets. This project is independent and does not claim endorsement by their maintainers or by Eagle Dynamics. Copyright licensing does not grant third-party trademark rights.
