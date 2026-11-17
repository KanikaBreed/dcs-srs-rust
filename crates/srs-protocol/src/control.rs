//! Control envelope ported from `Common/Models/NetworkMessage.cs` by Ciribob
//! and contributors (GPLv3), revision d01f540ce5e363e9ac841775c8f7e2253138cd81.
//! TCP framing follows its newline terminator. Partial input and serialized
//! output are bounded; no sockets, async runtime, or authentication are included.

use std::{collections::BTreeMap, error::Error, fmt, io};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Local resource limit excluding the newline, not a limit specified by SRS.
pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

/// Integer values match the C# enum. Unknown values survive decoding for callers
/// to ignore or reject explicitly instead of being mistaken for UPDATE.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct MessageType(pub i32);

impl MessageType {
    pub const UPDATE: Self = Self(0);
    pub const PING: Self = Self(1);
    pub const SYNC: Self = Self(2);
    pub const RADIO_UPDATE: Self = Self(3);
    pub const SERVER_SETTINGS: Self = Self(4);
    pub const CLIENT_DISCONNECT: Self = Self(5);
    pub const VERSION_MISMATCH: Self = Self(6);
    pub const EXTERNAL_AWACS_MODE_PASSWORD: Self = Self(7);
    pub const EXTERNAL_AWACS_MODE_DISCONNECT: Self = Self(8);
    pub const GATEWAY_CLIENT_FULL_UPDATE: Self = Self(9);
    pub const GATEWAY_CLIENT_METADATA_UPDATE: Self = Self(10);
    pub const GATEWAY_CLIENT_DISCONNECT: Self = Self(11);
}

/// Wire envelope only. Nested client/radio models remain JSON until typed domain
/// models are introduced. JSON member order/escaping is not a wire invariant.
#[derive(Clone, Deserialize, PartialEq, Serialize)]
pub struct ControlMessage {
    #[serde(rename = "MsgType")]
    pub message_type: MessageType,
    #[serde(rename = "Version", skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(rename = "Client", skip_serializing_if = "Option::is_none")]
    pub client: Option<Map<String, Value>>,
    #[serde(rename = "Clients", skip_serializing_if = "Option::is_none")]
    pub clients: Option<Vec<Map<String, Value>>>,
    #[serde(rename = "ServerSettings", skip_serializing_if = "Option::is_none")]
    pub server_settings: Option<BTreeMap<String, String>>,
    #[serde(
        rename = "ExternalAWACSModePassword",
        skip_serializing_if = "Option::is_none"
    )]
    pub external_awacs_mode_password: Option<String>,
    // Private so callers cannot introduce duplicate reserved keys during encoding.
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

impl fmt::Debug for ControlMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Passwords and nested client/settings fields may contain sensitive data.
        f.debug_struct("ControlMessage")
            .field("message_type", &self.message_type)
            .finish_non_exhaustive()
    }
}

impl ControlMessage {
    /// Version is explicit: our Cargo package version is not an SRS version.
    pub fn new(message_type: MessageType, version: impl Into<String>) -> Self {
        Self {
            message_type,
            version: Some(version.into()),
            client: None,
            clients: None,
            server_settings: None,
            external_awacs_mode_password: None,
            extensions: BTreeMap::new(),
        }
    }

    pub fn extensions(&self) -> &BTreeMap<String, Value> {
        &self.extensions
    }

    /// Decode one JSON document (the newline may be stripped or included).
    pub fn decode_json(bytes: &[u8]) -> Result<Self, ControlError> {
        let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(ControlError::FrameTooLarge);
        }
        serde_json::from_slice(bytes).map_err(|err| ControlError::InvalidJson {
            line: err.line(),
            column: err.column(),
        })
    }

    /// Serialize into a bounded buffer and append exactly one LF terminator.
    pub fn encode(&self) -> Result<Vec<u8>, ControlError> {
        let mut output = LimitedBuffer {
            bytes: Vec::new(),
            exceeded: false,
        };
        if serde_json::to_writer(&mut output, self).is_err() {
            return Err(if output.exceeded {
                ControlError::FrameTooLarge
            } else {
                ControlError::EncodingFailed
            });
        }
        output.bytes.push(b'\n');
        Ok(output.bytes)
    }
}

struct LimitedBuffer {
    bytes: Vec<u8>,
    exceeded: bool,
}

impl io::Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_FRAME_BYTES - self.bytes.len() {
            self.exceeded = true;
            return Err(io::Error::other("control frame exceeds limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlError {
    InvalidLimit,
    FrameTooLarge,
    /// Only location is retained, to avoid echoing credentials from JSON errors.
    InvalidJson {
        line: usize,
        column: usize,
    },
    EncodingFailed,
    TruncatedFrame,
    DecoderFailed,
}

impl fmt::Display for ControlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLimit => {
                f.write_str("control frame limit must be between 1 byte and 1 MiB")
            }
            Self::FrameTooLarge => f.write_str("control frame exceeds configured limit"),
            Self::InvalidJson { line, column } => {
                write!(f, "invalid control JSON at line {line}, column {column}")
            }
            Self::EncodingFailed => f.write_str("unable to encode control message"),
            Self::TruncatedFrame => f.write_str("stream ended before a newline terminator"),
            Self::DecoderFailed => {
                f.write_str("decoder failed; create a new decoder for a new connection")
            }
        }
    }
}
impl Error for ControlError {}

/// One decoder per connection. It emits one message at a time and never queues
/// an unbounded batch. Any framing/JSON error permanently fails this decoder.
#[derive(Default)]
pub struct ControlDecoder {
    pending: Vec<u8>,
    limit: Option<usize>,
    failed: bool,
}

impl ControlDecoder {
    pub fn with_limit(limit: usize) -> Result<Self, ControlError> {
        if !(1..=MAX_FRAME_BYTES).contains(&limit) {
            return Err(ControlError::InvalidLimit);
        }
        Ok(Self {
            limit: Some(limit),
            ..Self::default()
        })
    }

    pub fn buffered_len(&self) -> usize {
        self.pending.len()
    }

    /// Consume through at most one LF, leaving later frames in `input`.
    /// `None` means all input was consumed and a terminator is still needed.
    /// CRLF is accepted because CR is legal JSON trailing whitespace.
    pub fn decode(&mut self, input: &mut &[u8]) -> Result<Option<ControlMessage>, ControlError> {
        if self.failed {
            return Err(ControlError::DecoderFailed);
        }
        let limit = self.limit.unwrap_or(MAX_FRAME_BYTES);
        let remaining = limit - self.pending.len();
        // Inspect at most the available frame budget plus one terminator byte.
        let inspected = &input[..input.len().min(remaining + 1)];
        let newline = inspected.iter().position(|b| *b == b'\n');
        let take = newline.unwrap_or(inspected.len());
        if take > remaining {
            self.fail();
            return Err(ControlError::FrameTooLarge);
        }
        self.pending.extend_from_slice(&input[..take]);
        *input = &input[take + usize::from(newline.is_some())..];
        if newline.is_none() {
            return Ok(None);
        }
        let message = ControlMessage::decode_json(&self.pending);
        self.pending.clear();
        match message {
            Ok(message) => Ok(Some(message)),
            Err(err) => {
                self.fail();
                Err(err)
            }
        }
    }

    /// Call when EOF is observed; an unterminated JSON document is not a frame.
    pub fn finish(self) -> Result<(), ControlError> {
        if self.failed {
            return Err(ControlError::DecoderFailed);
        }
        if self.pending.is_empty() {
            Ok(())
        } else {
            Err(ControlError::TruncatedFrame)
        }
    }

    fn fail(&mut self) {
        self.pending.clear();
        self.failed = true;
    }
}
