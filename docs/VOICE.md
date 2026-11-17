# UDP voice and codec foundation

The implementation sends and receives generated Opus frames through an unmodified SRS 2.4.1.0 server. It does not open microphones or speakers. This is transport/codec evidence, not a usable voice application or human listening validation.

## API and ownership

Configure and authenticate a [TCP session](SESSION.md), set its radios, and call `session.open_voice().await`. The returned `VoiceConnection` is tied to that exact TCP connection and can be driven independently of the control reader. There are no background workers or unbounded queues. Callers must keep both control and UDP receive operations running.

UDP registration sends the 22-byte client identity, retries every second until the operation deadline, and requires an echo of the same identity from the connected server. Registration establishes reachability; the stock server also echoes unauthenticated identities. It does not establish coalition authorization.

After registration, `next_event()` sends keepalives every 15 seconds. Valid voice packets or matching identity echoes refresh a 42-second deadline. Invalid datagrams do not. A timeout requires opening a new UDP connection; it never resumes an earlier PTT action automatically. The OS UDP socket is connected to the chosen server endpoint, filtering other source endpoints.

`press_ptt(slot)` requires an authenticated session and an enabled, valid local radio. `send_frame(opus)` sends on that radio, with its frequency, modulation, simulated key, and unit/intercom identity. Each connection starts sequence numbering at 1; original and sending IDs are the local identity and relay count is 0. There is no relay or simultaneous multi-radio transmission yet. `release_ptt()` stops transmission immediately.

PTT is invalidated by EAM leave/revocation, a local radio update, relevant server policy changes, control connection closure or cancellation, UDP timeout, invalid encoded input, send errors, and cancelled sends. Reauthentication or retuning requires a new press. TCP liveness is signaled when the transport is dropped, even while its owning session method is cancelled. A 40 ms UDP send deadline drops an expired frame and releases PTT; no old frame is queued for later transmission. The caller must still supply fresh captured/encoded input and release PTT on lost input ownership or device failure when those adapters are implemented.

UDP `next_event()` is cancellation-safe and can be selected against other work. Cancelling it does not represent a PTT release. TCP `Session::next_event()` retains its stricter contract: cancelling it closes TCP and invalidates voice. Future application orchestration must keep a persistent control reader instead of cancelling it for each button event.

## Receive and codec limits

The receive socket reads into a fixed 64 KiB buffer to avoid accepting a truncated oversized UDP packet. Datagrams above 8 KiB, empty audio/frequencies, malformed lengths, and audio above 4000 bytes are rejected. Accepted payloads are returned one at a time with identity, sequence, radio fields, and receive time; there is no application receive queue.

`ReceivedVoice` contains raw encoded data. It is **not permission to play audio**. The stock server can forward frames that the client should render as undecryptable radio noise, and global/test frequencies have special rules. Receive-radio eligibility, simulated-key effects, sender-specific sequencing, jitter buffering, mixing, terrain/distance rules, and playback are not wired yet. No received frame is automatically played.

`srs-audio::Encoder` uses Opus VOIP, 16 kHz mono, 640 signed 16-bit samples per 40 ms input frame, and disabled in-band FEC, matching the reviewed stock client settings. Default codec bitrate remains automatic. Encoding uses a caller-owned 4000-byte output buffer. `Decoder` produces 1920 mono samples at 48 kHz and rejects packets of other durations. Empty input is rejected; `conceal()` explicitly requests one 40 ms loss-concealment frame. `reset()` resets stream state.

Create one codec state per stream, outside audio callbacks. Network methods, allocations, codec initialization, and codec processing belong on workers; this implementation makes no hard real-time callback guarantee. A future jitter buffer must decide when to conceal, reset, discard old frames, or reorder. The adapter does not infer loss from sequence gaps itself.

The safe `opus` 0.4 binding uses `opusic-sys` 0.7.7, which bundles libopus 1.6.1. CMake and a C compiler are required on all three desktop build hosts. Native FFI is confined to those dependencies; workspace Rust still forbids unsafe code. Mobile toolchains and native codec packaging remain unvalidated. See [notices](../NOTICE.md) and [libopus copyright/license](../licenses/opus-COPYING.txt).

## Reproducible evidence

```sh
python3 scripts/check.py
python3 scripts/test_stock_server.py
python3 scripts/test_reference_codec.py --reference ../DCS-SRS-reference
```

The stock-server runner requires Linux network namespaces as described in [SESSION.md](SESSION.md). It now runs `voice_probe` in the enabled-EAM scenario. Two Blue endpoints exchange 25 generated 700 Hz frames in each direction at 40 ms spacing; all frames must arrive in order and decode with signal energy. An opposing Red endpoint must receive no voice. An untuned Blue receiver must receive no voice, while tuning guard to the sender must restore reception. EAM leave and TCP close invalidate held PTT/readiness.

For each direction, the probe prints frame count, missing-frame count, encode-start-to-decode-complete p50/p95/max time, mean absolute interarrival-spacing error from 40 ms, and mean encode/decode duration. These are debug-build measurements in one machine's isolated loopback network. They exclude capture accumulation, hardware latency, playout buffering, resampling outside the codec, and audible output. They are not end-to-end microphone-to-speaker latency, WAN loss/jitter evidence, or release performance targets. CI logs bind measurements to the tested source revision.

The independent codec runner requires .NET SDK 10, Linux `libopus.so.0` (e.g. the `libopus0` package), and a clean source checkout pinned to `d01f540ce5e363e9ac841775c8f7e2253138cd81`. It links the original `Common/Audio/Opus/API.cs`, `Core/OpusEncoder.cs`, and `Core/OpusDecoder.cs` unchanged. Only native-library resolution and the generated-signal driver are supplied by the harness. Native codec versions are printed. Twenty frames cross each way; both decoders must produce the expected duration, signal energy, and 700 Hz spectral component. The temporary generated corpus is removed afterward. This tests original codec-wrapper interoperability, not the full stock desktop client.

Automated tests also cover dropped-registration retry, matched keepalives, 15/42-second timing, packet bounds, identities/sequence fields, PTT release, cancellation, authorization revision changes, explicit PLC, wrong-duration input, and TCP cancellation invalidating an existing UDP transmitter. No microphone, playback, recordings, shared server, or physical device is used.

## Remaining milestone work

A01 remains awaiting full validation: stock desktop-client voice exchange, receive eligibility, a bounded jitter/sequence pipeline, impairment tests (loss, duplication, reordering, long pauses), and measured playout behavior. A02 adds intentional device capture/playback and hardware PTT with human listening. Mobile lifecycle and UI work remain separate milestones.
