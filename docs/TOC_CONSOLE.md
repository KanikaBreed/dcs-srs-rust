# Integrated desktop TOC development console

`srs-toc` connects an authenticated SRS session to capture, Opus, UDP, receive policy/jitter, a bounded mono mix, and playback. `toc_console` is an interactive development tool with default timed transmit commands, live radio edits, and opt-in [foreground held-key PTT](KEYBOARD.md). Global/HID PTT remains pending. Physical headset listening and stock desktop-client exchange remain pending. There is no installer or mobile UI yet.

## Build and explicit use

Follow the native build prerequisites in the [README](../README.md) and first validate your chosen headset with the [local audio probe](DEVICES.md). Build/help does not open devices or sockets:

```sh
cargo build --locked -p srs-toc --bin toc_console
cargo run --locked -p srs-toc --bin toc_console -- --help
```

Use an SRS server where you have permission to connect and transmit. It must enable External AWACS Mode. Set `SRS_EAM_PASSWORD` privately in the launcher's process environment; never put the password in command arguments, screenshots, source, or logs. On Bash, `read -rs SRS_EAM_PASSWORD` followed by `export SRS_EAM_PASSWORD` reads it without terminal echo or shell-history text. The server assigns your coalition from that password. The console does not print credentials or server payloads, and does not record audio. SRS control/voice traffic is not transport-encrypted.

Provide a unique 22-character SRS client ID per concurrent endpoint. This command generates one locally using the Python standard library:

```sh
python3 -c "import base64, uuid; print(base64.urlsafe_b64encode(uuid.uuid4().bytes).decode().rstrip('='))"
```

Replace the example ID, address, callsign, frequency and device selections. IPv4/IPv6 socket addresses are accepted; DNS lookup is not implemented. This example deliberately activates microphone/output for at most five minutes, starting receive-only:

```sh
cargo run --locked -p srs-toc --bin toc_console -- --server 127.0.0.1:5002 --id ZZZZZZZZZZZZZZZZZZZZZZ --name TOC --mhz 251 --mode am --input default --output default --seconds 300
```

Wait for `RX READY`, then enter commands:

| Command | Behavior |
|---|---|
| `tx 1000` | Transmit microphone audio on radio 1 for one second; accepted range is 100–5000 ms |
| `rx` | Release transmit early; keep receiving |
| `quit` | Close the session and devices |

`TX LOCAL` means the local transmit gate is open; SRS does not acknowledge voice delivery. The console displays a separate `RX READY`/`STOPPED` state. Timed-mode `tune`, `guard`, `key`, `off` and `select` commands now edit/select slots 1–10; see [radio commands](KEYBOARD.md). Saved presets, graphical radio controls and receive volume/mute are available in the [desktop client](DESKTOP.md); this diagnostic console has no monitor-level commands. The shared receive engine can mix up to 16 eligible speakers. Use headphones to avoid feedback. Microphone capture remains open during the session but its callback discards samples while PTT is released.

EOF, input-reader failure, line/queue overflow, the session duration limit, and `quit` stop the worker. `Ctrl+C` or terminal termination exits the process and the OS closes sockets/devices. There is no persistent transmitter or automatic reconnect. Commands are limited to 128 bytes and eight queued entries; queued commands older than 250 ms release PTT instead of replaying old input. The console has one blocking stdin reader thread; it exits on EOF, and process termination handles a pending read.

## Ownership and failure contract

The reusable `srs_toc::run` future takes ownership of an authenticated, tuned `Session` and borrows capture/playback bridges. Native stream owners must remain alive until it finishes. It opens UDP for that exact control session. A persistent TCP pump runs concurrently with the audio worker; ordinary audio ticks do not cancel control reads. There are no detached library tasks. Cancelling the outer future deliberately closes TCP/UDP, stops both bridges, and invalidates PTT; reuse requires new streams and a new session.

`controls()` returns one non-cloneable `Controller` and its latest-value input channel. `press` creates a new generation. `renew` extends only a live 250 ms lease; it cannot revive an expired one. Input already held when UDP registration finishes is ignored. Owner drop shuts down the session. A policy revision consumes the current held generation, releases capture/PTT and flushes playback; renewing that generation cannot restart TX. Disabling EAM ends the session. Device error signals and capture stalls end the session; devices never switch/restart automatically.

The worker checks input/device/authorization state every 5 ms while scheduled and before each send. Pending sends observe release/owner loss, policy/TCP changes, device errors and the earlier of input/frame expiry; there is no outgoing frame queue or retransmission. These are scheduling targets, not hard real-time guarantees. Encoding and mixing happen outside device callbacks. Capture retains the device adapter's 80 ms maximum age and 250 ms stall limit.

The mixer polls at 40 ms intervals, taking at most one due frame per speaker. It applies local per-radio receive gains, sums in i32 and saturates once into i16; each speaker retains its own decoder/PLC. This adds up to one 40 ms alignment interval to the existing 80 ms jitter delay. A missed interval flushes downstream backlog instead of playing a catch-up burst. Output queue overflow discards its backlog and is counted. Disconnect/policy/PTT transitions flush partial playback conservatively; hardware samples already submitted cannot be recalled. Peer disconnects remove that speaker's sequence/decoder state and flush the device queue. At most 32 peer-departure events are queued; overload closes the control session instead of retaining stale audio.

This mono mixer defaults to full gain and can clip simultaneous loud voices; the counter reports clipping. Limiting, per-radio panning, smooth gain ramps, effects, adaptive clock drift, 44.1 kHz devices, capture hotplug recovery and sustained-load/WAN behavior need further work. Neither a native error callback nor a software timer establishes physical device audibility.

## Automated evidence and physical follow-up

```sh
python3 scripts/check.py
python3 scripts/test_stock_server.py
```

Workspace tests exercise generated capture through TCP/UDP orchestration, fresh-press gating, old partial-frame rejection, lease expiry, capture stall, reported device failures, policy revocation, cancellation, owner drop and downstream silence. Mixer tests compare the simultaneous decoded sum and saturation, enforce one frame per speaker, and revoke buffered audio. The stock-server runner uses its existing isolated Linux namespace and pinned official SRS 2.4.1.0 binary. It now runs `session_audio_probe` through the same two authenticated endpoints in both directions, with 25 generated capture/voice frames each way; it checks callback output energy and queue drops. No physical device is opened by that probe. Its counters include the two expected PLC tail frames.

A local Linux x86-64 debug run delivered 25 frames each way, produced 27 mixed frames per direction including PLC tails, and recorded zero capture/playback queue drops. First capture-callback to nonzero playback-callback times were 131 ms forward and 120 ms reverse; maximum observed playback-bridge callback work was 32 microseconds. The complete two-endpoint probe took 2.531 seconds wall time, 0.085 seconds user CPU, 0.003 seconds system CPU, and 27,288 KiB peak RSS with receive-monitor controls enabled at their default gain. The runner measures that exact child with Linux `wait4`, excluding the stock server. These are short synthetic callback/loopback measurements, not ADC/DAC latency, WAN quality, a soak result or mobile power data.

The runner also launches the actual console with a process-local ALSA configuration containing only a null PCM and synthetic credentials. It checks receive-only startup, timed TX release, explicit `rx`, `quit`, native stream lifetime, and a nonzero transmitted frame count. The helper `scripts/test_toc_console.py` is invoked inside that runner; it is not a standalone command. Null PCM is unpaced, so these checks do not establish native latency or audio quality.

Physical follow-up: use two authorized SRS endpoints and selected headsets, verify intelligibility both ways, timed TX ending and early `rx`, then interrupt input, disconnect TCP, revoke coalition access and unplug each active device. Confirm local TX stops, old audio does not replay, and reopening needs explicit action. Record Windows/Linux OS, device format, listening/delay observations, counters and failures. Global keyboard/HID PTT, physical headset checks, full stock desktop-client exchange, UI and Android/iOS lifecycle behavior remain pending.

Foreground keyboard mode has a release-event calibration, focus/cancellation rules and a per-press hold cap; see [keyboard contract and validation](KEYBOARD.md). Live edits use a bounded control queue and preserve partially received TCP frames.
