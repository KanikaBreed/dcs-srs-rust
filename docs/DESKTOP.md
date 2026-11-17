# Desktop client preview

Windows and Linux desktop delivery now comes before Android/iOS. `srs-desktop` provides a Slint window around the existing SRS session and audio engine. This is a source-built development preview, not a packaged or hardware-validated release.

## Run

Install stable Rust (at least 1.95 for Slint 1.19), a C/C++ toolchain and CMake. Windows builds use the MSVC toolchain. On Debian/Ubuntu, install `libasound2-dev pkg-config libxkbcommon-dev libxkbcommon-x11-0 libfontconfig1-dev`; a graphical X11 or Wayland session and system fonts are required. The renderer is software-based and does not require Qt.

```sh
cargo run --locked -p srs-desktop
```

Launching enumerates device metadata but opens no audio stream or SRS connection. Enter the server's **IP address and port**, callsign and coalition password. Choose the microphone and output, then select **Connect & open audio**. The server must enable External AWACS Mode. The microphone runs while connected; capture is discarded outside an authorized hold. Passwords are cleared from the field when connecting and are neither logged nor saved. This is not a promise of memory zeroization. A fresh random wire identity is generated for each connection.

All radios start disabled. Select a slot from 1–10, enter MHz with a decimal point, choose AM/FM and apply tuning. Guard and simulated key have separate Apply buttons; blank turns them off. Displayed state comes from the session, including server-policy adjustments. The editor may replace an unsubmitted draft when authoritative radio state changes. The selected transmit radio and status remain visible while the editors/list scroll. All tuned radios remain monitored by the shared engine.

Tap and release F8, or the on-screen pad, to arm that input; then hold it to talk. Each input arms independently. The maximum hold is five seconds. Release and press again for another transmission. F8 works only while this window owns focus; it is not a global or HID binding. Pointer release/cancellation/leaving the pad, focus loss, edits, selection changes, disconnect and loss of input ownership release PTT. Changing focus back never resumes a hold. Following a UI stall, queued input is discarded until a quiet heartbeat interval and a fresh release/press. A fresh connection starts receive-only.

The TX label means local microphone transmission, not confirmed remote reception. RX means recent mixed audio; this preview does not identify individual receiving radios/speakers. Disconnect and window close stop the owned session and audio streams. There is no automatic reconnect or automatic device switching. Restart to refresh the device list after plugging in another device.

## Ownership and bounds

The UI thread owns input intent and a 50 ms heartbeat. One joinable thread owns a current-thread Tokio runtime, session and native device owners. Latest-value watch channels carry input, radio snapshots and status; the UI radio-command queue has one slot. A command older than 150 ms is rejected. No UI callback performs network/codec work. Native callbacks retain the bounded, allocation-free device bridges.

The worker checks UI freshness every 25 ms and rejects intent at 150 ms, independently of the UI timer. Expired intent cannot revive by refreshing its timestamp. Existing 250 ms controller/capture leases remain a second boundary. Radio edits release PTT before being awaited; presses during an edit are consumed. The five-second hold cap is anchored to the initial input. Disconnect cancels connection/authentication as well as an active session; native device opening is synchronous on the worker and can delay joining until the backend returns.

The UI is compiled with Slint 1.19.0, winit 0.30, software rendering and accessibility enabled. Slint's generated component module permits its generated unsafe/vtable code; every handwritten application module forbids unsafe. The pinned winit integration supplies window focus and F8 events. No MCP, inspection server or system-testing endpoint is enabled in the application.

## Automated inspection

The preview example has synthetic data and **opens no audio or network**. On Linux, use a private display:

```sh
cargo build --locked -p srs-desktop --example desktop_preview
mkdir -p .scratch
xvfb-run -a -s '-screen 0 1400x1100x24 -nolisten tcp' target/debug/examples/desktop_preview .scratch/desktop-rx.ppm
xvfb-run -a -s '-screen 0 1400x1100x24 -nolisten tcp' target/debug/examples/desktop_preview .scratch/desktop-tx.ppm --tx
```

The PPM files are actual software-rendered window snapshots. Set `SLINT_SCALE_FACTOR=1.5` and increase the private display size to inspect DPI scaling. These synthetic labels/counters are not evidence of network delivery.

`python3 scripts/test_stock_server.py` also runs the ignored desktop integration test in a private network namespace with stock SRS 2.4.1.0, a private Xvfb display and process-local ALSA null PCM. Additional Linux test packages are `xvfb xauth x11-utils fonts-dejavu-core`. The helper focuses only its visibly marked synthetic window on that private display. A second generated-tone client sends Opus through the stock server into desktop playout and the RX indicator. GUI callbacks drive connect, tuning, null capture/transmit, guard/key/off and disconnect. It deliberately freezes the UI thread while the session continues to verify release, rejects queued input after that stall, checks pointer cancellation and the hold cap, and joins the worker. It does not automate physical mouse/keyboard input. Ordinary workspace tests skip this explicitly marked integration test. Use `--desktop-only` with the stock-server runner to repeat just this integration check.

Local verification includes the full workspace checks, the private stock-server desktop test, and visual inspection of actual synthetic RX at 100% and TX at 150% scale. These are software/null-device checks, not hardware acceptance.

## Remaining validation

Physical Windows/Linux headsets, listening, device removal, actual foreground keys/mouse, screen lock/sleep, Wayland focus behavior and full stock desktop-client voice exchange remain pending. Accessibility APIs and labels are present; screen-reader navigation, long translations and a broader display/scaling matrix still need human validation. Gain/mute/stereo, per-radio receive indicators, saved servers/presets, configurable/global/HID PTT, device refresh, reconnect, long-session CPU/memory measurements and installers follow separately. Mobile work is deferred until the desktop milestone.

For a hardware check, start this build on an authorized test server alongside stock SRS. Tune matching coalition/radio settings, verify intelligible voice in both directions, then test release, changing focus, moving outside the pad, closing the window and unplugging each device. Confirm no held transmission resumes. Record OS, device, routing, source revision and listening results using [DEVICES.md](DEVICES.md).
