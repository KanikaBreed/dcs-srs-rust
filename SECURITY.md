# Security and privacy

This pre-alpha repository has no supported application releases. Do not publish credentials, private packet captures, APNs tokens, or other users' voice recordings in issues.

Private vulnerability reporting is enabled. For sensitive findings, use [Report a vulnerability](https://github.com/KanikaBreed/dcs-srs-rust/security/advisories/new). If that option becomes unavailable, open an issue containing only a request for a private reporting channel, without exploit details or secrets.

The implementation plan includes bounded network parsing, audited codec/native interfaces, explicit microphone/PTT ownership, redacted diagnostics, and intentional recording/export. These are requirements, not claims that a complete application has passed a security audit.

SRS radio encryption simulates key matching and is not cryptographic transport confidentiality. Compatibility with existing SRS servers does not add transport security by itself. Any future gateway must document its trust boundary, data retention, credentials, and authentication.
