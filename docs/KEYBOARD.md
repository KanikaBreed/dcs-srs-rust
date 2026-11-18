# Foreground keyboard PTT and live radios

The development console has an opt-in `--ptt keyboard` mode. It uses native Windows console key events, or enhanced keyboard reports in a compatible Unix terminal. It does **not** install a global keyboard hook or read HID devices. Reported focus loss releases transmission. Physical keyboard/headset and terminal combinations still need validation; automated evidence uses a simulated terminal and null audio.

## Run intentionally

Follow [TOC console setup](TOC_CONSOLE.md) for the server, private credential environment and explicit input/output selection. Add `--ptt keyboard` to that command. Build/help does not access a terminal or audio device. Active keyboard mode requires terminal stdin/stdout. On Unix it negotiates enhanced keyboard reporting before connecting or opening audio; unsupported terminals return an error and restore terminal settings. Timed line-command mode remains the default.

Wait for `RX READY`. Tap and release **F8** once to prove that key-release events arrive; this initial tap does not transmit. After `PTT ARMED`, hold F8 to transmit and release it to stop. On keyboards that assign media actions to function keys, the physical function-key modifier may be necessary. F8 avoids treating pasted ordinary text as a transmit action.

| Input | Behavior |
|---|---|
| Hold/release F8 | Local TX while held, with a five-second maximum per press |
| `1`–`9`, `0` | Select transmit radio 1–9 or 10; release PTT first |
| `f` | Release PTT and edit the selected radio's MHz; Enter applies, Escape cancels |
| `m` | Release PTT and switch a tuned radio between AM/FM |
| `x` | Release PTT and disable the selected radio |
| `q`, Escape outside editing, Ctrl+C | Close session/audio and restore terminal settings |

After retuning, changing selection, losing focus, input interruption, or the hold cap, release F8 before pressing again. After an edit or polling interruption, wait for `INPUT READY`. If a release was lost or discarded during recovery, tap/release once to rearm. Focus gain and key repeats never resume transmission. An untuned/disabled radio cannot start TX. The visible `TX LOCAL` state indicates the local gate, not acknowledged delivery. The textual radio lines report locally confirmed settings; later server policy can adjust them, and the worker always enforces its current authorization.

## Failure and input bounds

The foreground adapter polls at 25 ms intervals, processes at most 64 events per pass, and uses the existing 250 ms ownership lease. Repeats and duplicate key-downs neither restart PTT nor extend its five-second cap. A reported focus loss immediately releases PTT. Without a reliable focus/release event, the hold cap still bounds a transmission. These timers depend on process scheduling and are not hard real-time guarantees.

The backlog guard runs only when all 64 event slots were consumed. A fresh event arriving after an empty-queue check belongs to the next tick; it must not be discarded as overflow. This preserves a release sent immediately after `INPUT READY` following a radio edit. The private PTY integration check exercises eight consecutive tune/ready/release/press cycles without adding a delay after that acknowledgement, then still checks the hold cap and interruption behavior.

A polling gap of 250 ms, an event backlog, a paste event, a radio edit or a policy revocation inhibits held input. After a polling gap or asynchronous edit, accumulated events are discarded before accepting fresh input. Release calibration is required again. Frequency editing is limited to 16 ASCII characters; Unix bracketed paste is ignored; native Windows function-key events are distinct from pasted text. The keyboard policy is separate from Crossterm so future native/HID adapters can use the input-owner contract without terminal dependencies in the protocol/radio core.

Raw terminal settings, focus reporting, bracketed-paste mode and enhanced keyboard flags are scoped to a guard. Normal quit, error, disconnection and Ctrl+C through the keyboard loop restore them. Forced process termination such as SIGKILL cannot execute that cleanup; a terminal may then need its normal reset command. No automatic reconnect or hardware-route switch occurs.

## Live radio edits

Timed mode now accepts these commands after `RX READY`:

```text
tune 2 253.125 fm
guard 2 243
key 2 7
select 2
tx 1000
rx
key 2 off
guard 2 off
off 2
```

Slots 1–10 are exposed. Tuning changes frequency/modulation while retaining that slot's guard/key; disabling resets that slot. Selection releases TX and chooses the next transmit radio. All enabled radios remain eligible for receive mixing. A key is SRS simulated matching, not transport encryption. Invalid settings and server-denied encryption are rejected without disconnecting a healthy session.

`RadioControl::apply` edits the session's current radio state through a four-entry queue. A request must start within 250 ms and must still have a waiting caller. Cancellation before processing skips the request. Once its TCP write has started, cancellation does not undo it. The TOC controller releases PTT before enqueueing, bounds acknowledgement waiting to three seconds, and stays inhibited after an abandoned/unknown result until another edit is acknowledged. A successful reply includes the exact authorization revision, allowing a fresh press after an acknowledged edit without confusing it with a held, revoked press. Success means a completed TCP write, not a server acknowledgement.

`Session::next_event_with_updates` selects edits against the transport's internal cancellation-safe read. Partial JSON and unread bytes survive an intervening write. Cancelling the outer operation still closes TCP. A regression test exposed and fixed resetting the read buffer's consumed range before a pending read completed, which could replay old bytes when interrupted.

## Evidence and follow-up

`python3 scripts/check.py` covers release calibration, repeats, focus changes, hold expiry, partial TCP reads during edits, invalid policy, abandoned/expired requests, controller cancellation, retuning during capture and a fresh press on the new wire channel. The existing [stock-server runner](SESSION.md) additionally drives both the line console and a PTY-hosted keyboard console against unmodified SRS 2.4.1.0 using isolated native ALSA null PCM. It negotiates synthetic key reports, tests focus loss, pauses/resumes its own child process to test stale input, tunes while connected, verifies the hold cap, and checks exact terminal-attribute restoration. It never uses the user's terminal or physical audio devices.

A local Linux debug worker regression run exchanged 25 generated frames each way with zero bridge drops; first callback-to-nonzero-output times were 130/121 ms. The two-endpoint child used 0.089 s user CPU, 0.004 s system CPU and 26,404 KiB peak RSS over 2.528 s wall time (Linux `wait4`, excluding the server). This measures the synthetic voice worker, not physical keyboard latency, native audio timing or mobile power; null PCM is unpaced.

Physical validation remains required on Windows native consoles and Linux terminal emulators: check F8 press/release, function-key mapping, focus loss, missing release/hold cap, retuning, server revocation, disconnect and terminal restoration, then listen through two authorized headset endpoints. The graphical client now has separate [background keyboard/joystick adapters](BACKGROUND_PTT.md); physical validation is pending. This terminal adapter remains foreground-only. Mobile adapters are deferred.

References: [Crossterm key event contract](https://docs.rs/crossterm/0.29.0/crossterm/event/struct.KeyEvent.html), [enhancement flags](https://docs.rs/crossterm/0.29.0/crossterm/event/struct.KeyboardEnhancementFlags.html), and the [Kitty keyboard protocol](https://sw.kovidgoyal.net/kitty/keyboard-protocol/).

Whole-process suspension now terminates the audio session through the worker wall-clock guard; reconnect explicitly. The private terminal test queues release/press/repeat during SIGSTOP of its own child, resumes it and checks terminal restoration and no resumed TX. Input-only interruptions still require fresh calibration.
