# Desktop audio adapter and probe

`srs-device` adds explicit native input/output ownership, bounded callback bridges, and a local audio probe. Windows, Linux, and macOS builds use CPAL's default WASAPI, ALSA, and CoreAudio hosts respectively. Physical headset/hotplug/listening validation is pending. The local probe has no network connection or hardware/global PTT controls. The separate [TOC console](TOC_CONSOLE.md) now integrates these bridges with SRS and timed transmit commands.

## Build and run intentionally

Build with Rust, a native C compiler, CMake, and Python as described in the README. Linux additionally needs ALSA development headers and pkg-config (Debian/Ubuntu: `libasound2-dev pkg-config`). ALSA can use a desktop server's configured compatibility device; direct CPAL PipeWire/PulseAudio backends are not enabled here. The probe never changes system routing, mixer volume, or device permissions.

```sh
cargo build --locked -p srs-device --bin audio_probe
cargo run --locked -p srs-device --bin audio_probe -- --help
cargo run --locked -p srs-device --bin audio_probe -- devices
```

Startup/help opens no devices. `devices` enumerates metadata/capabilities without starting capture/playback. It prints native device identifiers locally; avoid pasting personal device metadata into public reports. Use a listed ID instead of `default` to select a particular device.

These commands deliberately activate audio for the specified duration (1–30 seconds):

```sh
cargo run --locked -p srs-device --bin audio_probe -- tone --output default --seconds 3
cargo run --locked -p srs-device --bin audio_probe -- capture --input default --seconds 5
cargo run --locked -p srs-device --bin audio_probe -- loopback --input default --output default --seconds 5
```

Tone plays a generated 700 Hz signal at 10% full-scale with a short fade. Capture reports frame count, RMS, clipping count, and dropped blocks; it does not play or save the microphone. Loopback captures your voice, converts to SRS input, runs Opus encode/decode, and plays it locally. Use headphones for loopback to avoid speaker feedback. No command records audio or contacts a server. The process prints its active mode before opening streams and releases them on exit. Ctrl+C terminates the process and closes its devices; there is no network PTT to leave active.

Only **48 kHz, one or two channels, f32/i16/u16** configurations are accepted. The adapter prefers float, mono input, and stereo output; it averages input channels and duplicates mono output. It requests a 480-sample/10 ms period where advertised, otherwise uses the backend default. Unsupported rates/channels/formats return an error. There is no automatic fallback to another device or silent 44.1 kHz conversion. Startup requests a three-second backend timeout, but CPAL documents that some backends do not honor it.

## Callback and worker contract

The adapter follows the existing SRS 16 kHz/640-sample encoder and 48 kHz/1920-sample decoder contract. Upstream desktop `AudioManager.cs` at `d01f540ce5e363e9ac841775c8f7e2253138cd81` was inspected for capture/codec boundaries. The Rust callback bridges and fixed-ratio FIR are new code, not a port of its WASAPI/EventDrivenResampler implementation.

| Stage | Bound and failure behavior |
|---|---|
| Input callback | Converts and averages into 480-sample mono blocks; eight preallocated queue slots (80 ms) |
| Capture overflow | Drops the new block and advances its sequence; the worker resets partial PCM and filter state across gaps |
| Capture permit | Explicit `begin`, then `renew` before 250 ms; expiry requires a fresh begin and cannot resume an old frame |
| Capture device stall | No nonempty capture callbacks for 250 ms while armed permanently stops the bridge, even if its controller keeps renewing |
| Capture handoff | Drops blocks aged 80 ms or more; rechecks a completed frame after conversion and before use |
| Resampling | Worker-only 97-tap Blackman-windowed sinc, 7 kHz cutoff, 48-to-16 kHz decimation; 1 ms filter group delay |
| Output queue | Three complete 1920-sample/40 ms frames; enqueue is all-or-nothing |
| Output overflow | Returns `QueueFull` and changes generation so the callback discards its backlog |
| Output stale/flush | Drops frames aged 80 ms or more and invalidates partially played frames on the next callback |
| Callback size | At most 4800 frames/100 ms, with complete mono/stereo sample groups; invalid sizes stop capture/output and output remains silent |
| Native error/route change | Sets a terminal stop flag; callbacks discard capture/silence playback, and worker operations return errors |
| Owner drop | Stops the associated bridge and drops the native stream; reopening is always explicit |

A capture permit belongs to one bridge instance and generation. A frame from a released permit or a previous device instance cannot validate after reopening. `CaptureReader::validate_frame` must be called after encoding and immediately before network handoff. Device errors and ownership/lifecycle loss must also release the separate network PTT gate; the device library does not own a `VoiceConnection`.

The `srs-toc` session worker acquires a fresh capture permit together with `press_ptt`, renews only while the input owner remains active, and calls both `CaptureReader::release` and `VoiceConnection::release_ptt` on release/interruption/error. It feeds `poll_mix` output to the playback writer and flushes it on policy/session changes. TCP polling stays persistent; cancelling a control read closes the session. This orchestration and an equal-gain mono mix are implemented in `srs-toc`; keyboard/HID hold-to-talk remains pending.

The data-callback bodies perform format conversion, bounded copying, clock reads, atomic operations, and SPSC push/pop only. They do not enter an async runtime, allocate/free, log, perform file/network I/O, run codecs/FIR, or take mutexes. The error callbacks only set a stop flag. `rtrb` preallocates its storage. All stream construction, buffer allocation, error reporting, and codec work occur outside data callbacks. Tests instrument the Rust bridge callback paths to assert zero heap allocations/deallocations; this does not establish hard real-time guarantees for CPAL, the OS, or a driver.

Bridge calls use monotonic `Instant` timestamps from the same clock. A concurrent producer can enqueue after a consumer samples that clock; assembly/playback treats that small timestamp lead as zero age. The pre-send frame check uses a fresh timestamp. Timestamps are callback arrival times, not calibrated ADC/DAC timestamps. Application age limits exclude hidden device/OS buffers. A flush/stop cannot retract samples already submitted to hardware, and concurrent state changes take effect by the next callback. Adaptive sample-rate conversion, independent capture/playback clock drift, effects, gain/mute/stereo routing, and 44.1 kHz support remain pending.

## Automated evidence

```sh
python3 scripts/check.py
python3 scripts/test_null_audio.py
cargo run --locked -p srs-device --example bridge_probe
```

The normal tests open no native devices. They cover fragmented callbacks, format conversion/downmix, stereo output, filter response, Opus round trips, queue overflow, stale/partial buffers, permit expiry, device restart identity, callback stalls, owner drop, invalid configuration, and callback allocation counts. A 700 Hz tone survives conversion, a 6 kHz tone remains in the passband, and tested 12 kHz input is attenuated by over 59 dB to prevent aliasing into 4 kHz speech audio.

On Linux, `test_null_audio.py` creates a temporary complete ALSA configuration containing only a null PCM and passes its path only to child probe processes. It never changes the user's ALSA files or opens physical microphones/speakers. Tone and capture exercise native open/start, callbacks, frame processing, and process exit. Null PCM is unpaced: its overflow/underrun counts are expected and do not measure hardware timing or audio quality. Each child has a 15-second process timeout.

`bridge_probe` uses a simulated clock and generated samples, with no native devices or network. A local Linux x86-64 debug run processed 40,000 capture blocks through 10,000 codec frames with zero drops, three startup silence callbacks, and a maximum 30 ms age from first-block callback to assembled frame. It took 6.617 seconds wall time, 6.588 seconds user CPU, under 0.001 seconds system CPU, and 14,612 KiB process peak RSS (Python `resource.getrusage(RUSAGE_CHILDREN)`). This is offline worker cost for 400 seconds of simulated single-voice audio, not microphone-to-speaker latency, hardware underrun evidence, or mobile power use.

## Physical validation still required

On Windows and Linux, select a headset, run the tone, then capture, then headphone loopback. Confirm intelligibility and expected channels, check that capture reports nonzero speech level, and unplug the active device during a probe. Expected behavior is termination/error with no automatic restart; reopening requires another explicit command. Record OS, device/rate/format, command, counts, perceived clipping/delay, and any failure. Check permission denial, native route changes, and process termination as separate cases. macOS testing likewise requires intentional microphone permission and an actual device.

A02 remains incomplete until physical checks and integrated SRS/PTT behavior are validated. Android/iOS audio sessions, permission/lifecycle adapters, headset controls, packaging, and UI remain separate tasks.

References: [CPAL 0.18.2](https://docs.rs/cpal/0.18.2/cpal/), [CPAL stream construction/timeout contract](https://docs.rs/cpal/0.18.2/cpal/traits/trait.DeviceTrait.html), [rtrb 0.4.1](https://docs.rs/rtrb/0.4.1/rtrb/), and [allocation-counter](https://docs.rs/allocation-counter/0.8.1/allocation_counter/).
