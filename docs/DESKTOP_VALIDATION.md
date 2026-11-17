# Desktop validation handoff

This checklist is for the Windows/Linux standalone TOC preview. Physical tests below are **pending**, even when CI passes. Do not use a shared server or microphone until its operator/user has agreed to that test. Use headphones. This build is not an installer or release.

## Prepare an identifiable build

Follow the native dependencies in [DESKTOP.md](DESKTOP.md), then run these commands in the chosen source checkout (Windows: `py -3` in place of `python3`):

```sh
python3 scripts/check.py
python3 scripts/build_desktop.py --out .scratch/desktop-validation
```

The script builds the desktop binary with `Cargo.lock`, copies it into a new directory, and records its SHA-256, source revision/dirty status, Rust/Cargo versions and build profile in `manifest.json`. It never overwrites a previous validation build or launches audio. Use `--release` for an optimized build. Debug builds are suitable for menu checks; use optimized builds for meaningful CPU/latency measurements. Recording the build inputs does not promise bit-for-bit reproducibility across toolchains/OS versions.

Launch `srs-desktop.exe` on Windows or `./srs-desktop` on Linux from the output directory. Keep the manifest with the results. Use a clean committed checkout for a reviewable source identity; `source_dirty: true` cannot identify uncommitted content by commit alone. Native libraries/fonts still come from the host. The copied project notices do not constitute the full dependency/source bundle needed for redistribution. Signing, installers, dependency notices and distribution-source availability remain release gates.

## Menu and native input checks

| Check | Steps and expected result | Current evidence |
|---|---|---|
| Launch/navigation | Launch offline. Visit Radios, Connection, Saved setups, Controls, Server & clients, Help. Resize and test 100/150/200% scaling. Connection/PTT status and disconnect remain reachable; no audio opens on launch. | Private Xvfb render/callback checks at 1080×900 and 900×800 logical size, including 150% scaling; native Windows/Wayland and 200% pending. |
| Devices | Refresh offline with a headset attached, remove it, refresh, reattach and refresh. Explicit device IDs remain selected, missing choices block connection and never silently fall back. Check System default follows the OS choice. | ID/restoration and null-device refresh tests; physical hotplug pending. |
| Server setup | Use an authorized EAM server. Connect by hostname, IPv4 and bracketed IPv6 where supported. Try a wrong password. Password clears on connect and is absent from saved settings. | Loopback parser/DNS and stock hostname sign-in tests; real DNS/network/IPv6 server pending. |
| Keyboard/PTT | Choose F1, F8, F12 and Mouse only. Save/relaunch. Calibrate with tap/release, then hold. Check modifiers, repeats, focus changes, menu changes, rebinding, pointer leaving/canceling and five-second limit. No held TX resumes after interruption. | Synthetic input/controller and GUI release/persistence checks; actual keys/mouse/OS shortcuts pending. |
| Preset transfer | Save two presets. Export to a new file. Import into a separate test configuration; inspect all ten radios, selected slot, volume and mute. Try cancel, conflict, invalid JSON and an existing export destination. Active radios change only after Load. Close the chooser before the main window. | Bounded file, migration, conflict and no-overwrite tests; native portal/Windows/macOS chooser interaction pending. |
| Server/help | Compare shown version/coalition/allowed policies and visible clients with stock SRS. Check unknown values and post-disconnect “last session” state. Read help without clipping or inaccessible controls. | Scripted sanitized 257-client/256-display test and live stock policy view; screen readers/long translations pending. |

## Voice and recovery checks

1. Use stock SRS and this client on the authorized test server with matching coalition, frequency/modulation, guard and simulated keys. Speak both ways. Record intelligibility, clipping, delay, dropouts and radio matching. Automated generated Opus/null-device checks do not count as a listening pass.
2. Adjust each receive level and mute while multiple peers speak. Confirm volume affects reception only and RX remains visible when muted. Audio currently routes to both headphone channels; stereo assignment remains planned.
3. With retry **off**, interrupt the network and confirm the session stops. With **Retry network failures** enabled before connection, interrupt TCP during TX and during receive. Reconnection retains acknowledged radio/monitor settings, starts receive-only and requires a fresh PTT action. A new session has fresh counters. Stop recovery with Disconnect.
4. Leave the server unreachable through all three retries (1/2/4 second backoffs). Confirm the fourth failed connection stops. Wrong credentials, EAM/policy denial and device failure must stop without repeated attempts. OS connection/DNS deadlines are additional to backoff; a blocked native resolver/device call may delay shutdown.
5. Unplug capture/output, change OS audio route, lock/sleep/resume and close the app while talking. Verify devices close and no stale audio or PTT resumes. Retry never selects another device automatically. An explicit System default selection may follow a changed OS default on a new session.
6. Run an optimized multi-hour session and record CPU, memory, latency, drops and device stability. Accessibility navigation and screen-reader labels need their own results.

The private GUI test already forces TCP loss through a loopback relay while transmitting, reconnects to stock SRS with new streams, checks retained radio/monitor settings and zero resumed TX, then verifies a fresh press works. Unit tests cover the retry budget, new identities, credential rejection and cancellation during backoff. These are software evidence, not approval for headsets, HID/global PTT, Wayland or mobile.

Record source revision, binary hash, OS/session type, device/driver, SRS version, each check's result and any reproduction steps in [DEVICES.md](DEVICES.md)'s report format. Keep credentials and recordings out of public reports. Outstanding development work is mapped to the original source menus in [DESKTOP_PARITY.md](DESKTOP_PARITY.md).
