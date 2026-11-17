# Receive eligibility and bounded playout

The receive worker now checks standalone radio eligibility, orders encoded frames, and decodes one 40 ms PCM frame at a time. It opens no speakers. This is the foundation for a device output adapter, not a complete audio engine.

## Calling the worker

Keep the TCP control reader running. Forward `Event::ClientDisconnected(id)` to `voice.forget_speaker(id)` so a peer reconnecting with the same identity can restart its sequence; this clears only that speaker and preserves local PTT. For each UDP `VoiceEvent::Voice(packet)`, call `voice.buffer_voice(&packet)`. `QueueOutcome` distinguishes queued/resynchronized data, duplicates, late frames, ineligible radio/key, stale handoff, and speaker capacity. Poll `voice.poll_audio(&mut pcm)` at `voice.audio_deadline()` while continuing to poll UDP events. The [stock probe](../crates/srs-client/examples/voice_probe.rs) provides a working select loop.

A returned `Playout` identifies the original speaker, radio slot, guard flag, sequence, and decoded/concealed status. PCM is 1920 signed 16-bit mono samples at 48 kHz. No frame or error leaves the output buffer zeroed. Calls may allocate/free buffers and initialize or invoke libopus: they belong on a worker, never in a device callback. Later mixing/output queues must have their own bounds and flush on policy/session changes; an already returned frame cannot be recalled by this API.

Each packet carries its receiving connection identity and authorization revision. Retuning, EAM leave/revocation, relevant settings changes, TCP closure/cancellation, or UDP expiry invalidate pending audio before any further output. IRL-radio PTT transitions also clear pending audio, including a press/release between polls. Packets held by the caller for 120 ms or more, or from an older authorization revision, are rejected. Network queues held by the OS do not carry a send timestamp; newly received datagrams are evaluated against current radio policy.

## Compatibility rules

The reference is Ciribob and contributors' GPLv3 revision `d01f540ce5e363e9ac841775c8f7e2253138cd81`. Reviewed files: `Common/Models/Player/PlayerRadioInfoBase.cs`, `RadioBase.cs`, desktop `DCSPlayerRadioInfo.cs`, `UDPClientAudioProcessor.cs`, and `Common/Network/Singletons/SyncedServerSettings.cs`. See [notices](../NOTICE.md).

- Primary frequencies require the same modulation, receive frequency above 10 kHz, and an absolute difference strictly below 500 Hz. Disabled radios never match.
- Guard uses the same tolerance/frequency floor but does not require matching modulation, following the stock client.
- Intercom requires a positive matching unit ID or the receiving radio's instructor intercom override. Matching intercom bypasses simulated key checks.
- Matching keys permit clear audio. With strict encryption disabled, unencrypted transmissions are also clear on keyed receivers. Wrong-key audio is dropped until simulated noise/effects exist; it is never decoded into clear speech.
- Global frequencies are parsed from comma-separated MHz settings, with exact Hz membership as in the desktop client. Incoming keys are treated as zero there, but strict receive-key checks still apply. Invalid/nonfinite entries are ignored; more than 64 usable entries closes the control session with a resource-limit error.
- `IRL_RADIO_TX` blocks reception on the held AM/FM/HAVEQUICK/MIDS/SINCGARS transmit radio. The stock post-match exceptions for intercom, MIDS, and global frequencies are retained.
- Coalition/spectator routing remains the connected server's responsibility, preserving its global/test-frequency exceptions. Standalone TOC has no terrain/location model; DCS distance/line-of-sight simulation remains pending.

The independent C# harness now emits 46 matcher cases in [radio-matching.json](../crates/srs-protocol/tests/fixtures/radio-matching.json), using the unchanged upstream `PlayerRadioInfoBase.CanHearTransmission`. Its desktop counterpart was inspected for equivalence. Cases cover tolerance boundaries, guards, keys/strictness, disabled radios, intercom, blocked radios, and candidate ordering.

One intentional correction is pinned by a regression case: upstream's fallback guard can inherit an earlier blocked primary radio's `decryptable = true`, even though the guard's key is wrong. Rust recomputes the flag for that guard. The fixture records the original C# result and the test explicitly asserts this single difference.

Initial TOC output chooses the first clear eligible packet frequency and the upstream radio search order. It emits one copy per original speaker/sequence. Selected-radio and volume scoring, multi-radio interference, static, recording, effects, retransmission, and simultaneous-radio mixing remain pending. Server settings for those effects are retained but their DSP is not implemented.

## Timing, limits, and overflow policy

The Rust scheduler is a new fixed-delay implementation. It does not reproduce upstream's adaptive Speex jitter implementation or its decode-before-buffer order. Encoded frames are reordered before Opus decoding to preserve codec state.

| Bound | Behavior |
|---|---|
| Initial delay | 80 ms; two 40 ms frames of reorder tolerance |
| Packet queue | At most 8 encoded frames per original speaker, each at most 4000 bytes |
| Speakers | At most 16 decoder/queue states; reject newcomers while all slots were active in the last 2 seconds |
| Idle speaker eviction | A new speaker may replace an idle slot; no unbounded identity map |
| Missing frame | Explicit Opus PLC for at most two consecutive 40 ms frames, including burst tails |
| Longer hole | Reset decoder and restart at the earliest queued frame after 80 ms |
| Worker stall | At 120 ms past the scheduled frame, discard the backlog and reset |
| Forward sequence jump | At 8 frames or more beyond expected, discard backlog and restart at the new sequence |
| Silence | Reset codec on a new accepted packet after 400 ms without arrivals |
| Late/duplicate relay | Deduplicate by original identity/sequence before decoding; retain sequence high-water marks for the lifetime of that stream slot |
| Sequence maximum | No wraparound; exhausted sequence stops scheduling |

The encoded payload ceiling is 512,000 bytes across all speakers, plus bounded map entries, codec state, fixed buffers, and receive metadata. Old high-water marks are forgotten when a slot is evicted or authorization resets. SRS has no cryptographic replay protection or explicit talkspurt identifier, so this is bounded real-time deduplication, not persistent replay detection. A sequence reset under the same identity needs its disconnect event forwarded with `forget_speaker`, a new local receive context, or an evicted slot.

Ordinary timer overshoot keeps the original cadence. If a complete frame deadline was missed, the next deadline is rebased so repeated polls cannot drain a catch-up burst. At most one speaker frame is returned per call; poll again for another due speaker. Once the two-frame tail is exhausted, there is no audio wakeup until another packet arrives. Concealment can predict packet numbers never sent during PTT silence. An idle restart retains only observed sequence numbers, so those predictions do not reject the next burst. Very short pauses cannot always be distinguished from late packets without a protocol talkspurt marker. Adaptive delay, skew estimation, resampling, mixing/clipping, and multi-hour WAN soak tests remain pending.

## Reproducible validation

```sh
python3 scripts/check.py
python3 scripts/reference_fixtures.py --reference /path/to/DCS-SRS-reference
python3 scripts/test_stock_server.py
cargo run --locked -p srs-audio --example jitter_probe
```

Unit simulations exercise 100-frame loss/reorder/duplicate schedules, exact decoder-order equivalence, bounded concealment, long pauses, sequence extremes, timer drift, capacity, and invalid inputs. The 10% loss schedule produces 90 decoded and 10 concealed frames at fixed 40 ms spacing. Session tests verify that a strict-encryption update clears queued clear audio without retuning, and malformed policy closes reception.

The stock-server probe uses this worker for 25 generated frames in each direction, guard playout, and wrong-key rejection. It prints encode-start-to-PCM-playout latency, spacing error, and worker duration. A local Linux debug run measured p50 80.8–82.1 ms and p95 81.8–82.6 ms with no missing/concealed frames in those bursts. These include the 80 ms software buffer, but exclude capture, device buffers, speakers, and WAN conditions. CI logs provide repeatable results for the exact commit.

`jitter_probe` processes 10,000 frames for each of 16 synthetic speakers, with 10% loss and duplicates. Its clock is simulated: elapsed process time measures offline worker cost, not 400 seconds of real-time device operation. It reports decoded/concealed totals, peak queued packet/payload counts, and wall time. Memory/CPU measurements must name the build profile and host; this probe is not mobile-power or full-application evidence.

A local Linux x86-64 debug run of the load probe produced 144,000 decoded and 16,000 concealed frames, with 48 queued packets/4,992 payload bytes at peak. It used 3.05 seconds wall time, 3.03 seconds user CPU, 0.002 seconds system CPU, and a process peak RSS of 14,352 KiB (`resource.getrusage(RUSAGE_CHILDREN)` around the standalone executable). The reused generated frame and simulated time isolate scheduler/codec cost; these are host-specific observations, not device or release targets.
