//! Shared radio-domain foundation for DCS SRS Rust.
//!
//! This bootstrap intentionally exposes no application API. Radio state,
//! eligibility rules, and PTT transitions will be introduced with independent
//! compatibility evidence. Networking, audio device I/O, UI frameworks, and
//! mobile lifecycle objects belong outside this crate.
