# Testing and acceptance

## Checks available today

```sh
python3 scripts/check.py
python3 scripts/check.py --docs-only
```

Use `py -3` on Windows. Full checks validate relative Markdown file targets, `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `cargo test --workspace --locked`.

The link check validates file existence only, not remote URLs or heading anchors. The scaffold has no behavior tests. CI runs these checks on Linux, Windows, and macOS hosts; it does not test physical audio hardware or build mobile apps yet.

## Test layers to implement

| Layer | Required evidence |
|---|---|
| Core rules | Radio eligibility, state transitions, explicit PTT release, server settings, invalid input |
| Protocol | Independent upstream vectors, truncated/malformed data, TCP framing, compatibility version handling |
| Audio simulation | Generated signals, loss/reorder/duplication/jitter, bounded buffering, sample-rate conversion, clipping |
| Stock SRS interoperability | Both voice directions and server policy with pinned unmodified reference versions |
| Native desktop | Device selection/hotplug, keyboard and HID PTT, X11/Wayland, sleep/resume, CPU contention |
| Android device | User-started service, locked screen, interruptions, permissions, Bluetooth, network transitions, battery |
| iOS device | Audio session activation, foreground/background distinction, system PTT, wake-up path, first-audio buffering |
| UI | Touch and keyboard, screen reader, text scaling, visible TX/RX, error recovery, narrow and tablet layouts |

## Offline harness design (planned)

Implement deterministic fake clocks, mock transports, synthetic audio sources, packet fixtures, and loopback reference servers. These are planned facilities, not existing CLI flags. Keep test configurations isolated from user profiles. Bind local test services to loopback by default and use distinct ports for parallel sessions.

Use a task-specific window title for future automated UI demos and make automation visibly identifiable. Do not copy Aurora environment variable names or document nonexistent commands. Add runnable commands here when the harness lands.

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
