# Testing and acceptance

## Checks available today

```sh
python3 scripts/check.py
python3 scripts/check.py --docs-only
```

Use `py -3` on Windows. Full checks validate relative Markdown file targets, workspace and fuzz-target formatting, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked`.

The link check validates file existence only, not remote URLs or heading anchors. The protocol suite tests upstream UDP vectors, control JSON, enum values, unknown fields, TCP fragmentation/coalescing, size limits, invalid input, and single-byte packet mutations. The session suite uses loopback TCP to test login, radio updates, metadata retention, revocation, cancellation, deadlines, and cumulative resource limits. CI runs these checks on Linux, Windows, and macOS hosts. Linux also compiles fuzz targets, checks fixtures with unchanged upstream C# sources, runs isolated stock-server control/generated-voice tests, and verifies bidirectional Opus exchange with original C# wrappers. The additional UDP/codec tests cover registration loss/retry, heartbeat expiry, packet identity/size, PTT invalidation, TCP-to-UDP liveness, frame duration, PLC, and generated-signal decoding. Receive tests add 46 original C# matcher cases, deterministic loss/reorder/duplicate schedules, bounded concealment, stale-frame drops, timer drift, capacity, and policy-revoked playout. Device tests also verify bounded callback queues, zero callback heap allocations/deallocations, lease/stop/stall behavior, sample-format conversion, and anti-alias filtering. Linux exercises native CPAL through a process-local ALSA null PCM. CI does not test physical audio hardware or build mobile apps yet.

For independent local verification, follow the [fixture guide](../crates/srs-protocol/tests/fixtures/README.md). It requires .NET SDK 10 and a clean pinned reference. The Rust round-trip example and C# oracle open no sockets or audio devices. For fuzzing, see [the target instructions](../fuzz/README.md).

## Test layers to implement

| Layer | Required evidence |
|---|---|
| Core rules | Radio eligibility, state transitions, explicit PTT release, server settings, invalid input |
| Protocol/control (implemented) | Independent vectors, framing, bounded session state, and control/version/EAM behavior against stock SRS 2.4.1.0; broad release compatibility remains pending |
| Audio simulation | Generated signals, loss/reorder/duplication/jitter, bounded buffering, sample-rate conversion, clipping |
| Stock SRS interoperability | Both voice directions and server policy with pinned unmodified reference versions |
| Native desktop | Device selection/hotplug, keyboard and HID PTT, X11/Wayland, sleep/resume, CPU contention |
| Android device | User-started service, locked screen, interruptions, permissions, Bluetooth, network transitions, battery |
| iOS device | Audio session activation, foreground/background distinction, system PTT, wake-up path, first-audio buffering |
| UI | Touch and keyboard, screen reader, text scaling, visible TX/RX, error recovery, narrow and tablet layouts |

## Harnesses

Session tests use loopback sockets on ephemeral ports, scripted peers, and Tokio virtual time for refresh scheduling. The stock-server runner uses a fresh private Linux network namespace and temporary synthetic configuration. Run `python3 scripts/test_stock_server.py`; [SESSION.md](SESSION.md) records the pinned release hash, prerequisites, exact checks, and limits. Both enabled and disabled EAM are tested. This runner downloads a server binary and opens isolated local sockets, but uses no audio devices.

Generated-tone probes now report local encode-start-to-PCM-playout timing, spacing variation, frame delivery, and codec durations. [VOICE.md](VOICE.md) gives the commands and measurement limits. The [receive guide](RECEIVE.md) documents jitter/impairment simulations and the offline 16-speaker `jitter_probe` CPU/memory workload. Keep test configurations isolated from user profiles. Bind local test services to loopback by default and use distinct ports for parallel sessions.

Use a task-specific window title for future automated UI demos and make automation visibly identifiable. Do not copy Aurora environment variable names or document nonexistent commands. Add runnable commands here when the harness lands.

See [device validation](DEVICES.md) for local tone/capture/loopback commands and physical headset checks.

## Voice and PTT acceptance

- A release event, capture failure, lost input owner, interrupted session, or disconnected transport ends transmission.
- Reconnect does not resume a stale transmit action.
- Audio queues remain bounded and stale audio is not replayed after a long pause.
- Independent receivers observe the expected radio/coalition behavior; the local TX light alone is insufficient evidence.
- A human checks intelligibility, clipping, sidetone/feedback, simultaneous receptions, and routing on the actual hardware.
- Unsupported Bluetooth control behavior is reported as unsupported, not silently treated as a working PTT button.

## Performance reports

Record source revision, OS/device, input/output device and sample rate, frame size, codec settings, network RTT/jitter/loss, queue depth, underruns, CPU, and memory. Measure end-to-end latency with a defined method; distinguish it from codec-frame duration or network ping. Report percentiles and worst observed dropouts during multi-hour sessions. For mobile, include idle listening power, active TX/RX, locked-screen duration, and network handover.

Numeric release thresholds will be set from measured baseline results before beta; no latency or battery guarantee exists today.

## Evidence and privacy

Use generated audio and synthetic identities for automated captures. Store logs/screenshots locally in `.scratch/`, review and redact before sharing, and never publish coalition credentials, APNs tokens, private server addresses, personal paths, or other users' voices. Note exactly which checks were run and which still require a person or device. Follow [bug reporting](BUG_REPORTING.md).

The integrated [TOC console checks](TOC_CONSOLE.md) add generated callback-to-network tests, mono mix/saturation, input expiry and ownership loss, and session/device cancellation. `test_stock_server.py` also runs the synthetic session audio probe and an actual native null-PCM console session in its private network namespace. The runner also tests [foreground keyboard PTT](KEYBOARD.md) with a private PTY, generated release/focus events, a child-process pause/resume and exact terminal restoration. Physical listening, physical foreground-key validation and global/HID PTT remain pending.
