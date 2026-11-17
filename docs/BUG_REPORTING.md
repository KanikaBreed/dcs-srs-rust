# Bug reports and diagnostics

There is no installable application yet. Report scaffold/tooling issues through the repository's bug form. For later application reports, include:

- Commit/build version, OS version, device model, and the affected radio workflow.
- Reproduction steps, expected result, actual result, and frequency of failure.
- SRS server/client versions and relevant policy settings, without credentials or private server addresses.
- Audio input/output type, Bluetooth/wired/USB route, and whether the app was foreground, background, or locked.
- For PTT: touch/keyboard/HID/system control, press/release sequence, and whether another receiver confirmed transmission.
- A minimal redacted diagnostic excerpt, with timestamps relative to the failure.

Do not upload full logs by default. Review screenshots and captures for callsigns, addresses, passwords, device IDs, personal paths, and voice content. Prefer synthetic audio and test-server reproductions. A reporter's claim is evidence to investigate, not proof of a reproduced defect.

A future diagnostic panel should summarize connection, authentication, device route, sample rates, queue depth, dropped packets, underruns, and PTT state. It must redact sensitive values and require an intentional export. No diagnostics panel or automatic crash uploader exists today.

Sensitive findings follow [SECURITY.md](../SECURITY.md).
