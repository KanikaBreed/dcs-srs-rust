# TCP and External AWACS Mode sessions

`srs-client` is a library with runnable conformance probes. Its TCP/EAM contract is described here; [VOICE.md](VOICE.md) covers the UDP transport, session-bound PTT gate, and Opus adapter. Device I/O and session orchestration now live in the separate [TOC console](TOC_CONSOLE.md); this crate itself has no device API or reconnect worker.

## Calling the library

1. Construct `Identity` with a valid, unique SRS `ClientId` and callsign. The caller supplies identity generation/persistence. The default standalone unit ID is `100000001`, matching the stock EAM intercom offset. Recording permission defaults to false.
2. Await `Session::connect(SocketAddr, identity, options)`. It sends a spectator SYNC with disabled radios, validates the server version, and stores peers/settings. DNS resolution belongs to the caller. The default compatibility declaration is `2.4.1.0`; this is separate from the Rust package version.
3. Await `authenticate(&password)`. The server chooses Blue or Red; the client never sets a coalition before success. A rejected password leaves a spectator session available for retry. Disabled EAM prevents sending the password. Passwords are not retained in the session or included in diagnostics.
4. Start with `disabled_radios()`, configure slots, and await `set_radios(radios)`. Frequencies are Hz, slot 0 is reserved for disabled/intercom, and every update contains all 11 slots. `Radio::tuned(251_000_000.0, Modulation::AM)` is one example. Guard, simulated key, model, name, retransmit, and instructor-intercom fields are represented. Successful sending confirms a TCP write, not server receipt or voice delivery.
5. Keep awaiting `next_event()`. It applies SYNC, UPDATE, RADIO_UPDATE, disconnects, and server settings. Metadata updates retain previous radio state. It sends a full radio refresh every 60 seconds while being polled, following the stock desktop EAM refresh cadence. No background task pumps the connection for you.
6. `leave_eam().await` removes local authorization and sends both the EAM disconnect and disabled radio state. `close()` or dropping the session closes TCP. A new session must authenticate again.

Unknown message types generate an `Ignored` event. Unknown peer fields and server settings remain available as JSON/settings snapshots. Server presets are retained but are not applied to the local radios yet. When settings disable EAM, local authorization is removed and radio state resets; `EamDisabled` reports this. Revoked encryption/instructor/retransmit features are removed and broadcast through `RadiosAdjusted`. Callers must reflect these events in any future UI/PTT state.

## Cancellation, errors, and bounds

Cancelling any pending public async session operation closes TCP. This is a deliberate first API contract: partially written frames, abandoned login replies, and stale authorization cannot be reused. In particular, selecting `next_event()` against a timer and dropping it disconnects; the `srs-toc` orchestration layer owns and continuously polls the session. Internal read cancellation at a refresh deadline preserves fragmented and coalesced data.

Connect/SYNC, login response, and writes use configurable timeouts (default 10 seconds, maximum 90). Connect/SYNC has a shared receive deadline, while a pending write has its own bound. `next_event` may wait indefinitely on an idle server while sending refreshes; it does not assume the server sends echo pings. There is no automatic reconnect or mobile network-handover handling.

Malformed frames, invalid required fields, incompatible versions, transport errors, and limit violations close TCP. `state()` then returns `Disconnected`. Snapshots may contain last-known data until `close()` clears the roster; they do not prove current connectivity. Check state before use. Session, identity, and radio structures deliberately do not implement unrestricted debug dumping.

| Resource | Limit |
|---|---|
| TCP JSON frame | 1 MiB excluding LF, inherited from srs-protocol |
| Persistent roster | 1024 clients; 64 KiB serialized JSON per client; 8 MiB aggregate |
| Accumulated server settings | 4096 keys; 1 MiB serialized JSON |
| Local identity/password input | Callsign 128 UTF-8 bytes; password 1024 bytes |
| Local radio state | Exactly 11 slots, finite nonnegative frequencies, known modulation values |
| Buffered events | No queue; one event per call |

Serialized-size limits bound retained JSON content, not exact heap usage; JSON tree overhead and bounded temporary parsing buffers add memory. These are Rust resource policies, not limits promised by SRS. Very large servers need separate evaluation.

The client follows upstream's minimum-version gate (`1.9.0.0`) and .NET component ordering. Only the pinned `2.4.1.0` release has connected-server evidence here; acceptance by that gate does not establish compatibility with every release. SRS coalition credentials/control traffic are not encrypted by this protocol.

## Reproducing the stock-server test

On Linux with Rust, Python 3.11+, `unshare`, `ip`, and user/network namespaces:

```sh
python3 scripts/test_stock_server.py
```

The runner downloads the official [SRS 2.4.1.0 Linux command-line server](https://github.com/ciribob/DCS-SimpleRadioStandalone/releases/tag/2.4.1.0), verifies SHA-256 `46268dd35204d04426974f66d25eaddaebbaa6b133a7bd227675fae30ed88127`, and stores it under ignored `.scratch/reference-server`. To use an existing official binary, pass `--server /path/to/SRS-Server-Commandline-Linux`; the same digest is required.

It creates a private network namespace with only loopback, chooses free ports, launches separate EAM-enabled and EAM-disabled stock processes with synthetic config, then runs the Rust `stock_probe` example. Isolation is required because the reviewed stock TCP server binds to all addresses even when its UDP bind setting is loopback. UPnP, HTTP, exports, and transmission logging are disabled. The namespace also prevents upstream update/telemetry traffic. Logs and config remain ephemeral; child servers are terminated in cleanup. No binaries or upstream logs are committed.

CI opts into `SRS_TEST_SUDO_NAMESPACE=1` for hosts that prohibit unprivileged user namespaces. This elevates only the namespace runner on the disposable hosted Linux machine. The test does not fall back to running the stock server on the host network.

The probe asserts SYNC version, wrong-password rejection, Blue and Red authentication, radio updates observed by another Rust endpoint through the server, AM/FM, guard/key fields, EAM leave/reset, peer disconnection, and rejection of an old client version. Disabled-mode testing checks both the client's refusal to send credentials and the stock server's coalition-0 response to a direct synthetic request. It handles unrelated interleaved broadcasts while awaiting replies.

This establishes control interoperability with an unmodified server. The runner additionally tests generated Opus through UDP in both directions; see [VOICE.md](VOICE.md) for measurements and limits. Full stock-client audio interoperability, human listening, PTT hardware, and Android/iOS execution remain pending.
