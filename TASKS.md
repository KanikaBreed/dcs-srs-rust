# Tasks

Status vocabulary: TODO, IN PROGRESS, AWAITING VALIDATION, DONE, BLOCKED. Update the relevant row with evidence and an owner/branch when work starts. The plan is in [SCOPE.md](docs/SCOPE.md).

## Foundation

| ID | Status | Task and acceptance |
|---|---|---|
| F01 | DONE | Adapt Aurora contributor guidance and inventory all 39 tracked Markdown documents; record decisions in docs/AURORA_ADAPTATION.md. |
| F02 | DONE | Create a dependency-free Rust workspace and a common documentation/fmt/clippy/test check command. No application behavior claimed. |
| F03 | DONE | Public repository published; Windows, Linux, and macOS host checks passed for bootstrap commit `86e3745` in [CI run 44012728630](https://github.com/KanikaBreed/dcs-srs-rust/actions/runs/44012728630). These checks cover the scaffold only. |
| F04 | DONE | Verify main requires pull requests and all three host CI checks, including for administrators; force pushes and branch deletion are disabled. Private vulnerability reporting is enabled. |

## First milestone: Windows/Linux desktop interoperability and usability

Desktop delivery precedes mobile. Android/iOS application and lifecycle work is deferred until the desktop milestone is validated.

| ID | Status | Task and acceptance |
|---|---|---|
| P01 | DONE | Pin clean source `d01f540c` and 13 C# fixture files (including 46 receive cases)/all four published UDP vectors; connected control baseline now verified with the official SRS 2.4.1.0 server and recorded SHA-256. Stock-client voice validation remains explicitly in A01. |
| P02 | DONE | `feat/protocol-foundation`: implement `srs-protocol` UDP voice/keepalive codecs, TCP envelope and bounded incremental framing; independent vectors, malformed input, enum widths, unknown values, frame boundaries, and mutation tests. Voice/control fuzz targets compile; no fuzz campaign or live connection claimed. |
| P03 | DONE | `feat/eam-session`: `srs-client` connects/authenticates and exchanges radio state through unmodified SRS 2.4.1.0. Isolated stock-server probes pass Blue/Red login, rejected credentials, disabled EAM, two-way radio updates, guard/key/modulation, leave/reset, peer disconnect, and version rejection. Loopback tests cover cancellation, revocation, framing, periodic refreshes, and resource limits. See docs/SESSION.md; voice/PTT/mobile remain pending. |
| A01 | AWAITING VALIDATION | `feat/receive-pipeline`: UDP/PTT/Opus plus receive radio/key/intercom/guard policy, bounded ordering/jitter/PLC, and session-revoked playout implemented. Stock server passes 25 generated frames each direction through playout, coalition/tuning isolation, guard and wrong-key checks. 46 independent C# receive cases, deterministic impairments, and 16-speaker load probe added. Original C# codec wrappers exchange 20 frames each direction. User reported basic two-way speech and Windows-to-Linux frequency isolation (251/252 MHz AM mismatch silent; matching 251 MHz restores speech) with stock Windows SRS 2.4.1.0 on 2026-11-17; build identity and limits are in docs/VOICE_VALIDATION.md. Detailed listening quality, broader radio/effects/mixing behavior and WAN/skew/soak remain pending. See docs/VOICE.md and docs/RECEIVE.md. |
| A02 | IN PROGRESS | `feat/radio-controls`: desktop adapter now connects to an owned SRS session through an expiring PTT controller, bounded mono/stereo mix, live radio edits and a timed/foreground-keyboard console. Generated bridge-to-stock-server and actual console/native-null checks pass locally, including release/expiry/policy/disconnect/cancellation. Basic Linux Rust-to-stock Windows SRS speech was reported working both ways on 2026-11-17. Still required: Rust-on-Windows audio, headset/removal and detailed listening checks, physical foreground/background-input validation and real WAN/multi-hour validation. See docs/TOC_CONSOLE.md and docs/DEVICES.md. |
| M01 | TODO | Android physical-device spike: service lifetime, microphone permission, locked-screen RX/TX, headset routing, interruptions. |
| M02 | TODO | iPhone physical-device spike: audio session, system PTT, background wake-up, buffering, and gateway requirements. |
| U01 | IN PROGRESS | `feat/desktop-client`: Slint desktop window, ten-slot radio editor, foreground F8/pointer PTT, device/connection controls and authoritative radio snapshots implemented. Private-display/null-audio stock-server integration and synthetic RX/TX inspection added. Basic two-way speech on Linux/Wayland with stock Windows SRS was reported working on 2026-11-17. Rust-on-Windows audio, detailed device/input/Wayland behavior, screen readers and broader scaling remain pending; phone/tablet work is deferred. See docs/DESKTOP.md. |
| L01 | TODO | Confirm redistributed dependency/asset licenses and feasible iOS distribution before committing to store delivery. |

## TOC alpha and beta

| ID | Status | Task and acceptance |
|---|---|---|
| T01 | IN PROGRESS | `feat/radio-controls`: bounded live tune/modulation/guard/key/off commands implemented with current server policy and partial-TCP preservation. Desktop connection/tuning/guard/key UI now implemented in `feat/desktop-client`; `feat/saved-profiles`: named saved servers (address/callsign/device IDs, no passwords), offline radio preparation and named ten-radio presets implemented. Bounded version-1 persistence, conflict/corrupt-file protection and atomic live preset updates have unit and private stock-server GUI checks; `feat/desktop-menus` adds native radio-only import/export, hostname endpoints and read-only server information; native chooser validation remains pending. Original scope: Callsign, saved servers, coalition sign-in, preset import/export, radio tuning/modulation/guard, server policy. |
| T02 | IN PROGRESS | `feat/radio-controls`: selected TX radio and multi-radio receive eligibility in the console implemented; desktop selected-radio/PTT/mixed-RX UI now implemented in `feat/desktop-client`; `feat/radio-monitor`: local 0–100% receive gain/mute, per-radio decoded-RX indicators and strict v1-to-v2 preset migration implemented. Synthetic gain/deduplication/mute-flush tests and private GUI persistence checks added; `feat/stereo-routing` adds Left/Right/Both, full-level mono fallback and schema-v4 persistence. User reported Linux radio 1 mute stops stock Windows speech while RX stays active, and unmute restores speech, on 2026-11-17; see docs/VOICE_VALIDATION.md. Stereo/mono listening, receive volume, multiple speakers, continuous balance and selected/volume receive-priority scoring remain pending. Original scope: Multi-radio monitoring, per-radio gain/mute/stereo routing, selected TX radio, prominent PTT and RX/TX state. |
| T03 | IN PROGRESS | `feat/radio-controls`: opt-in foreground F8 PTT with release calibration, focus/stall invalidation and hold cap; synthetic terminal/native-null checks pass. `feat/desktop-menus` adds configurable foreground F1–F12/mouse-only binding and saved preferences. Desktop Windows/X11 keyboard, Wayland portal and SDL joystick-button adapters are implemented under U11; physical validation and broader bindings remain pending. Original scope: Windows and Linux keyboard/HID PTT; supported Wayland portal integration and permission handling. |
| T04 | TODO | Android TOC UI, user-started background session, device routing, network handover, and supported accessories. |
| T05 | IN PROGRESS | `feat/desktop-menus`: opt-in bounded network recovery and offline device refresh implemented; forced TCP-loss/private stock-server recovery and synthetic retry/cancel tests pass. Native device failures, real network handover and multi-hour soak remain pending. Packet-loss/stale-audio behavior is covered by the existing bounded receive engine. |
| T06 | IN PROGRESS | Strict v1/v2/v3-to-v4 profile migration, bounded/redacted session display and local build manifests/checklist implemented. Packages, signing, full redistribution notices and native accessibility validation remain pending. |
| T07 | TODO | iOS TOC delivery after M02 and L01; gateway deployment/operations if the validated design needs it. |

## Later scope

| ID | Status | Task and acceptance |
|---|---|---|
| D01 | TODO | DCS Lua interoperability, cockpit PTT/radio sync, aircraft behavior, line of sight, and distance rules. |
| D02 | TODO | Desktop overlays, recording/consent, retransmission, simulcast, and external integrations as separately scoped work. |
| S01 | TODO | Optional compatible Rust server: stock client interop, routing/policy parity, load tests, administration, packaging. |

A row marked DONE satisfies its stated boundary only. F02 does not mean a radio engine exists; desktop CI does not mean mobile applications are validated.

## Desktop menu completion before human validation

The source-based screen inventory is [DESKTOP_PARITY.md](docs/DESKTOP_PARITY.md). This work expands the standalone desktop client; DCS-only controls do not become placeholder buttons.

| ID | Status | Task and acceptance |
|---|---|---|
| U02 | AWAITING VALIDATION | `feat/desktop-menus`: six working pages with persistent connection/PTT, selected TX radio and disconnect. Synthetic render/navigation checks pass; native Windows/Wayland navigation, screen readers and broader scaling pending. |
| U03 | AWAITING VALIDATION | Owned metadata worker refreshes devices offline, preserves IDs and unavailable choices, and never opens streams. ID/null-device refresh checks pass; physical removal/reappearance and backend stalls pending. |
| U04 | DONE | Explicit hostname/IPv4/bracketed IPv6 parsing, worker DNS result deadline and at most four addresses. Parser, localhost and private stock hostname connection checks pass. OS resolver cancellation can delay runtime shutdown; documented, not claimed as a hard deadline. |
| U05 | AWAITING VALIDATION | F1–F12/mouse-only foreground PTT, saved settings, release on rebinding/menu/modifier/focus changes and repeat filtering implemented. Synthetic key and private GUI persistence/release checks pass; physical keys and compositor interaction pending; see U11 for background adapters. |
| U06 | AWAITING VALIDATION | Sanitized 256-name client display, 15 allowlisted read-only policies, version/coalition, health counters and built-in help implemented. Scripted roster/privacy and private stock checks pass; native accessibility and long translations pending. |
| U07 | AWAITING VALIDATION | Native chooser plus bounded radio-only import/export implemented. Complete validation, conflict/corrupt-file/no-overwrite and exclusion checks pass; imported presets never auto-apply. Native portal/Windows/macOS chooser interaction pending. |
| U08 | AWAITING VALIDATION | Opt-in three-retry network recovery (1/2/4 seconds), fresh identity/devices, retained acknowledged radios/monitor levels and receive-only restart implemented. Forced TCP loss during TX through private stock relay, no resumed TX, fresh press, finite budget, credential rejection and cancellation tests pass. On 2026-11-17 the user reported manual Linux disconnect/reconnect starts with TX off, requires a fresh press and restores two-way speech with stock Windows SRS. Human forced-loss/automatic-retry, device interruption and soak checks remain pending. |
| U09 | AWAITING VALIDATION | `feat/stereo-routing`: Left/Right/Both radio routing, full-level mono fallback, bounded atomic stereo output and schema-v4 profiles/version-2 transfers implemented. Deterministic gain/isolation/clipping, callback interleave/flush/allocation, migration and mono/stereo worker checks pass; private stock probe and GUI persistence/recovery checks added. Physical stereo/mono listening remains pending. |
| U10 | DONE | Local validation build helper and hash verification exercised; native build preparation is included in all three CI jobs; source/toolchain/profile manifest and concrete human checklist prepared in docs/DESKTOP_VALIDATION.md. It preserves existing output directories and labels dirty builds. No release, installer, bit-for-bit reproducibility or signing claim. |

## Desktop input, voice validation, resilience and usability

| ID | Status | Task and acceptance |
|---|---|---|
| U11 | AWAITING VALIDATION | `feat/desktop-input`: explicit Windows/X11/Wayland background keyboard and SDL joystick-button PTT, owned bounded worker, fresh-release calibration, state freshness and schema-v5 preferences implemented. Virtual joystick removal, private XTest/portal denial/cancellation/loss and hold/stall tests pass locally. Physical Windows/Linux keys/HOTAS, real portal/lock/elevated-app behavior and wider bindings remain pending. See docs/BACKGROUND_PTT.md. |
| U12 | AWAITING VALIDATION | Offline microphone level, Opus loopback and alternating stereo diagnostics implemented with persistent Stop, 15-second cap, session exclusion and owned stream shutdown. Native null cancellation/stall/deadline checks pass; two-way generated stock-server coverage retained. Basic two-way speech and PTT release at both endpoints were reported working between the Linux Rust client and stock Windows SRS on 2026-11-17. Detailed listening quality, headset identification, offline diagnostics and stereo/removal remain pending; concrete matrix and build identity in docs/VOICE_VALIDATION.md. |
| U13 | AWAITING VALIDATION | Worker/UI/input wall-clock interruption guards, guarded pending sends, distinct playback invalidation metrics and same-session generated stock-server loss/jitter/duplicate probe implemented. Runtime stall and finite TCP recovery checks cover no resumed held TX. The 121.63-second local generated run completed 49 two-way rounds with 144 injected drops, 80 duplicates and zero unplanned bridge/relay overflow. Measurements and exact software evidence are in docs/VOICE_VALIDATION.md; physical sleep, WAN/device routes and multi-hour optimized sessions remain pending. |
| U14 | AWAITING VALIDATION | Focusable Space PTT with selected-radio accessibility label, focus border and release on Tab-away; scrolling Connection/Controls pages; persistent diagnostic Stop. Private native rfd portal import/export/cancel/conflict checks and headless KWin renders of six pages plus new states at 100%/150% pass. Physical Windows/portal dialogs, keyboard/screen-reader navigation and long translations remain pending. No installer/release work. |
