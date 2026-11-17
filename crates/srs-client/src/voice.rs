//! UDP identity/heartbeat behavior follows Common/Network/Client/UDPVoiceHandler.cs,
//! Ciribob and contributors (GPLv3), upstream d01f540c. Adds bounded datagrams,
//! matched keepalive identities, session-bound PTT, and explicit timeout errors.
//! Receive eligibility and bounded worker playout live in the receive module.

#[cfg(test)]
#[path = "voice_tests.rs"]
mod tests;

use srs_protocol::voice::{ClientId, Datagram, Frequency, VoicePacket};
use std::{
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};
use tokio::{
    net::UdpSocket,
    sync::watch,
    time::{Instant, sleep_until, timeout_at},
};

pub const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(15);
pub const VOICE_TIMEOUT: Duration = Duration::from_secs(42);
pub const SEND_DEADLINE: Duration = Duration::from_millis(40);
const REGISTRATION_RETRY: Duration = Duration::from_secs(1);
pub const MAX_VOICE_DATAGRAM: usize = 8192;
pub const MAX_AUDIO_BYTES: usize = 4000;

#[derive(Clone, Copy)]
pub(crate) struct Channel {
    pub frequency: Frequency,
    pub unit_id: u32,
}

#[derive(Clone)]
pub(crate) struct Authorization {
    pub revision: u64,
    pub receive: Option<std::sync::Arc<crate::receive::Policy>>,
    pub channels: [Option<Channel>; 11],
}
impl Authorization {
    pub fn disabled() -> Self {
        Self {
            revision: 0,
            receive: None,
            channels: [None; 11],
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VoiceError {
    Io(std::io::ErrorKind),
    Timeout,
    ControlDisconnected,
    NotReady,
    NotTransmitting,
    InvalidRadio,
    InvalidAudio,
    SequenceExhausted,
    SendTimeout,
    Codec(srs_audio::CodecError),
}
impl fmt::Display for VoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Io(_) => "SRS UDP I/O failed",
            Self::Timeout => "SRS UDP heartbeat timed out",
            Self::ControlDisconnected => "SRS control session is closed",
            Self::NotReady => "SRS UDP is not ready",
            Self::NotTransmitting => "PTT is released or its authorization changed",
            Self::InvalidRadio => "radio cannot transmit",
            Self::InvalidAudio => "empty or oversized encoded audio",
            Self::SequenceExhausted => "voice sequence exhausted; open a new voice connection",
            Self::Codec(_) => "receive codec or playout failed",
            Self::SendTimeout => "voice frame expired before UDP could send it",
        })
    }
}
impl std::error::Error for VoiceError {}
impl From<std::io::Error> for VoiceError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.kind())
    }
}

/// Encoded network data, not permission to play it. The receiver still needs
/// `buffer_voice` for local radio/key checks, ordering and bounded decoding.
/// Coalition routing is enforced by the connected SRS server.
pub struct ReceivedVoice {
    pub(crate) generation: u64,
    pub(crate) receiver: ClientId,
    pub audio: Vec<u8>,
    pub frequencies: Vec<Frequency>,
    pub unit_id: u32,
    pub packet_number: u64,
    pub retransmission_count: u8,
    pub original_client_id: ClientId,
    pub client_id: ClientId,
    pub received_at: Instant,
}

pub enum VoiceEvent {
    Keepalive,
    Voice(ReceivedVoice),
    /// Malformed, oversized, empty, or mismatched keepalive datagram. It does
    /// not refresh liveness and no payload is echoed into diagnostics.
    Rejected,
    AuthorizationChanged,
}

pub struct VoiceConnection {
    pub(crate) receive: crate::receive::ReceiveState,
    socket: Option<UdpSocket>,
    pub(crate) id: ClientId,
    pub(crate) authorization: watch::Receiver<Authorization>,
    control_alive: watch::Receiver<bool>,
    pub(crate) ptt: Option<(usize, u64)>,
    sequence: u64,
    last_received: Instant,
    ping_at: Instant,
    registering: bool,
    // Full UDP size prevents OS truncation from turning a large datagram into
    // a valid short one. Application acceptance is capped to 8 KiB below.
    receive_buffer: Box<[u8; 65536]>,
}

impl VoiceConnection {
    pub(crate) async fn register(
        address: SocketAddr,
        id: ClientId,
        authorization: watch::Receiver<Authorization>,
        control_alive: watch::Receiver<bool>,
        deadline: Duration,
    ) -> Result<Self, VoiceError> {
        if !*control_alive.borrow() {
            return Err(VoiceError::ControlDisconnected);
        }
        let local = if address.is_ipv4() {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        };
        let socket = UdpSocket::bind(SocketAddr::new(local, 0)).await?;
        socket.connect(address).await?;
        let mut connection = Self {
            receive: crate::receive::ReceiveState::default(),
            socket: Some(socket),
            id,
            authorization,
            control_alive,
            ptt: None,
            sequence: 1,
            last_received: Instant::now(),
            ping_at: Instant::now(),
            registering: true,
            receive_buffer: Box::new([0; 65536]),
        };
        let until = Instant::now() + deadline;
        loop {
            match timeout_at(until, connection.next_event()).await {
                Ok(Ok(VoiceEvent::Keepalive)) => {
                    connection.registering = false;
                    connection.ping_at = Instant::now() + KEEPALIVE_INTERVAL;
                    return Ok(connection);
                }
                Ok(Ok(_)) => continue,
                Ok(Err(error)) => return Err(error),
                Err(_) => return Err(VoiceError::Timeout),
            }
        }
    }

    pub(crate) fn check(&self) -> Result<(), VoiceError> {
        if !*self.control_alive.borrow()
            || self.control_alive.has_changed().is_err()
            || self.authorization.has_changed().is_err()
        {
            return Err(VoiceError::ControlDisconnected);
        }
        if self.socket.is_none() {
            return Err(VoiceError::NotReady);
        }
        if Instant::now().duration_since(self.last_received) >= VOICE_TIMEOUT {
            return Err(VoiceError::Timeout);
        }
        Ok(())
    }

    pub fn is_ready(&self) -> bool {
        self.check().is_ok()
    }
    pub fn is_transmitting(&self) -> bool {
        self.check().is_ok()
            && self.ptt.is_some_and(|(slot, revision)| {
                let auth = self.authorization.borrow();
                revision == auth.revision && auth.channels[slot].is_some()
            })
    }
    pub fn press_ptt(&mut self, radio: usize) -> Result<(), VoiceError> {
        self.release_ptt();
        self.check()?;
        let auth = self.authorization.borrow_and_update();
        if radio >= auth.channels.len() || auth.channels[radio].is_none() {
            return Err(VoiceError::InvalidRadio);
        }
        self.ptt = Some((radio, auth.revision));
        Ok(())
    }
    pub fn release_ptt(&mut self) {
        self.ptt = None;
        // Observe transitions even when press and release both happen between
        // playout polls; old audio must not survive a half-duplex TX interval.
        if self
            .authorization
            .borrow()
            .receive
            .as_ref()
            .is_some_and(|p| p.irl_tx)
        {
            self.receive.clear();
        }
    }
    pub fn close(&mut self) {
        self.release_ptt();
        self.socket = None;
        self.receive.clear();
    }

    /// The caller supplies one current encoded frame. No outgoing queue or
    /// automatic retransmission. Cancellation/errors release PTT. Codec work and
    /// this async method belong outside audio callbacks.
    pub async fn send_frame(&mut self, audio: &[u8]) -> Result<u64, VoiceError> {
        let held = self.ptt.take().ok_or(VoiceError::NotTransmitting)?;
        self.check()?;
        if audio.is_empty() || audio.len() > MAX_AUDIO_BYTES {
            return Err(VoiceError::InvalidAudio);
        }
        let channel = {
            let auth = self.authorization.borrow();
            if held.1 != auth.revision {
                return Err(VoiceError::NotTransmitting);
            }
            auth.channels[held.0].ok_or(VoiceError::NotTransmitting)?
        };
        let sequence = self.sequence;
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or(VoiceError::SequenceExhausted)?;
        let packet = VoicePacket {
            audio,
            frequencies: vec![channel.frequency],
            unit_id: channel.unit_id,
            packet_number: sequence,
            retransmission_count: 0,
            original_client_id: self.id,
            client_id: self.id,
        }
        .encode()
        .map_err(|_| VoiceError::InvalidAudio)?;
        let socket = self.socket.as_ref().ok_or(VoiceError::NotReady)?;
        // Bound the send and observe session changes while waiting for UDP capacity.
        tokio::select! {
            biased;
            _ = self.control_alive.changed() => { self.close(); return Err(VoiceError::ControlDisconnected); },
            _ = self.authorization.changed() => return Err(VoiceError::NotTransmitting),
            _ = sleep_until(self.last_received + VOICE_TIMEOUT) => return Err(VoiceError::Timeout),
            _ = tokio::time::sleep(SEND_DEADLINE) => return Err(VoiceError::SendTimeout),
            result = socket.send(&packet) => {
                match result {
                    Ok(n) if n == packet.len() => (),
                    Ok(_) => { self.close(); return Err(VoiceError::Io(std::io::ErrorKind::WriteZero)); }
                    Err(error) => { self.close(); return Err(error.into()); }
                }
            }
        }
        self.ptt = Some(held);
        Ok(sequence)
    }

    /// Cancellation-safe read: callers may select this against input events.
    /// Keep polling for 15-second keepalives; any valid datagram from the connected
    /// server resets the 42-second deadline. No automatic socket restart.
    pub async fn next_event(&mut self) -> Result<VoiceEvent, VoiceError> {
        if let Err(error) = self.check() {
            self.close();
            return Err(error);
        }
        loop {
            let socket = self.socket.as_ref().ok_or(VoiceError::NotReady)?;
            if Instant::now() >= self.ping_at {
                let result = tokio::select! {
                    biased;
                    _ = self.control_alive.changed() => Err(VoiceError::ControlDisconnected),
                    _ = sleep_until(self.last_received + VOICE_TIMEOUT) => Err(VoiceError::Timeout),
                    result = socket.send(self.id.as_bytes()) => result.map(|_| ()).map_err(VoiceError::from),
                };
                if let Err(error) = result {
                    self.close();
                    return Err(error);
                }
                self.ping_at = Instant::now()
                    + if self.registering {
                        REGISTRATION_RETRY
                    } else {
                        KEEPALIVE_INTERVAL
                    };
            }
            let count = tokio::select! {
                biased;
                _ = self.control_alive.changed() => { self.close(); return Err(VoiceError::ControlDisconnected); }
                changed = self.authorization.changed() => {
                    self.release_ptt();
                    if changed.is_err() { self.close(); return Err(VoiceError::ControlDisconnected); }
                    return Ok(VoiceEvent::AuthorizationChanged);
                }
                _ = sleep_until(self.last_received + VOICE_TIMEOUT) => { self.close(); return Err(VoiceError::Timeout); }
                _ = sleep_until(self.ping_at) => continue,
                result = socket.recv(self.receive_buffer.as_mut_slice()) => match result {
                    Ok(count) => count,
                    Err(error) => { self.close(); return Err(error.into()); }
                },
            };
            if count > MAX_VOICE_DATAGRAM {
                return Ok(VoiceEvent::Rejected);
            }
            let bytes = &self.receive_buffer[..count];
            let event = match Datagram::decode(bytes) {
                Ok(Datagram::Keepalive(id)) if id == self.id => VoiceEvent::Keepalive,
                Ok(Datagram::Voice(packet))
                    if !packet.audio.is_empty()
                        && packet.audio.len() <= MAX_AUDIO_BYTES
                        && !packet.frequencies.is_empty() =>
                {
                    VoiceEvent::Voice(ReceivedVoice {
                        generation: self.authorization.borrow().revision,
                        receiver: self.id,
                        audio: packet.audio.to_vec(),
                        frequencies: packet.frequencies,
                        unit_id: packet.unit_id,
                        packet_number: packet.packet_number,
                        retransmission_count: packet.retransmission_count,
                        original_client_id: packet.original_client_id,
                        client_id: packet.client_id,
                        received_at: Instant::now(),
                    })
                }
                _ => return Ok(VoiceEvent::Rejected),
            };
            self.last_received = Instant::now();
            return Ok(event);
        }
    }
}
