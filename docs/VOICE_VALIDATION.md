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
| Speak both ways on matching coalition/radio | Speech is heard at both endpoints | User reported both directions working on 2026-11-17; see the session record below. |
| Basic PTT release at both endpoints | TX indication turns off immediately on release and the other endpoint stops receiving | User reported the requested check working correctly on 2026-11-17. Exact input method and measured stop latency were not recorded. |
| Manual Linux disconnect/reconnect | Reconnected client starts with TX off, transmits only after a fresh PTT press, and speech works both ways again | User reported the requested check working on 2026-11-17. |
| Speech quality | Assess intelligibility, perceived delay, clipping and drops | Detailed assessment pending |
| Windows-to-Linux frequency isolation | With other Linux radios and guard off, Windows radio 1 at 251.000 MHz AM is silent on Linux radio 1 at 252.000 MHz AM; returning Linux to 251.000 MHz AM restores speech | User reported the requested check working on 2026-11-17. |
| Reverse-direction frequency isolation; wrong modulation/key/coalition; guard | Only eligible audio plays; matched guard works | Pending |
| Linux radio 1 mute/unmute with stock Windows speech | Muting stops audible speech while RX still shows reception; unmuting restores speech | User reported the requested check working on 2026-11-17, with Windows transmitting on 251.000 MHz AM. |
| Linux radio 1 Left/Right/Both with stock Windows speech | Left plays only in the left ear, Right only in the right ear, and Both in both ears | User reported the requested stereo-headphone check working on 2026-11-17. Output model/driver and exact transition timing were not recorded. |
| Mono fallback; multiple speakers and receive volume; routing transitions | Correct levels and mixing; no stale partial frame after routing change | Pending |
| Linux foreground F8 calibration and focus loss | With background PTT disabled, tap/release arms F8; holding transmits; clicking another application stops TX; returning while held does not resume TX; a fresh release/press transmits again | User replied “Good” to the requested check on 2026-11-17; recorded as a human-reported pass. |
| Linux Wayland global keyboard shortcut | Approved shortcut transmits while another app has focus, including a seven-second hold, then releases | Initial attempt failed (enable, hold, no TX). After the registration/launcher fix, the user reported “working now” in response to the requested approved-shortcut, tap/release calibration, seven-second background hold and release check. See BACKGROUND_PTT.md and the exact fix build below. |
| Other foreground keys, Space/pointer details and HOTAS | Fresh release/press required; no stuck TX or unwanted source takeover | Pending |
| Unplug input/output or change OS route while speaking | Audio stops, TX releases; no automatic fallback or resumed hold | Pending |
| Lock, sleep/resume, network removal and reconnect | No stale audio/TX; explicit reconnect after worker/device interruption | Pending |
| Multi-hour optimized session | Record CPU, peak/stable memory, latency, drops, battery if applicable | Pending |

## First human voice result (2026-11-17)

The user connected stock SRS 2.4.1.0 on a Windows laptop and the Rust desktop client on Arch Linux/Wayland to the private stock SRS 2.4.1.0 server hosted on the Linux machine. After connecting both clients, the user reported “works both ways.” The follow-up asked the user to hold PTT, speak, release, confirm TX turns off immediately and the other endpoint stops receiving, then repeat from both clients; the user reported “works correctly.” These establish basic human-reported two-way speech and PTT-release passes for these endpoints. They do not validate the Rust client on Windows or establish a measured stop-latency bound.

The user subsequently reported the manual Linux disconnect/reconnect check working: TX starts off after reconnect, transmission requires a fresh PTT press, and two-way speech resumes. This is a manual reconnect result; forced network loss, automatic retry and disconnect with PTT held were not part of the requested check.

The Windows-to-Linux frequency isolation check was also reported working. With other Linux radios and guard disabled, Windows radio 1 transmitted on 251.000 MHz AM while Linux radio 1 was tuned to 252.000 MHz AM: Linux stayed silent. Returning Linux to 251.000 MHz AM restored speech. This checks the combined server/client receive behavior in that direction; reverse-direction frequency isolation, modulation, encryption key, coalition and guard behavior were not covered by this check.

The prepared test used Blue External AWACS Mode, radio 1 at 251.000 MHz AM, with encryption off. The running Linux development build came from clean source `be92335989ea7143fe968dd56ac50887e7cedecb`, whose tree matches merged main `bcf2c8f1525d8ad599c7b375f84edd10f50950cd`. Its executable SHA-256 is `ce1d1756186e2632fe47aa3939a5864903a100d946def774f23e8d8d700cfdc0`.

The separate background-shortcut fix build uses clean source `bfdb057a27e79f8a005191f628a1c9e50fca9f3b` and executable SHA-256 `b5ff7a1098fdf764a0612ba5d64b02dab42a6d06cfcb71fb4902a42f84668e88`. After native portal registration/CreateSession succeeded with the installed development entry, the user reported the requested global-shortcut retest “working now” on this build. This establishes a basic human-reported Linux/KDE Wayland background-keyboard pass, including Linux speech received by stock Windows SRS; no hold/stop latency was measured. The earlier passing listening/input rows identify the original build; they are not automatically transferred to this new build.

Headset/microphone models, drivers, Windows version, PTT input used for the earlier speech checks, test duration, detailed speech quality and latency were not reported. F8 was specified for the foreground focus-loss check. Other foreground input behavior, other desktop/background-keyboard combinations, HOTAS, portal denial/cancellation/lock behavior, remaining radio isolation cases, mono fallback, multiple speakers, receive volume, precise routing-transition behavior, device removal, interruption/automatic recovery and soak checks remain pending. No voice recording was collected by the agent; credentials, network addresses and device identifiers are excluded from this record.

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

The full-workspace check passed 140 ordinary tests; six environment-specific tests are exercised separately (private GUI, native chooser, null diagnostics, virtual joystick, X11 and shortcut portal). The private GUI passed focused Space transmission, Tab-away release and diagnostic/session exclusion as well as the existing TCP recovery checks. Private KWin rendered 18 cases at 900×800 logical size across 100% and 150% scaling. Windows/macOS host results are recorded with the PR's exact CI revision. These software results are separate from the human-reported two-way speech result above; the remaining human rows are pending.
