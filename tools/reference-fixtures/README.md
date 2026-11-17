# Upstream C# fixture oracle

This .NET 10 console program links the pinned upstream SRS wire implementation and its client/radio models directly from an external clean checkout. No upstream source copy is embedded here. It produces deterministic synthetic fixtures and can verify Rust-generated output through the original implementation.

Use [scripts/reference_fixtures.py](../../scripts/reference_fixtures.py), which checks the source revision and published unit-test vectors. Fixture provenance and commands are in the [corpus guide](../../crates/srs-protocol/tests/fixtures/README.md).

`Shims.cs` replaces only NLog and updater infrastructure. Logging errors become exceptions, and the entry assembly version is pinned for deterministic fixture output. Packet layout, JSON serializer settings, numeric enums, client models, and radio models are upstream code compiled unchanged.

A matching packet/JSON fixture is not a connected-session or voice-codec interoperability test. The command opens no sockets or audio devices. .NET and the upstream checkout are needed only for this optional local check and the Linux CI oracle, not for normal Rust builds/tests.
