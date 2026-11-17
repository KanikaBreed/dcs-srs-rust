# Desktop client preview

Windows and Linux desktop delivery now comes before Android/iOS. `srs-desktop` provides a Slint window around the existing SRS session and audio engine. This is a source-built development preview, not a packaged or hardware-validated release.

## Run

Install stable Rust (at least 1.95 for Slint 1.19), a C/C++ toolchain and CMake. Windows builds use the MSVC toolchain. On Debian/Ubuntu, install `libasound2-dev pkg-config libxkbcommon-dev libxkbcommon-x11-dev libfontconfig1-dev libx11-xcb-dev libx11-dev libxcursor-dev xinput`; a graphical X11 or Wayland session and system fonts are required. The renderer is software-based and does not require Qt. X11 packages follow the [Slint winit dependency list](https://docs.slint.dev/latest/docs/slint/guide/backends-and-renderers/backend_winit/).

```sh
cargo run --locked -p srs-desktop
```

Launching enumerates device metadata but opens no audio stream or SRS connection. Enter the server's **IP address and port**, callsign and coalition password. Choose the microphone and output, then select **Connect & open audio**. The server must enable External AWACS Mode. The microphone runs while connected; capture is discarded outside an authorized hold. Passwords are cleared from the field when connecting and are neither logged nor saved. This is not a promise of memory zeroization. A fresh random wire identity is generated for each connection.

A fresh launch starts with all radios disabled; tune offline or explicitly load a saved preset before connecting. Select a slot from 1–10, enter MHz with a decimal point, choose AM/FM and apply tuning. Radio names, guard and simulated key have separate Apply buttons; blank turns them off. Displayed state comes from the session, including server-policy adjustments. The editor may replace an unsubmitted draft when authoritative radio state changes. The selected transmit radio and status remain visible while the editors/list scroll. All tuned radios remain monitored by the shared engine.

Tap and release F8, or the on-screen pad, to arm that input; then hold it to talk. Each input arms independently. The maximum hold is five seconds. Release and press again for another transmission. F8 works only while this window owns focus; it is not a global or HID binding. Pointer release/cancellation/leaving the pad, focus loss, edits, selection changes, disconnect and loss of input ownership release PTT. Changing focus back never resumes a hold. Following a UI stall, queued input is discarded until a quiet heartbeat interval and a fresh release/press. A fresh connection starts receive-only.

The TX label means local microphone transmission, not confirmed remote reception. Each radio now shows recent **RX** when a decoded voice frame is assigned to it, including when muted; concealment frames do not extend that indication. The top RX indicator means at least one radio recently received voice. It does not confirm audible output or identify speakers.

Select a radio to set **Receive volume** from 0–100% or **Mute receive**. Volume scales decoded PCM linearly before mixing; 100% preserves the previous level and 0% is exact silence. Mute retains the chosen volume for later unmuting. These settings do not disable that radio or its transmit eligibility. Desktop adjustments release held PTT, as other editor actions do. Muted input continues through decoding so ongoing reception stays current; the worker flushes queued/partial playback when a level changes. Samples already submitted to hardware cannot be recalled.

Receive selection still uses the first eligible packet frequency and the existing upstream radio search order, once per original speaker/sequence. Local volume/mute does not reroute a transmission. If two radios match the same transmission, the chosen matching radio controls its level and RX indicator. Selected-radio/volume priority scoring and stereo routing remain pending. Disconnect and window close stop the owned session and audio streams. There is no automatic reconnect or automatic device switching. Restart to refresh the device list after plugging in another device.

## Saved servers and radio presets

Choose **Show saved setups** to open the saved-server and preset controls; hide them to give the radio editor more space. While disconnected, enter an address, callsign and device choices. Enter a saved-server name and choose **Save / replace**. Select an entry and **Load** to restore those fields; enter the coalition password again. Devices are matched by their backend ID rather than list position. An unplugged or unavailable saved device stays visibly unavailable and blocks connection until you choose a replacement. An explicitly saved **System default** follows the OS default. Loading never connects automatically.

Prepare up to ten radios while offline, or use the current connected radio state. Enter a radio-preset name and **Save / replace**. A preset stores all ten RF slots, including names, tuning, AM/FM/off, guard, simulated keys, receive volume/mute and selected transmit slot. **Load** replaces all radios together. While connected, the complete preset must pass current server policy before a single TCP radio update is sent. Rejection leaves every radio unchanged. Loading releases PTT; an old hold never resumes. Acknowledgement means a completed TCP write, not confirmed server acceptance or delivery. Initial radios also pass policy before the connection opens audio.

Saved servers and radio presets are separate lists, so the same radios can be reused on different servers. **Save / replace** overwrites an entry with the same name; **Remove** deletes the selected saved entry without changing the active session. Names are case-sensitive, limited to 64 UTF-8 bytes; radio names may be blank. There are at most 32 entries per list. Local radio names retain their spelling; the existing wire serializer follows upstream `Common/Models/Player/RadioBase.cs` at `d01f540ce5e363e9ac841775c8f7e2253138cd81` (lowercase ASCII alphanumeric, 32-character limit).

The version-2 JSON file is `dcs-srs-rust/profiles.json` inside the platform user configuration directory: `$XDG_CONFIG_HOME` (normally `~/.config`) on Linux, `%APPDATA%` on Windows, and `~/Library/Application Support` on macOS. It contains server addresses, callsigns, device IDs and radio presets in plain text, including simulated radio keys and receive levels. Coalition passwords, session identities, held input and audio are never stored. Version-1 files load with every receive level at 100% and unmuted. Loading validates the old schema and migrates only in memory; the next explicit Save/Remove writes version 2. Older builds preserve but cannot open version-2 files. Saved lists load at launch; applying an entry always requires **Load**. No settings are written just by launching or closing the app.

Reads are limited to 256 KiB and validate the complete schema, version, names and radio values. Corrupt, unknown/newer-schema and oversized files are preserved and cannot be overwritten by Save. Repair or move the file, then choose **Reload saved lists**. Saves use a temporary file in the same directory, flush its contents and atomically replace the old file. Unix files use mode 0600 and newly created directories 0700; Windows uses inherited directory access controls. A nonblocking sidecar lock and byte comparison reject another instance's unseen changes; reload before retrying. Atomic replacement does not promise directory-entry durability across power failure. Disk errors leave the session usable and show a separate settings message. Preset import/export UI and migration from future schema versions remain separate work.

## Ownership and bounds

The UI thread owns input intent and a 50 ms heartbeat. One joinable thread owns a current-thread Tokio runtime, session and native device owners. Latest-value watch channels carry input, radio snapshots and status; the UI radio-command queue has one slot. A command older than 150 ms is rejected. Receive levels use bounded latest-value channels from UI to controller to mixer, without TCP changes or work in audio callbacks. RX counters keep brief activity visible across the UI heartbeat. No UI callback performs network/codec or filesystem work. A second joinable worker owns profile reads/writes, with one-slot command/result queues and one outstanding UI request. Window close joins that worker too; a slow or unavailable configuration filesystem can delay shutdown. Native callbacks retain the bounded, allocation-free device bridges.

The worker checks UI freshness every 25 ms and rejects intent at 150 ms, independently of the UI timer. Expired intent cannot revive by refreshing its timestamp. Existing 250 ms controller/capture leases remain a second boundary. Radio edits release PTT before being awaited; presses during an edit are consumed. The five-second hold cap is anchored to the initial input. Disconnect cancels connection/authentication as well as an active session; native device opening is synchronous on the worker and can delay joining until the backend returns.

The UI is compiled with Slint 1.19.0, winit 0.30, software rendering and accessibility enabled. Slint's generated component module permits its generated unsafe/vtable code; every handwritten application module forbids unsafe. The pinned winit integration supplies window focus and F8 events. No MCP, inspection server or system-testing endpoint is enabled in the application.

## Automated inspection

The preview example has synthetic data and **opens no audio or network**. On Linux, use a private display:

```sh
cargo build --locked -p srs-desktop --example desktop_preview
mkdir -p .scratch
env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET WINIT_UNIX_BACKEND=x11 xvfb-run -a -s '-screen 0 1400x1100x24 -nolisten tcp' target/debug/examples/desktop_preview .scratch/desktop-rx.ppm
env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET WINIT_UNIX_BACKEND=x11 xvfb-run -a -s '-screen 0 1400x1100x24 -nolisten tcp' target/debug/examples/desktop_preview .scratch/desktop-tx.ppm --tx
```

Add `--profiles` to show the expanded saved-setup controls in a taller synthetic window. The PPM files are actual software-rendered window snapshots. Set `SLINT_SCALE_FACTOR=1.5` and increase the private display size to inspect DPI scaling. These synthetic labels/counters are not evidence of network delivery.

`python3 scripts/test_stock_server.py` also runs the ignored desktop integration test in a private network namespace with stock SRS 2.4.1.0, a private Xvfb display and process-local ALSA null PCM. Additional Linux test packages are `xvfb xauth x11-utils fonts-dejavu-core`. The helper focuses only its visibly marked synthetic window on that private display. A second generated-tone client sends Opus through the stock server into desktop playout and the RX indicator. GUI callbacks drive connect, tuning, null capture/transmit, guard/key/off and disconnect. It deliberately freezes the UI thread while the session continues to verify release, rejects queued input after that stall, checks pointer cancellation and the hold cap, saves/reloads a server and named radios in a temporary configuration directory, reconnects with those radios, applies a live preset during TX, verifies receive levels survive save/reload/reconnect, checks the matching radio RX indication, and joins both workers. It does not automate physical mouse/keyboard input. Ordinary workspace tests skip this explicitly marked integration test. Use `--desktop-only` with the stock-server runner to repeat just this integration check.

Local verification includes the full workspace checks, the private stock-server desktop test, and visual inspection of actual synthetic RX at 100% and TX at 150% scale. These are software/null-device checks, not hardware acceptance.

## Remaining validation

Physical Windows/Linux headsets, listening, device removal, actual foreground keys/mouse, screen lock/sleep, Wayland focus behavior and full stock desktop-client voice exchange remain pending. Accessibility APIs and labels are present; screen-reader navigation, long translations and a broader display/scaling matrix still need human validation. Stereo routing, receive priority scoring, preset import/export, configurable/global/HID PTT, device refresh, reconnect, long-session CPU/memory measurements and installers follow separately. Mobile work is deferred until the desktop milestone.

For a hardware check, start this build on an authorized test server alongside stock SRS. Tune matching coalition/radio settings, verify intelligible voice in both directions, then test release, changing focus, moving outside the pad, closing the window and unplugging each device. Confirm no held transmission resumes. Record OS, device, routing, source revision and listening results using [DEVICES.md](DEVICES.md).
