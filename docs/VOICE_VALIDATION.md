# Voice, interruption and usability validation

This is a development validation procedure. Generated audio, null PCM, virtual input and simulated portals are software evidence; they do not establish intelligibility, physical headset routing, stock Windows-client voice interoperability or a supported device list.

## Offline check in the client

Disconnect, open **Connection**, select microphone/output, and choose one of:

- **Microphone level**: captures without playback and shows peak level/clipped samples.
- **Hear microphone**: plays the selected microphone through the SRS Opus encoder/decoder. Wear headphones to avoid feedback.
- **Left / right tones**: alternates a generated 700 Hz signal between ears every two seconds, at about 10% digital peak with short fades. A mono route is labelled explicitly.

Each test stops after 15 seconds or **Stop test**. Stop remains available in the top status bar on every page. An active test excludes server connection and device refresh; it does not record audio or contact a server. A 250 ms UI/worker timing gap, cancellation, device error or window closure stops the owned streams. The test never restarts automatically. Native device calls can delay shutdown if the backend blocks.

`python3 scripts/test_null_audio.py` checks real CPAL/ALSA null streams, diagnostic cancellation, lost UI heartbeat and the 15-second deadline. Null callbacks are unpaced and can overflow capture continuously; those drop/underrun counts are not headset timing evidence. Paced generated bridge tests separately validate codec delivery, stereo and bounded queues.

## Two-way listening with stock SRS

Use an authorized private server and two separate headset endpoints, one running stock SRS. Record both client versions, server version, source commit/build hash, OS/session type, headset/driver and result for every row. Keep credentials, unique hardware identifiers and recordings out of public evidence.

| Check | Expected result | Current human result |
|---|---|---|
| Speak both ways on matching coalition/radio | Intelligible speech; note perceived delay, clipping and drops | Pending |
| Wrong frequency/modulation/key/coalition; guard | Only eligible audio plays; matched guard works | Pending |
| Both/Left/Right and mono; multiple speakers, mute/volume | Correct ears and levels; no stale partial frame after routing change | Pending |
| Focused key/Space/pointer and enabled background keyboard/HOTAS | Fresh release/press required; no stuck TX or unwanted source takeover | Pending |
| Unplug input/output or change OS route while speaking | Audio stops, TX releases; no automatic fallback or resumed hold | Pending |
| Lock, sleep/resume, network removal and reconnect | No stale audio/TX; explicit reconnect after worker/device interruption | Pending |
| Multi-hour optimized session | Record CPU, peak/stable memory, latency, drops, battery if applicable | Pending |

## Repeatable software checks

```sh
python3 scripts/check.py
python3 scripts/test_background_input.py
python3 scripts/test_native_dialogs.py
python3 scripts/test_null_audio.py
python3 scripts/test_stock_server.py --soak-seconds 120
python3 scripts/test_wayland_preview.py --out .scratch/wayland-validation
```

The last four commands require Linux; Wayland preview additionally requires KWin. The stock runner verifies the pinned SRS 2.4.1.0 binary hash and runs only in a fresh loopback-only network namespace. Its optional 1–3600 second test retains the same two sessions, alternates generated one-second talk turns in both directions, and drains both playback devices continuously. A finite UDP relay injects deterministic loss (every 17th voice packet), duplicates (every 29th remaining packet), and 10/30/70 ms delay, producing reordering. Keepalives remain unimpaired. Each turn checks 25 sent frames, stereo isolation, useful playout and capture-to-playout within 400 ms; the impaired case permits PLC/loss instead of claiming perfect reception. Explicit playback flushes between talk turns are counted separately from unexpected stale/overflow drops. The test requires zero unexpected drops and at most three invalidated frames per endpoint per round; the original short unimpeded probe still requires zero total drops. Reported CPU/RSS covers the two synthetic endpoints and relay, excluding the stock server. This is not a WAN or multi-hour certification.

The GUI integration test forces TCP loss during TX, checks finite receive-only recovery, then verifies a fresh press. It also tests focused Space PTT, Tab-away release and offline diagnostic/session exclusion. A separate runtime stall test stops both bridges even if the owner renews on resume. Monotonic and wall clocks are checked; a significant clock adjustment can conservatively interrupt a session.

The native file chooser test runs the real rfd D-Bus request/response path against a private simulated portal, including cancel, escaped filenames, successful import/export, existing destination, duplicate preset and malformed JSON. Native Windows dialogs and visual interaction with the desktop portal remain manual checks.

The Wayland script starts its own virtual KWin, runtime/config/cache/data directories and private bus. It renders all six pages plus keyboard, joystick and audio-test states at 100% and 150%, without opening audio or using the shared display. This proves native Wayland rendering in that environment, not real portal permissions or screen-reader usability. Actual Tab order, screen readers, long translations, Windows scaling and permission dialogs remain on the [human checklist](DESKTOP_VALIDATION.md).

## Local software evidence for the desktop-input batch

Linux x86_64 development build, SRS 2.4.1.0 in a private namespace: the 120-second request completed 49 two-way rounds (121.63 seconds, 2,450 generated sent frames). The relay injected 144 drops and 80 duplicates with zero relay overflow. All 98 talk turns met playout/isolation checks; first capture-to-audible callback was 130–251 ms under these impairments. Maximum measured playback-bridge work was 120 microseconds. Intentional PTT flushes invalidated 37 playback tails; unexpected capture/playback queue drops and engine playback overflows were zero. The process used 4.614 s user CPU plus 0.118 s system CPU over 121.691 s wall time, with 27,564 KiB peak RSS. These are local debug/generated-signal observations, not production latency or headset measurements.

The full-workspace check passed 140 ordinary tests; six environment-specific tests are exercised separately (private GUI, native chooser, null diagnostics, virtual joystick, X11 and shortcut portal). The private GUI passed focused Space transmission, Tab-away release and diagnostic/session exclusion as well as the existing TCP recovery checks. Private KWin rendered 18 cases at 900×800 logical size across 100% and 150% scaling. Windows/macOS host results are recorded with the PR's exact CI revision. All human rows above remain pending.
