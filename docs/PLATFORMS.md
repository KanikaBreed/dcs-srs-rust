# Platform implementation plan

Application delivery below remains planned. The shared radio engine, desktop audio probe and integrated [TOC console](TOC_CONSOLE.md) build on host targets. The native Linux adapter is exercised with an isolated null PCM; physical audio and full radio application validation remain pending. See [device guide](DEVICES.md).

| Target | Native responsibilities | Acceptance gate |
|---|---|---|
| Windows | WASAPI/device routing, keyboard/HID/joystick PTT, installer/signing, later DCS integration | Real headset and hardware PTT with stock SRS |
| Linux | ALSA/PipeWire/PulseAudio routing, X11/Wayland shortcuts, device access, packaging | Supported desktops and headset/hotplug matrix |
| Android | Microphone/media foreground services as required, audio focus, permissions, routing, hardware controls | Actual phone/tablet, locked-screen sessions and interruptions |
| iOS/iPadOS | AVAudioSession, system Push to Talk, wake-up path, signing/provisioning | Actual iPhone/iPad, foreground and suspended-state reception |

## Shared candidates

[CPAL 0.18.2](https://docs.rs/cpal/0.18.2/cpal/) is now used by the desktop probe with WASAPI, ALSA, and CoreAudio. Its mobile backends are not a validated lifecycle integration. [Slint](https://slint.dev/faqs) supports those UI targets. The UI remains a candidate; neither dependency guarantees lifecycle/accessory support. Verify the chosen release and native integration in the first spike.

## Android

A microphone foreground service needs the appropriate service type and permissions and is subject to while-in-use/background-start restrictions. Start the session through allowed user interaction and expose its persistent controls. Handle audio focus, calls, device removal, and network changes. Do not assume a UI activity can own a long-running audio engine.

Reference: [Android foreground service types](https://developer.android.com/develop/background-work/services/fgs/service-types).

## iOS

Apple's Push to Talk framework supplies system interaction and an incoming-audio wake-up mechanism using APNs. The application still supplies encoding and transport. A stock SRS deployment does not automatically provide the application's APNs service.

Design hypothesis: a small gateway may maintain SRS presence and buffer initial audio while waking a suspended client. Validate this hypothesis, the mapping of several SRS radios into the system PTT session, foreground behavior, reconnect, and first-syllable loss. Document any gateway's hosting, credentials, privacy, reliability, and costs before making it a product dependency. Investigate supported local-network wake-up options only where the deployment requires them.

An iOS build requires macOS/Xcode and device signing. Resolve GPL distribution rights before promising App Store availability. No signing assets or APNs credentials belong in the repository.

Reference: [Creating a Push to Talk app](https://developer.apple.com/documentation/pushtotalk/creating-a-push-to-talk-app).

## Linux PTT

Evaluate the [Global Shortcuts portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.GlobalShortcuts.html) on supported Wayland desktops. Validate press/release behavior and compositor support. If a device-specific input backend is needed, document its permissions and fallback. Do not silently require root or broad access to every keyboard.

## Desktop DCS integration

Standalone TOC operation is independent of running DCS. Later cockpit integration uses the existing Lua interfaces. Linux/Wine testing and access to DCS terrain/position data are separate work; a Rust Linux client is not evidence that DCS itself is natively supported there.

The opt-in [foreground keyboard adapter](KEYBOARD.md) compiles for desktop hosts and is exercised with a simulated Unix terminal. Physical Windows/Linux terminal/key/headset combinations, global/HID PTT and Wayland portal support remain unvalidated.
