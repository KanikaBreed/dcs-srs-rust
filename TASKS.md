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
| P01 | DONE | Pin clean source `d01f540c` and 12 C# fixtures/all four published UDP vectors; connected control baseline now verified with the official SRS 2.4.1.0 server and recorded SHA-256. Stock-client voice validation remains explicitly in A01. |
| P02 | DONE | `feat/protocol-foundation`: implement `srs-protocol` UDP voice/keepalive codecs, TCP envelope and bounded incremental framing; independent vectors, malformed input, enum widths, unknown values, frame boundaries, and mutation tests. Voice/control fuzz targets compile; no fuzz campaign or live connection claimed. |
| P03 | DONE | `feat/eam-session`: `srs-client` connects/authenticates and exchanges radio state through unmodified SRS 2.4.1.0. Isolated stock-server probes pass Blue/Red login, rejected credentials, disabled EAM, two-way radio updates, guard/key/modulation, leave/reset, peer disconnect, and version rejection. Loopback tests cover cancellation, revocation, framing, periodic refreshes, and resource limits. See docs/SESSION.md; voice/PTT/mobile remain pending. |
| A01 | AWAITING VALIDATION | `feat/voice-transport`: UDP keepalives/registration retry, TCP-bound PTT gate, and fixed-frame Opus implemented. Stock server passes 25 generated frames each direction, coalition/tuning isolation, guard routing, and timing/loss measurements. Original C# wrappers exchange 20 codec frames each direction. Still required: full stock desktop-client voice exchange, receive eligibility/jitter buffering, impairment/sequence tests, and playout measurements. See docs/VOICE.md. |
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
