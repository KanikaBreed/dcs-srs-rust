# Scope and delivery plan

## Product

DCS SRS Rust targets Windows, Linux, Android, and iOS. The first release is a standalone TOC radio console with full bidirectional voice and push-to-talk. Operators connect to existing SRS servers without running DCS on their device. Server-enabled External AWACS Mode supplies coalition authentication.

## First desktop release

- Saved servers, callsign, coalition credentials, status, reconnect, and actionable errors.
- Named presets; frequency, modulation, guard, and simulated encryption settings consistent with upstream/server policy.
- Multiple monitored radios with volume, mute, stereo placement, and clear speaker/activity indicators.
- One clearly selected transmit target with hold-to-talk, plus configurable desktop hardware PTT.
- Compatible 11-slot model; a phone may show a subset without silently dropping monitored radio state.
- Deliberate microphone/device selection, interruption handling, and release of stale PTT.
- Diagnostics that expose connection/audio health without voice recordings or credentials.

## Mobile releases

Android background listening/transmission and iOS foreground/background operation follow as separate acceptance gates after the desktop milestone.

## Platform sequence

Windows and Linux desktop come first: deliver and validate the graphical TOC client, bidirectional voice, PTT and device behavior before starting mobile application work. Android follows that desktop milestone; iOS follows its own lifecycle and distribution validation. Mobile spikes are deferred under this priority. Desktop CI on macOS is not an iOS app build. See the current [desktop preview](DESKTOP.md).

## Later deliverables

DCS cockpit synchronization, aircraft-specific behavior, terrain/line-of-sight integration, overlays, recording, advanced retransmission/simulcast, and external integrations follow as separate increments. A Rust server is optional and follows client interoperability. No server replacement is necessary for the first desktop/Android milestone.

## Initial effort estimate

Planning ranges for one experienced full-time developer; not delivery commitments. Device access and native integration may change them.

| Increment | Additional engineering time |
|---|---|
| Protocol/toolkit proof for desktop | 2–3 weeks |
| Shared engine and Windows/Linux voice alpha | 5–8 weeks |
| Desktop feature completion, then Android TOC | 5–8 weeks |
| Reliability, hardware matrix, packaging, beta fixes | 3–5 weeks |
| iOS delivery, potentially including a gateway | 6–10 weeks |
| Optional compatible Rust server | 4–8 weeks |

The original combined Windows/Linux/Android beta estimate was roughly 4–6 months. Broader DCS parity, iOS, and server replacement expand the planning envelope toward 9–15 months. Re-estimate after the first milestone.

## First milestone acceptance

A Rust client and stock SRS client exchange intelligible voice through an unmodified test server. Both directions work; coalition credentials and server settings are respected; multiple frequencies can be monitored; PTT releases correctly. Independent packet fixtures pass. Record baseline latency and recovery behavior. Android and iOS background-operation spikes belong to the subsequent mobile milestone.

## Open design decisions

- UI/audio toolkit acceptance after physical-device testing.
- Exact minimum OS versions and supported Bluetooth/PTT accessories.
- Whether an iOS wake-up gateway is needed, and who operates it.
- iOS distribution/licensing route for a GPL application.
- Desktop DCS integration order and scope after TOC beta.

Current work is tracked in [TASKS.md](../TASKS.md).
