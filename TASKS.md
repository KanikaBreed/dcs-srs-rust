# Tasks

Status vocabulary: TODO, IN PROGRESS, AWAITING VALIDATION, DONE, BLOCKED. Update the relevant row with evidence and an owner/branch when work starts. The plan is in [SCOPE.md](docs/SCOPE.md).

## Foundation

| ID | Status | Task and acceptance |
|---|---|---|
| F01 | DONE | Adapt Aurora contributor guidance and inventory all 39 tracked Markdown documents; record decisions in docs/AURORA_ADAPTATION.md. |
| F02 | DONE | Create a dependency-free Rust workspace and a common documentation/fmt/clippy/test check command. No application behavior claimed. |
| F03 | DONE | Public repository published; Windows, Linux, and macOS host checks passed for bootstrap commit `86e3745` in [CI run 44012728630](https://github.com/KanikaBreed/dcs-srs-rust/actions/runs/44012728630). These checks cover the scaffold only. |
| F04 | DONE | Verify main requires pull requests and all three host CI checks, including for administrators; force pushes and branch deletion are disabled. Private vulnerability reporting is enabled. |

## First milestone: interoperability and platform feasibility

| ID | Status | Task and acceptance |
|---|---|---|
| P01 | AWAITING VALIDATION | `feat/protocol-foundation`: pin clean upstream `d01f540c`; 12 synthetic fixtures verified against actual C# wire code and all four published UDP vectors. Source attribution and reproducible generator included. Live server/client version validation remains with P03. |
| P02 | DONE | `feat/protocol-foundation`: implement `srs-protocol` UDP voice/keepalive codecs, TCP envelope and bounded incremental framing; independent vectors, malformed input, enum widths, unknown values, frame boundaries, and mutation tests. Voice/control fuzz targets compile; no fuzz campaign or live connection claimed. |
| P03 | TODO | Authenticate External AWACS Mode and exchange radio state through an unmodified local SRS server; test rejected credentials and disabled EAM. |
| A01 | TODO | Send and receive generated Opus audio between a Rust endpoint and stock SRS client; measure loss, jitter, and latency. |
| A02 | TODO | Validate Windows/Linux capture, playback, device removal, and PTT release on an isolated test server. |
| M01 | TODO | Android physical-device spike: service lifetime, microphone permission, locked-screen RX/TX, headset routing, interruptions. |
| M02 | TODO | iPhone physical-device spike: audio session, system PTT, background wake-up, buffering, and gateway requirements. |
| U01 | TODO | Validate Slint with native audio/lifecycle adapters, scaling, accessibility, keyboard, and phone/tablet input. |
| L01 | TODO | Confirm redistributed dependency/asset licenses and feasible iOS distribution before committing to store delivery. |

## TOC alpha and beta

| ID | Status | Task and acceptance |
|---|---|---|
| T01 | TODO | Callsign, saved servers, coalition sign-in, preset import/export, radio tuning/modulation/guard, server policy. |
| T02 | TODO | Multi-radio monitoring, per-radio gain/mute/stereo routing, selected TX radio, prominent PTT and RX/TX state. |
| T03 | TODO | Windows and Linux keyboard/HID PTT; supported Wayland portal integration and permission handling. |
| T04 | TODO | Android TOC UI, user-started background session, device routing, network handover, and supported accessories. |
| T05 | TODO | Reconnect, device changes, packet-loss handling, stale-audio policy, and multi-hour soak validation. |
| T06 | TODO | Platform packages, signing, configuration migration, redacted diagnostics, and accessible UI. |
| T07 | TODO | iOS TOC delivery after M02 and L01; gateway deployment/operations if the validated design needs it. |

## Later scope

| ID | Status | Task and acceptance |
|---|---|---|
| D01 | TODO | DCS Lua interoperability, cockpit PTT/radio sync, aircraft behavior, line of sight, and distance rules. |
| D02 | TODO | Desktop overlays, recording/consent, retransmission, simulcast, and external integrations as separately scoped work. |
| S01 | TODO | Optional compatible Rust server: stock client interop, routing/policy parity, load tests, administration, packaging. |

A row marked DONE satisfies its stated boundary only. F02 does not mean a radio engine exists; desktop CI does not mean mobile applications are validated.
