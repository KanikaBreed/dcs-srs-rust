//! SRS wire formats, independent of sockets, audio devices, and UI.
//!
//! Packet layout and control fields follow Ciribob and contributors' GPLv3
//! DCS-SimpleRadioStandalone revision d01f540ce5e363e9ac841775c8f7e2253138cd81.
//! See the repository NOTICE.md and this crate's fixture provenance.
//!
//! These codecs do not establish a connection or claim live SRS interoperability.

pub mod control;
pub mod voice;

/// Exact upstream revision used by the independent fixture generator.
pub const REFERENCE_REVISION: &str = "d01f540ce5e363e9ac841775c8f7e2253138cd81";
