# Desktop background push to talk

In **Controls**, select **Global keyboard shortcut** or **Joystick / HOTAS**, configure the binding, then **Enable background PTT**. Enable is explicit on every launch. **Save controls** saves the choice, F1–F12/modifiers or joystick GUID/button; it never saves enabled, pressed or session state. Profile schema 5 strictly migrates versions 1–4 without enabling input. Radio-transfer format remains version 2.

Connect and tune a radio, release the input to arm, then hold. Wayland requires a tap/release after accepting the portal dialog before the first usable press. Menu changes, radio edits, reconfiguration, stale input and reconnection require another release and press. Background holds survive ordinary window focus changes; foreground keys and the on-screen pad release on focus loss.

| Adapter | Behavior and limits |
|---|---|
| Windows | Polls selected F1–F12 and exact Shift/Ctrl/Alt/Win state every 20 ms. Uses current-down bits, not key history. An inactive/unreadable input desktop is unknown and releases PTT. Secure desktop, screen lock, remote sessions and elevated applications still need physical validation. No text collection or keyboard hook. |
| X11 | Queries selected key/modifier state on an owned asynchronous connection every 20 ms. Does not grab keys. Connection/mapping deadline is 3 s; each query is bounded to 150 ms. Re-enable after changing keyboard layout. |
| Wayland | Uses `org.freedesktop.portal.GlobalShortcuts`; the desktop chooses/approves the actual shortcut, which is displayed. No direct keyboard-device fallback. Unsupported/denied portals leave PTT disabled. Events are filtered to this session/binding and consumed in wire order, including equal timestamps. Binding changes, portal/session loss and stalled delivery revoke the hold. Because the portal supplies edges rather than a pollable physical state, a hold has a 120 s cap. |
| Joystick/HOTAS | SDL 3 is built with joystick/HIDAPI support only and polls one selected device instance/button every 20 ms, on its owned thread. Refresh discovers at most 32 devices and up to 128 buttons per device. The UI uses button numbers 1–128. Device loss disables input; a replacement requires Refresh and Enable. A saved GUID restores selection only when unique. Axes, POV hats, joystick modifier chords and multiple simultaneous bindings are not implemented. Normal OS device permissions are required; no root requirement or permission change is introduced. |

Windows/X11/joystick state holds have no five-second cap. A 150 ms monotonic or wall-clock freshness gap releases input and requires calibration; audio ownership independently expires. The latest-value input channel is bounded. Lost intermediate edges, a restarted backend and input-source changes cannot revive a hold. The Wayland connection is owned per attempt; disabling/dropping it ends pending requests and the shortcut session. Native SDL/OS calls can delay worker shutdown if the backend itself blocks.

The focused on-screen PTT pad supports Tab navigation and held Space, has a visible focus border and exposes the selected radio in its accessibility label. Its default accessibility action focuses the pad; it does not latch transmission. Foreground controls retain the five-second cap. Screen-reader passthrough and physical keyboards remain manual checks.

## Linux development launcher

Modern portals require an application identity and matching desktop entry before accepting global shortcuts. Each owned shortcut connection registers `io.github.KanikaBreed.DcsSrsRust` before creating its session. The Linux window uses the same identity. Registration denial or timeout stops setup; only a missing Registry interface/method on older portals permits their normal identity/permission checks to proceed. Sandboxed applications retain the sandbox-provided identity through ashpd.

Prepare a validation build, then install its generated development entry for the current user:

```sh
python3 scripts/build_desktop.py --out .scratch/desktop-validation
install -Dm644 .scratch/desktop-validation/io.github.KanikaBreed.DcsSrsRust.desktop "${XDG_DATA_HOME:-$HOME/.local/share}/applications/io.github.KanikaBreed.DcsSrsRust.desktop"
```

The helper includes the entry in its checksums but does not install it automatically. Its Exec argument points to that build's absolute path; regenerate/reinstall it after moving or replacing the build. The template is in [the Linux desktop entry](../crates/srs-desktop/linux/io.github.KanikaBreed.DcsSrsRust.desktop). This development launcher is not an installer or release package, and it grants no shortcut permission. Use **Enable background PTT** and complete the desktop dialog. The bottom status shows setup progress until the backend is ready.

On 2026-11-17, the first physical Linux background shortcut attempt did not transmit. A valid CreateSession probe in the same non-application systemd launch context returned `org.freedesktop.portal.Error.NotAllowed: An app id is required` on xdg-desktop-portal 1.22.1 / KDE portal 6.8.2. Registration/launcher support addresses that setup failure; native permission and physical shortcut retesting remain required. Foreground F8 and the earlier voice results are recorded separately in [VOICE_VALIDATION.md](VOICE_VALIDATION.md).

## Evidence and references

`python3 scripts/test_background_input.py` exercises a virtual SDL joystick (selected/unselected button, invalid button and removal), XTest on a private authenticated Xvfb display, and a simulated portal on a private D-Bus with service activation disabled. The portal requires registration on the same connection before CreateSession; it rejects registration, checks missing-Registry legacy fallback and verifies registration on each new attempt. This regression failed without the registration call and passed with it. Other cases include release calibration, equal-time event ordering, wrong session/binding, denial, pending-dialog cancellation, session closure and service-owner loss. Unit tests cover freshness, skipped edges, long holds and wall-clock discontinuities. These checks do not establish physical HOTAS or compositor shortcut support.

The identity fix passed the 140-test workspace check, private native input checks, isolated desktop/stock-server integration and native headless Wayland rendering at 100%/150%. Native GIO launcher checks preserved executable paths containing spaces, quotes, dollars, backticks, percent signs and backslashes; desktop-file validation passed. These do not replace the physical global-shortcut retest.

Host identity follows the [XDG Registry contract](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.host.portal.Registry.html), the [1.22.1 GlobalShortcuts identity check](https://github.com/flatpak/xdg-desktop-portal/blob/1.22.1/src/global-shortcuts.c) and [host desktop-entry lookup](https://github.com/flatpak/xdg-desktop-portal/blob/1.22.1/src/xdp-app-info-host.c).

Behavior was compared with Ciribob and contributors' `DCS-SR-Client/Singletons/InputDeviceManager.cs`, `Common/Settings/Input/InputDevice.cs` and `Common/Settings/Input/InputBindState.cs` at `d01f540ce5e363e9ac841775c8f7e2253138cd81`. Upstream uses background DirectInput polling and broader bindings; this adapter is original Rust code with a narrower explicit binding scope.

Primary API references: [GetAsyncKeyState](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getasynckeystate), [desktop UOI_IO state](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getuserobjectinformationw), [GlobalShortcuts portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html), [shortcut grammar](https://specifications.freedesktop.org/shortcuts/latest/), [SDL background joystick hint](https://wiki.libsdl.org/SDL3/SDL_HINT_JOYSTICK_ALLOW_BACKGROUND_EVENTS). See [desktop validation](DESKTOP_VALIDATION.md) before claiming hardware support.
