# Protocol fuzz targets

These targets call the public packet and TCP framing APIs. They are separate from the normal workspace, so libFuzzer is not an application dependency. Linux CI compile-checks both targets; this is not a claim of a completed fuzz campaign.

Install cargo-fuzz and a nightly Rust toolchain to run coverage-guided fuzzing:

```sh
cargo install cargo-fuzz --locked
rustup toolchain install nightly
cargo +nightly fuzz run voice -- -max_len=65536 -max_total_time=60
cargo +nightly fuzz run control -- -max_len=8192 -max_total_time=60
```

Run these commands from the repository root. Seed `fuzz/corpus/voice/` with the committed `.bin` fixtures and `fuzz/corpus/control/` with the `.jsonl` fixtures from [the protocol corpus](../crates/srs-protocol/tests/fixtures/README.md). Corpus growth and crash artifacts stay ignored; add a minimal reproducer as a regression test when fixing a finding.

For a build-only check without a nightly toolchain:

```sh
cargo check --manifest-path fuzz/Cargo.toml --bins --locked
```

Normal tests already exercise every single-byte mutation of three independent UDP vectors, all truncations, oversized input, JSON nesting, and fragmented/coalesced TCP frames. These deterministic checks complement fuzzing; they do not replace a campaign.
