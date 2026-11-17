//! TCP/EAM behavior derived from Common/Network/{Client/TCPClientHandler,
//! Server/ServerSync}.cs and DCSRadioSyncHandler.cs, Ciribob and contributors
//! (GPLv3), upstream d01f540ce5e363e9ac841775c8f7e2253138cd81.
//!
//! A caller-driven session: keep polling `next_event` to receive updates and send
//! periodic radio refreshes. No detached tasks, unbounded event queues, audio, or
//! automatic reconnect. Dropping a session closes TCP. Cancelling a pending
//! public async operation also closes TCP; reconnect with a new session.

mod model;
mod transport;

pub use model::{Coalition, EAM_UNIT_ID, Identity, RADIO_COUNT, Radio, disabled_radios};
use serde_json::{Map, Value};
use srs_protocol::{
    control::{ControlError, ControlMessage, MessageType},
    voice::{ClientId, Modulation},
};
use std::{collections::BTreeMap, fmt, net::SocketAddr, time::Duration};
use tokio::time::{Instant, timeout_at};
use transport::Transport;

pub const COMPATIBILITY_VERSION: &str = "2.4.1.0";
pub const MINIMUM_SERVER_VERSION: &str = "1.9.0.0";
pub const MAX_CLIENTS: usize = 1024;
pub const MAX_CLIENT_BYTES: usize = 64 * 1024;
pub const MAX_ROSTER_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SETTINGS_BYTES: usize = 1024 * 1024;
const REFRESH_INTERVAL: Duration = Duration::from_secs(60);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Io(std::io::ErrorKind),
    Protocol(ControlError),
    Timeout,
    Disconnected,
    InvalidIdentity,
    InvalidRadio,
    InvalidOptions,
    InvalidMessage,
    ResourceLimit,
    IncompatibleVersion,
    EamDisabled,
    AuthenticationRejected,
    NotAuthenticated,
    PolicyDenied,
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.kind())
    }
}
impl From<ControlError> for Error {
    fn from(value: ControlError) -> Self {
        Self::Protocol(value)
    }
}
impl std::error::Error for Error {}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never echo peer JSON, endpoints, identity, or credentials.
        f.write_str(match self {
            Self::Io(_) => "SRS connection I/O failed",
            Self::Protocol(_) => "invalid SRS control frame",
            Self::Timeout => "SRS operation timed out",
            Self::Disconnected => "SRS connection is closed",
            Self::InvalidIdentity => "invalid standalone identity",
            Self::InvalidRadio => "invalid radio settings",
            Self::InvalidOptions => "invalid session options",
            Self::InvalidMessage => "invalid SRS session message",
            Self::ResourceLimit => "SRS session resource limit exceeded",
            Self::IncompatibleVersion => "incompatible SRS version",
            Self::EamDisabled => "server has disabled External AWACS Mode",
            Self::AuthenticationRejected => "coalition password was rejected",
            Self::NotAuthenticated => "External AWACS Mode authentication required",
            Self::PolicyDenied => "radio settings conflict with server policy",
        })
    }
}

#[derive(Clone)]
pub struct Options {
    pub compatibility_version: String,
    pub operation_timeout: Duration,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            compatibility_version: COMPATIBILITY_VERSION.into(),
            operation_timeout: Duration::from_secs(10),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Connected,
    Authenticated(Coalition),
    Disconnected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Event {
    Synchronized,
    ClientUpdated(ClientId),
    ClientDisconnected(ClientId),
    SettingsUpdated,
    RadiosAdjusted,
    EamDisabled,
    Ignored(MessageType),
}

// Compare .NET Version's 2–4 integer components; omitted components sort before 0.
fn version(value: &str) -> Result<[i32; 4], Error> {
    let mut result = [-1; 4];
    let mut count = 0;
    for (i, part) in value.split('.').enumerate() {
        if i >= 4 || part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(Error::IncompatibleVersion);
        }
        result[i] = part.parse().map_err(|_| Error::IncompatibleVersion)?;
        count += 1;
    }
    if count < 2 {
        return Err(Error::IncompatibleVersion);
    }
    Ok(result)
}

/// Bounded JSON snapshots retain upstream optional and future radio fields.
/// UPDATE merges metadata without discarding the last RADIO_UPDATE payload.
pub struct Session {
    transport: Option<Transport>,
    identity: Identity,
    options: Options,
    coalition: Option<Coalition>,
    radios: [Radio; RADIO_COUNT],
    clients: BTreeMap<String, Map<String, Value>>,
    client_sizes: BTreeMap<String, usize>,
    roster_bytes: usize,
    settings: BTreeMap<String, String>,
    server_version: Option<String>,
    refresh_at: Instant,
}

impl Session {
    pub async fn connect(
        address: SocketAddr,
        identity: Identity,
        options: Options,
    ) -> Result<Self, Error> {
        identity.validate()?;
        version(&options.compatibility_version).map_err(|_| Error::InvalidOptions)?;
        if options.operation_timeout.is_zero()
            || options.operation_timeout > Duration::from_secs(90)
        {
            return Err(Error::InvalidOptions);
        }
        let deadline = Instant::now() + options.operation_timeout;
        let mut transport = Transport::connect(address, options.operation_timeout).await?;
        let mut session = Self {
            transport: None,
            identity,
            options,
            coalition: None,
            radios: disabled_radios(),
            clients: BTreeMap::new(),
            client_sizes: BTreeMap::new(),
            roster_bytes: 0,
            settings: BTreeMap::new(),
            server_version: None,
            refresh_at: Instant::now() + REFRESH_INTERVAL,
        };
        transport.send(&session.message(MessageType::SYNC)).await?;
        loop {
            let message = timeout_at(deadline, transport.receive())
                .await
                .map_err(|_| Error::Timeout)??;
            if session.apply(message)? == Event::Synchronized {
                break;
            }
        }
        session.transport = Some(transport);
        Ok(session)
    }

    pub fn state(&self) -> State {
        if self.transport.is_none() {
            State::Disconnected
        } else if let Some(coalition) = self.coalition {
            State::Authenticated(coalition)
        } else {
            State::Connected
        }
    }
    pub fn server_version(&self) -> Option<&str> {
        self.server_version.as_deref()
    }
    pub fn settings(&self) -> &BTreeMap<String, String> {
        &self.settings
    }
    pub fn clients(&self) -> &BTreeMap<String, Map<String, Value>> {
        &self.clients
    }
    pub fn close(&mut self) {
        self.transport = None;
        self.coalition = None;
        self.radios = disabled_radios();
        self.clients.clear();
        self.client_sizes.clear();
        self.roster_bytes = 0;
    }

    /// Password is borrowed and is not retained by the session or diagnostics.
    /// On rejection TCP stays connected as a spectator; on timeout/malformed
    /// responses TCP closes. The server selects Red/Blue based on the password.
    pub async fn authenticate(&mut self, password: &str) -> Result<Coalition, Error> {
        if self.transport.is_none() {
            return Err(Error::Disconnected);
        }
        if !self.setting_bool("EXTERNAL_AWACS_MODE", false)? {
            return Err(Error::EamDisabled);
        }
        if password.len() > 1024 {
            return Err(Error::InvalidOptions);
        }
        // One login attempt per connected spectator; leave first to switch sides.
        if self.coalition.is_some() {
            return Err(Error::InvalidOptions);
        }
        let mut transport = self.transport.take().ok_or(Error::Disconnected)?;
        let mut message = self.message(MessageType::EXTERNAL_AWACS_MODE_PASSWORD);
        message.external_awacs_mode_password = Some(password.into());
        transport.send(&message).await?;
        drop(message);
        let deadline = Instant::now() + self.options.operation_timeout;
        loop {
            let reply = timeout_at(deadline, transport.receive())
                .await
                .map_err(|_| Error::Timeout)??;
            if reply.message_type == MessageType::EXTERNAL_AWACS_MODE_PASSWORD {
                let coalition = reply
                    .client
                    .as_ref()
                    .and_then(|c| c.get("Coalition"))
                    .and_then(Value::as_u64)
                    .ok_or(Error::InvalidMessage)?;
                self.coalition = Coalition::parse(coalition)?;
                // A settings change received during login takes precedence.
                if !self.setting_bool("EXTERNAL_AWACS_MODE", false)? {
                    self.coalition = None;
                    return Err(Error::EamDisabled);
                }
                self.transport = Some(transport);
                return self.coalition.ok_or(Error::AuthenticationRejected);
            }
            self.apply(reply)?;
        }
    }

    /// Send a complete 11-slot state. Success means TCP write completed, not an
    /// acknowledgement or a voice-delivery guarantee; inspect peer broadcasts.
    pub async fn set_radios(&mut self, radios: [Radio; RADIO_COUNT]) -> Result<(), Error> {
        if self.transport.is_none() {
            return Err(Error::Disconnected);
        }
        if self.coalition.is_none() {
            return Err(Error::NotAuthenticated);
        }
        if !self.setting_bool("EXTERNAL_AWACS_MODE", false)? {
            return Err(Error::EamDisabled);
        }
        self.validate_radios(&radios)?;
        let mut message = self.message(MessageType::RADIO_UPDATE);
        message.client = Some(model::client_wire(&self.identity, self.coalition, &radios));
        let mut transport = self.transport.take().ok_or(Error::Disconnected)?;
        transport.send(&message).await?;
        self.radios = radios;
        self.refresh_at = Instant::now() + REFRESH_INTERVAL;
        self.transport = Some(transport);
        Ok(())
    }

    /// Leave EAM and explicitly reset the server's stored radio state.
    pub async fn leave_eam(&mut self) -> Result<(), Error> {
        let mut transport = self.transport.take().ok_or(Error::Disconnected)?;
        self.coalition = None;
        self.radios = disabled_radios();
        transport
            .send(&self.message(MessageType::EXTERNAL_AWACS_MODE_DISCONNECT))
            .await?;
        transport
            .send(&self.message(MessageType::RADIO_UPDATE))
            .await?;
        self.transport = Some(transport);
        Ok(())
    }

    /// Drive incoming state and 60-second radio refreshes. Idle servers need not
    /// send TCP pings. Callers must keep polling; cancellation closes the session.
    pub async fn next_event(&mut self) -> Result<Event, Error> {
        let mut transport = self.transport.take().ok_or(Error::Disconnected)?;
        loop {
            if Instant::now() >= self.refresh_at {
                transport
                    .send(&self.message(MessageType::RADIO_UPDATE))
                    .await?;
                self.refresh_at = Instant::now() + REFRESH_INTERVAL;
            }
            match timeout_at(self.refresh_at, transport.receive()).await {
                Ok(message) => {
                    let event = self.apply(message?)?;
                    if event == Event::EamDisabled {
                        // Revocation drops local authorization before any further await.
                        transport
                            .send(&self.message(MessageType::EXTERNAL_AWACS_MODE_DISCONNECT))
                            .await?;
                    }
                    if matches!(event, Event::EamDisabled | Event::RadiosAdjusted) {
                        transport
                            .send(&self.message(MessageType::RADIO_UPDATE))
                            .await?;
                    }
                    self.transport = Some(transport);
                    return Ok(event);
                }
                Err(_) => continue,
            }
        }
    }

    fn message(&self, kind: MessageType) -> ControlMessage {
        let mut message = ControlMessage::new(kind, &self.options.compatibility_version);
        message.client = Some(model::client_wire(
            &self.identity,
            self.coalition,
            &self.radios,
        ));
        message
    }

    fn setting_bool(&self, key: &str, default: bool) -> Result<bool, Error> {
        match self.settings.get(key).map(|v| v.trim()) {
            None => Ok(default),
            Some(v) if v.eq_ignore_ascii_case("true") => Ok(true),
            Some(v) if v.eq_ignore_ascii_case("false") => Ok(false),
            _ => Err(Error::InvalidMessage),
        }
    }

    fn validate_radios(&self, radios: &[Radio; RADIO_COUNT]) -> Result<(), Error> {
        if radios[0].modulation != Modulation::DISABLED
            && radios[0].modulation != Modulation::INTERCOM
        {
            return Err(Error::InvalidRadio);
        }
        for radio in radios {
            radio.validate()?;
            if (radio.encryption_key.is_some()
                && !self.setting_bool("ALLOW_RADIO_ENCRYPTION", true)?)
                || (radio.intercom_unit_id != 0
                    && !self.setting_bool("ALLOW_INSTRUCTOR_MODE", false)?)
                || (radio.retransmit
                    && self
                        .settings
                        .get("RETRANSMISSION_NODE_LIMIT")
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(0)
                        == 0)
            {
                return Err(Error::PolicyDenied);
            }
        }
        Ok(())
    }

    fn apply(&mut self, message: ControlMessage) -> Result<Event, Error> {
        if message.message_type == MessageType::VERSION_MISMATCH {
            return Err(Error::IncompatibleVersion);
        }
        let was_authenticated = self.coalition.is_some();
        let mut radios_adjusted = false;
        if let Some(settings) = message.server_settings {
            // Upstream merges settings, including partial updates. Bound the
            // accumulated map as well as each frame to prevent incremental growth.
            let mut combined = self.settings.clone();
            combined.extend(settings);
            if combined.len() > 4096
                || serde_json::to_vec(&combined)
                    .map_err(|_| Error::InvalidMessage)?
                    .len()
                    > MAX_SETTINGS_BYTES
            {
                return Err(Error::ResourceLimit);
            }
            self.settings = combined;
            if !self.setting_bool("EXTERNAL_AWACS_MODE", false)? {
                self.coalition = None;
                self.radios = disabled_radios();
            }
            let encryption = self.setting_bool("ALLOW_RADIO_ENCRYPTION", true)?;
            let instructor = self.setting_bool("ALLOW_INSTRUCTOR_MODE", false)?;
            let retransmit = match self.settings.get("RETRANSMISSION_NODE_LIMIT") {
                Some(value) => value.parse::<u32>().map_err(|_| Error::InvalidMessage)? > 0,
                None => false,
            };
            for radio in &mut self.radios {
                if !encryption && radio.encryption_key.take().is_some() {
                    radios_adjusted = true;
                }
                if !instructor && radio.intercom_unit_id != 0 {
                    radio.intercom_unit_id = 0;
                    radios_adjusted = true;
                }
                if !retransmit && radio.retransmit {
                    radio.retransmit = false;
                    radios_adjusted = true;
                }
            }
        }
        let event = match message.message_type {
            MessageType::SYNC => {
                let text = message.version.ok_or(Error::IncompatibleVersion)?;
                if version(&text)? < version(MINIMUM_SERVER_VERSION)? {
                    return Err(Error::IncompatibleVersion);
                }
                self.server_version = Some(text);
                self.clients.clear();
                self.client_sizes.clear();
                self.roster_bytes = 0;
                let clients = message.clients.ok_or(Error::InvalidMessage)?;
                if clients.len() > MAX_CLIENTS {
                    return Err(Error::ResourceLimit);
                }
                for client in clients {
                    self.store_client(client, false)?;
                }
                Event::Synchronized
            }
            MessageType::UPDATE | MessageType::RADIO_UPDATE => {
                let id = self.store_client(
                    message.client.ok_or(Error::InvalidMessage)?,
                    message.message_type == MessageType::UPDATE,
                )?;
                Event::ClientUpdated(id)
            }
            MessageType::CLIENT_DISCONNECT => {
                let client = message.client.ok_or(Error::InvalidMessage)?;
                let (key, id) = client_id(&client)?;
                self.clients.remove(key);
                self.roster_bytes -= self.client_sizes.remove(key).unwrap_or(0);
                Event::ClientDisconnected(id)
            }
            MessageType::SERVER_SETTINGS => Event::SettingsUpdated,
            kind => Event::Ignored(kind),
        };
        if was_authenticated && self.coalition.is_none() {
            Ok(Event::EamDisabled)
        } else if radios_adjusted {
            Ok(Event::RadiosAdjusted)
        } else {
            Ok(event)
        }
    }

    fn store_client(
        &mut self,
        mut client: Map<String, Value>,
        metadata: bool,
    ) -> Result<ClientId, Error> {
        let (key, id) = client_id(&client)?;
        let key = key.to_owned();
        if metadata {
            client.remove("RadioInfo");
        }
        if (metadata || client.get("RadioInfo").is_none_or(Value::is_null))
            && let Some(radios) = self.clients.get(&key).and_then(|c| c.get("RadioInfo"))
        {
            client.insert("RadioInfo".into(), radios.clone());
        }
        let size = serde_json::to_vec(&client)
            .map_err(|_| Error::InvalidMessage)?
            .len();
        let total = self.roster_bytes - self.client_sizes.get(&key).copied().unwrap_or(0) + size;
        if size > MAX_CLIENT_BYTES
            || total > MAX_ROSTER_BYTES
            || (!self.clients.contains_key(&key) && self.clients.len() >= MAX_CLIENTS)
        {
            return Err(Error::ResourceLimit);
        }
        self.clients.insert(key.clone(), client);
        self.client_sizes.insert(key, size);
        self.roster_bytes = total;
        Ok(id)
    }
}

fn client_id(client: &Map<String, Value>) -> Result<(&str, ClientId), Error> {
    let text = client
        .get("ClientGuid")
        .and_then(Value::as_str)
        .ok_or(Error::InvalidMessage)?;
    let id = ClientId::from_bytes(text.as_bytes()).map_err(|_| Error::InvalidMessage)?;
    Ok((text, id))
}
