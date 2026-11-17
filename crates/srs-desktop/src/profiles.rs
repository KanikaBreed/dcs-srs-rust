#![forbid(unsafe_code)]
//! Versioned user configuration. No passwords, session identity or input state.
//! All disk I/O runs on one owned worker; writes replace atomically under a
//! nonblocking sidecar lock and reject changes made by another application.
use crate::session::{Config, Problem};
use serde::{Deserialize, Serialize};
use srs_client::receive::{DEFAULT_MONITOR, Monitor, RadioLevel, Routing};
use srs_client::{RADIO_COUNT, Radio, disabled_radios};
use srs_protocol::voice::Modulation;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, SyncSender},
    thread::JoinHandle,
};
const MAX_BYTES: usize = 256 * 1024;
const MAX_ENTRIES: usize = 32;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {
    pub name: String,
    pub address: String,
    pub callsign: String,
    pub input: String,
    pub output: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Mode {
    Off,
    Am,
    Fm,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Route {
    Both,
    Left,
    Right,
}
impl From<Routing> for Route {
    fn from(value: Routing) -> Self {
        match value {
            Routing::Both => Self::Both,
            Routing::Left => Self::Left,
            Routing::Right => Self::Right,
        }
    }
}
impl From<Route> for Routing {
    fn from(value: Route) -> Self {
        match value {
            Route::Both => Self::Both,
            Route::Left => Self::Left,
            Route::Right => Self::Right,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetRadio {
    routing: Route,
    name: String,
    frequency_hz: f64,
    guard_hz: f64,
    mode: Mode,
    key: Option<u8>,
    volume: u8,
    muted: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub name: String,
    pub selected: usize,
    radios: [PresetRadio; 10],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    pub ptt_key: u8,
    pub background_mode: u8,
    pub background_key: u8,
    pub background_modifiers: u8,
    pub joystick_guid: String,
    pub joystick_button: u8,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            ptt_key: 8,
            background_mode: 0,
            background_key: 8,
            background_modifiers: 0,
            joystick_guid: String::new(),
            joystick_button: 0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Data {
    version: u32,
    pub servers: Vec<Server>,
    pub presets: Vec<Preset>,
    pub preferences: Preferences,
}
impl Default for Data {
    fn default() -> Self {
        Self {
            version: 5,
            servers: vec![],
            presets: vec![],
            preferences: Preferences::default(),
        }
    }
}
fn label(value: &str, max: usize, empty: bool) -> bool {
    (empty || !value.trim().is_empty())
        && value.len() <= max
        && !value.chars().any(char::is_control)
}
impl Server {
    pub fn new(name: &str, config: &Config) -> Result<Self, Problem> {
        if !label(name.trim(), 64, false) {
            return Err(Problem::Profile);
        }
        Ok(Self {
            name: name.trim().into(),
            address: config.address.to_string(),
            callsign: config.callsign.clone(),
            input: config.input.clone(),
            output: config.output.clone(),
        })
    }
    fn valid(&self) -> bool {
        label(&self.name, 64, false)
            && label(&self.input, 1024, false)
            && label(&self.output, 1024, false)
            && Config::parse(
                &self.address,
                &self.callsign,
                String::new(),
                self.input.clone(),
                self.output.clone(),
            )
            .is_ok()
    }
}
impl Preset {
    pub fn new(
        name: &str,
        selected: usize,
        radios: &[Radio; RADIO_COUNT],
        monitor: &Monitor,
    ) -> Result<Self, Problem> {
        let preset = Self {
            name: name.trim().into(),
            selected,
            radios: std::array::from_fn(|index| {
                let r = &radios[index + 1];
                PresetRadio {
                    routing: monitor[index + 1].routing().into(),
                    name: r.name.clone(),
                    frequency_hz: r.frequency_hz,
                    guard_hz: r.guard_hz,
                    mode: if r.modulation == Modulation::AM {
                        Mode::Am
                    } else if r.modulation == Modulation::FM {
                        Mode::Fm
                    } else {
                        Mode::Off
                    },
                    key: r.encryption_key,
                    volume: monitor[index + 1].percent(),
                    muted: monitor[index + 1].muted(),
                }
            }),
        };
        if !preset.valid()
            || radios[1..].iter().any(|r| {
                !matches!(
                    r.modulation,
                    Modulation::AM | Modulation::FM | Modulation::DISABLED
                ) || r.retransmit
                    || r.intercom_unit_id != 0
            })
        {
            return Err(Problem::Profile);
        }
        Ok(preset)
    }
    fn valid(&self) -> bool {
        label(&self.name, 64, false)
            && (1..=10).contains(&self.selected)
            && self.radios.iter().all(|r| {
                label(&r.name, 64, true)
                    && r.frequency_hz.is_finite()
                    && r.guard_hz.is_finite()
                    && (matches!(r.mode, Mode::Off) && r.frequency_hz == 1.0
                        || (10_001.0..=10e9).contains(&r.frequency_hz))
                    && (r.guard_hz == 1.0 || (10_001.0..=10e9).contains(&r.guard_hz))
                    && r.key != Some(0)
                    && r.volume <= 100
            })
    }
    pub fn monitor(&self) -> Result<Monitor, Problem> {
        let mut monitor = DEFAULT_MONITOR;
        for (out, radio) in monitor[1..].iter_mut().zip(&self.radios) {
            *out = RadioLevel::new(radio.volume, radio.muted)
                .ok_or(Problem::Profile)?
                .with_routing(radio.routing.into());
        }
        Ok(monitor)
    }
    pub fn radios(&self) -> [Radio; RADIO_COUNT] {
        let mut radios = disabled_radios();
        for (out, r) in radios[1..].iter_mut().zip(&self.radios) {
            *out = Radio {
                name: r.name.clone(),
                frequency_hz: r.frequency_hz,
                guard_hz: r.guard_hz,
                modulation: match r.mode {
                    Mode::Off => Modulation::DISABLED,
                    Mode::Am => Modulation::AM,
                    Mode::Fm => Modulation::FM,
                },
                encryption_key: r.key,
                ..Radio::default()
            };
        }
        radios
    }
}
impl Data {
    fn valid(&self) -> bool {
        fn unique<'a>(names: impl Iterator<Item = &'a str>) -> bool {
            let mut seen = std::collections::HashSet::new();
            names.into_iter().all(|name| seen.insert(name))
        }
        self.version == 5
            && self.preferences.ptt_key <= 12
            && self.preferences.background_mode <= 2
            && (1..=12).contains(&self.preferences.background_key)
            && self.preferences.background_modifiers < 16
            && self.preferences.joystick_button < 128
            && (self.preferences.joystick_guid.is_empty()
                || (self.preferences.joystick_guid.len() == 32
                    && self
                        .preferences
                        .joystick_guid
                        .bytes()
                        .all(|b| b.is_ascii_hexdigit())))
            && self.servers.len() <= MAX_ENTRIES
            && self.presets.len() <= MAX_ENTRIES
            && self.servers.iter().all(Server::valid)
            && self.presets.iter().all(Preset::valid)
            && unique(self.servers.iter().map(|s| s.name.as_str()))
            && unique(self.presets.iter().map(|p| p.name.as_str()))
    }
    pub fn save_server(&mut self, server: Server) -> Result<(), Problem> {
        if let Some(old) = self.servers.iter_mut().find(|s| s.name == server.name) {
            *old = server;
        } else if self.servers.len() < MAX_ENTRIES {
            self.servers.push(server);
        } else {
            return Err(Problem::Profile);
        }
        Ok(())
    }
    pub fn save_preset(&mut self, preset: Preset) -> Result<(), Problem> {
        if let Some(old) = self.presets.iter_mut().find(|p| p.name == preset.name) {
            *old = preset;
        } else if self.presets.len() < MAX_ENTRIES {
            self.presets.push(preset);
        } else {
            return Err(Problem::Profile);
        }
        Ok(())
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum StoreError {
    Io = 2,
    Invalid = 3,
    Conflict = 4,
    Busy = 5,
    NameConflict = 6,
    Exists = 7,
    Canceled = 8,
}
fn read(path: &Path) -> Result<Option<Vec<u8>>, StoreError> {
    match File::open(path) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(MAX_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| StoreError::Io)?;
            if bytes.len() > MAX_BYTES {
                return Err(StoreError::Invalid);
            }
            Ok(Some(bytes))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(StoreError::Io),
    }
}
// Keep the v1 wire types strict and separate: unknown/duplicate fields must
// still fail, and future additions to the current schema must not silently relax migration.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyRadio {
    name: String,
    frequency_hz: f64,
    guard_hz: f64,
    mode: Mode,
    key: Option<u8>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyPreset {
    name: String,
    selected: usize,
    radios: [LegacyRadio; 10],
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyData {
    version: u32,
    servers: Vec<Server>,
    presets: Vec<LegacyPreset>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyV2 {
    version: u32,
    servers: Vec<Server>,
    presets: Vec<LegacyMonitorPreset>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyMonitorRadio {
    name: String,
    frequency_hz: f64,
    guard_hz: f64,
    mode: Mode,
    key: Option<u8>,
    volume: u8,
    muted: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyMonitorPreset {
    name: String,
    selected: usize,
    radios: [LegacyMonitorRadio; 10],
}
impl From<LegacyMonitorPreset> for Preset {
    fn from(p: LegacyMonitorPreset) -> Self {
        Self {
            name: p.name,
            selected: p.selected,
            radios: p.radios.map(|r| PresetRadio {
                name: r.name,
                frequency_hz: r.frequency_hz,
                guard_hz: r.guard_hz,
                mode: r.mode,
                key: r.key,
                volume: r.volume,
                muted: r.muted,
                routing: Route::Both,
            }),
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyV3 {
    version: u32,
    servers: Vec<Server>,
    presets: Vec<LegacyMonitorPreset>,
    preferences: LegacyPreferences,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyPreferences {
    ptt_key: u8,
}
impl From<LegacyPreferences> for Preferences {
    fn from(old: LegacyPreferences) -> Self {
        Self {
            ptt_key: old.ptt_key,
            ..Self::default()
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyV4 {
    version: u32,
    servers: Vec<Server>,
    presets: Vec<Preset>,
    preferences: LegacyPreferences,
}
fn decode(bytes: &[u8]) -> Result<Data, StoreError> {
    #[derive(Deserialize)]
    struct Header {
        version: u32,
    }
    let header: Header = serde_json::from_slice(bytes).map_err(|_| StoreError::Invalid)?;
    let data = match header.version {
        1 => {
            let old: LegacyData = serde_json::from_slice(bytes).map_err(|_| StoreError::Invalid)?;
            if old.version != 1 {
                return Err(StoreError::Invalid);
            }
            Data {
                version: 5,
                preferences: Preferences::default(),
                servers: old.servers,
                presets: old
                    .presets
                    .into_iter()
                    .map(|p| Preset {
                        name: p.name,
                        selected: p.selected,
                        radios: p.radios.map(|r| PresetRadio {
                            routing: Route::Both,
                            name: r.name,
                            frequency_hz: r.frequency_hz,
                            guard_hz: r.guard_hz,
                            mode: r.mode,
                            key: r.key,
                            volume: 100,
                            muted: false,
                        }),
                    })
                    .collect(),
            }
        }
        2 => {
            let old: LegacyV2 = serde_json::from_slice(bytes).map_err(|_| StoreError::Invalid)?;
            if old.version != 2 {
                return Err(StoreError::Invalid);
            }
            Data {
                version: 5,
                servers: old.servers,
                presets: old.presets.into_iter().map(Into::into).collect(),
                preferences: Preferences::default(),
            }
        }
        3 => {
            let old: LegacyV3 = serde_json::from_slice(bytes).map_err(|_| StoreError::Invalid)?;
            if old.version != 3 {
                return Err(StoreError::Invalid);
            }
            Data {
                version: 5,
                servers: old.servers,
                presets: old.presets.into_iter().map(Into::into).collect(),
                preferences: old.preferences.into(),
            }
        }
        4 => {
            let old: LegacyV4 = serde_json::from_slice(bytes).map_err(|_| StoreError::Invalid)?;
            if old.version != 4 {
                return Err(StoreError::Invalid);
            }
            Data {
                version: 5,
                servers: old.servers,
                presets: old.presets,
                preferences: old.preferences.into(),
            }
        }
        5 => serde_json::from_slice(bytes).map_err(|_| StoreError::Invalid)?,
        _ => return Err(StoreError::Invalid),
    };
    if !data.valid() {
        return Err(StoreError::Invalid);
    }
    Ok(data)
}
struct Store {
    path: PathBuf,
    expected: Option<Vec<u8>>,
    writable: bool,
}
impl Store {
    fn load(&mut self) -> Result<Data, StoreError> {
        self.writable = false;
        let bytes = read(&self.path)?;
        let data: Data = match &bytes {
            Some(bytes) => decode(bytes)?,
            None => Data::default(),
        };
        if !data.valid() {
            return Err(StoreError::Invalid);
        }
        self.expected = bytes;
        self.writable = true;
        Ok(data)
    }
    fn save(&mut self, data: Data) -> Result<Data, StoreError> {
        if !self.writable || !data.valid() {
            return Err(StoreError::Invalid);
        }
        let bytes = serde_json::to_vec_pretty(&data).map_err(|_| StoreError::Invalid)?;
        if bytes.len() > MAX_BYTES {
            return Err(StoreError::Invalid);
        }
        let parent = self.path.parent().ok_or(StoreError::Io)?;
        let mut builder = std::fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(parent).map_err(|_| StoreError::Io)?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options
            .open(self.path.with_extension("lock"))
            .map_err(|_| StoreError::Io)?;
        lock.try_lock().map_err(|_| StoreError::Busy)?;
        if read(&self.path)? != self.expected {
            return Err(StoreError::Conflict);
        }
        // NamedTempFile uses restrictive file permissions and replaces the old
        // file atomically on Unix and Windows. A failed persist keeps the old file.
        let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| StoreError::Io)?;
        file.write_all(&bytes).map_err(|_| StoreError::Io)?;
        file.as_file().sync_all().map_err(|_| StoreError::Io)?;
        file.persist(&self.path).map_err(|_| StoreError::Io)?;
        self.expected = Some(bytes);
        Ok(data)
    }
}
// This interchange format deliberately contains only user-created radio presets.
// It is not the upstream SRS server preset format or our complete profiles file.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    format: String,
    version: u32,
    presets: Vec<Preset>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyBundle {
    format: String,
    version: u32,
    presets: Vec<LegacyMonitorPreset>,
}
const BUNDLE_FORMAT: &str = "dcs-srs-rust-radio-presets";
fn import_presets(path: &Path, mut data: Data) -> Result<Data, StoreError> {
    let bytes = read(path)?.ok_or(StoreError::Io)?;
    #[derive(Deserialize)]
    struct Header {
        version: u32,
    }
    let header: Header = serde_json::from_slice(&bytes).map_err(|_| StoreError::Invalid)?;
    let bundle = match header.version {
        1 => {
            let old: LegacyBundle =
                serde_json::from_slice(&bytes).map_err(|_| StoreError::Invalid)?;
            if old.version != 1 {
                return Err(StoreError::Invalid);
            }
            Bundle {
                format: old.format,
                version: 2,
                presets: old.presets.into_iter().map(Into::into).collect(),
            }
        }
        2 => serde_json::from_slice(&bytes).map_err(|_| StoreError::Invalid)?,
        _ => return Err(StoreError::Invalid),
    };
    if bundle.format != BUNDLE_FORMAT || bundle.version != 2 {
        return Err(StoreError::Invalid);
    }
    let incoming = Data {
        presets: bundle.presets,
        ..Data::default()
    };
    if !data.valid()
        || !incoming.valid()
        || data.presets.len() + incoming.presets.len() > MAX_ENTRIES
    {
        return Err(StoreError::Invalid);
    }
    if incoming
        .presets
        .iter()
        .any(|p| data.presets.iter().any(|old| old.name == p.name))
    {
        return Err(StoreError::NameConflict);
    }
    data.presets.extend(incoming.presets);
    Ok(data)
}
fn export_presets(path: &Path, data: &Data) -> Result<(), StoreError> {
    if !data.valid() {
        return Err(StoreError::Invalid);
    }
    let bytes = serde_json::to_vec_pretty(&Bundle {
        format: BUNDLE_FORMAT.into(),
        version: 2,
        presets: data.presets.clone(),
    })
    .map_err(|_| StoreError::Invalid)?;
    if bytes.len() > MAX_BYTES {
        return Err(StoreError::Invalid);
    }
    let parent = path.parent().ok_or(StoreError::Io)?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|_| StoreError::Io)?;
    file.write_all(&bytes).map_err(|_| StoreError::Io)?;
    file.as_file().sync_all().map_err(|_| StoreError::Io)?;
    file.persist_noclobber(path).map_err(|e| {
        if e.error.kind() == std::io::ErrorKind::AlreadyExists {
            StoreError::Exists
        } else {
            StoreError::Io
        }
    })?;
    Ok(())
}
pub enum Command {
    Reload,
    Save(Data),
    Import {
        data: Data,
        title: String,
        filter: String,
    },
    Export {
        data: Data,
        title: String,
        filter: String,
    },
}
pub struct Worker {
    sender: Option<SyncSender<Command>>,
    receiver: Option<Receiver<Result<Data, StoreError>>>,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    pub fn default_path() -> Option<PathBuf> {
        directories::BaseDirs::new()
            .map(|dirs| dirs.config_dir().join("dcs-srs-rust/profiles.json"))
    }
    pub fn start(path: Option<PathBuf>) -> Result<Self, StoreError> {
        let (sender, commands) = mpsc::sync_channel(1);
        let (results, receiver) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("srs-profiles".into())
            .spawn(move || {
                let Some(path) = path else {
                    let _ = results.send(Err(StoreError::Io));
                    return;
                };
                let mut store = Store {
                    path,
                    expected: None,
                    writable: false,
                };
                if results.send(store.load()).is_err() {
                    return;
                }
                while let Ok(command) = commands.recv() {
                    let result = match command {
                        Command::Reload => store.load(),
                        Command::Save(data) => store.save(data),
                        Command::Import {
                            data,
                            title,
                            filter,
                        } => rfd::FileDialog::new()
                            .set_title(title)
                            .add_filter(filter, &["json"])
                            .pick_file()
                            .ok_or(StoreError::Canceled)
                            .and_then(|path| import_presets(&path, data))
                            .and_then(|data| store.save(data)),
                        Command::Export {
                            data,
                            title,
                            filter,
                        } => rfd::FileDialog::new()
                            .set_title(title)
                            .add_filter(filter, &["json"])
                            .set_file_name("srs-radio-presets.json")
                            .save_file()
                            .ok_or(StoreError::Canceled)
                            .and_then(|path| export_presets(&path, &data).map(|()| data)),
                    };
                    if results.send(result).is_err() {
                        break;
                    }
                }
            })
            .map_err(|_| StoreError::Io)?;
        Ok(Self {
            sender: Some(sender),
            receiver: Some(receiver),
            thread: Some(thread),
        })
    }
    pub fn send(&self, command: Command) -> Result<(), StoreError> {
        self.sender
            .as_ref()
            .ok_or(StoreError::Io)?
            .try_send(command)
            .map_err(|_| StoreError::Busy)
    }
    pub fn poll(&self) -> Option<Result<Data, StoreError>> {
        match self.receiver.as_ref()?.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(StoreError::Io)),
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.receiver.take();
        self.sender.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn store(path: PathBuf) -> Store {
        Store {
            path,
            expected: None,
            writable: false,
        }
    }
    fn example() -> Data {
        let config = Config::parse(
            "127.0.0.1:5002",
            "TOC",
            "SECRET-PASSWORD".into(),
            "device:a".into(),
            "default".into(),
        )
        .ok()
        .unwrap();
        let mut data = Data::default();
        data.save_server(Server::new("Training", &config).ok().unwrap())
            .unwrap();
        let mut radios = disabled_radios();
        radios[1] = Radio {
            name: "Command".into(),
            guard_hz: 243e6,
            encryption_key: Some(7),
            ..Radio::tuned(251e6, Modulation::AM)
        };
        radios[2] = Radio::tuned(30e6, Modulation::FM);
        data.save_preset(
            Preset::new("TOC", 2, &radios, &DEFAULT_MONITOR)
                .ok()
                .unwrap(),
        )
        .unwrap();
        data
    }
    #[test]
    fn v2_migration_preserves_radios_and_adds_explicit_saved_ptt_binding() {
        let mut old = serde_json::to_value(example()).unwrap();
        old["version"] = 2.into();
        old.as_object_mut().unwrap().remove("preferences");
        for radio in old["presets"][0]["radios"].as_array_mut().unwrap() {
            radio.as_object_mut().unwrap().remove("routing");
        }
        old["presets"][0]["radios"][0]["volume"] = 42.into();
        let mut data = decode(&serde_json::to_vec(&old).unwrap()).unwrap();
        assert_eq!(data.preferences.ptt_key, 8);
        assert_eq!(data.presets[0].monitor().unwrap()[1].percent(), 42);
        data.preferences.ptt_key = 12;
        assert_eq!(decode(&serde_json::to_vec(&data).unwrap()).unwrap(), data);
        data.preferences.ptt_key = 0;
        assert!(decode(&serde_json::to_vec(&data).unwrap()).is_ok());
        data.preferences.ptt_key = 13;
        assert_eq!(
            decode(&serde_json::to_vec(&data).unwrap()),
            Err(StoreError::Invalid)
        );
        old["preferences"] = serde_json::json!({"ptt_key": 9});
        assert_eq!(
            decode(&serde_json::to_vec(&old).unwrap()),
            Err(StoreError::Invalid)
        );
    }
    #[test]
    fn routing_roundtrips_and_old_profiles_and_transfers_default_to_both() {
        let mut data = example();
        data.preferences.ptt_key = 12;
        let mut monitor = DEFAULT_MONITOR;
        monitor[1] = RadioLevel::new(72, false)
            .unwrap()
            .with_routing(Routing::Left);
        monitor[2] = RadioLevel::new(35, true)
            .unwrap()
            .with_routing(Routing::Right);
        data.save_preset(Preset::new("TOC", 2, &data.presets[0].radios(), &monitor).unwrap())
            .unwrap();
        assert_eq!(decode(&serde_json::to_vec(&data).unwrap()).unwrap(), data);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("routing.json");
        export_presets(&path, &data).unwrap();
        assert_eq!(
            import_presets(&path, Data::default()).unwrap().presets[0]
                .monitor()
                .unwrap(),
            monitor
        );
        let mut legacy = serde_json::to_value(&data).unwrap();
        legacy["version"] = 3.into();
        legacy["preferences"] = serde_json::json!({"ptt_key": 12});
        for radio in legacy["presets"][0]["radios"].as_array_mut().unwrap() {
            radio.as_object_mut().unwrap().remove("routing");
        }
        let migrated = decode(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(migrated.preferences.ptt_key, 12);
        assert_eq!(
            migrated.presets[0].monitor().unwrap()[2],
            RadioLevel::new(35, true).unwrap()
        );
        legacy["presets"][0]["radios"][0]["routing"] = "left".into();
        assert_eq!(
            decode(&serde_json::to_vec(&legacy).unwrap()),
            Err(StoreError::Invalid),
            "v3 accepted a v4 field"
        );
        let mut bundle: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        bundle["version"] = 1.into();
        for radio in bundle["presets"][0]["radios"].as_array_mut().unwrap() {
            radio.as_object_mut().unwrap().remove("routing");
        }
        std::fs::write(&path, serde_json::to_vec(&bundle).unwrap()).unwrap();
        let migrated = import_presets(&path, Data::default()).unwrap();
        assert_eq!(
            migrated.presets[0].monitor().unwrap()[1],
            RadioLevel::new(72, false).unwrap()
        );
        bundle["version"] = 2.into();
        std::fs::write(&path, serde_json::to_vec(&bundle).unwrap()).unwrap();
        assert_eq!(
            import_presets(&path, Data::default()),
            Err(StoreError::Invalid),
            "v2 requires routing"
        );
        let mut invalid = serde_json::to_value(&data).unwrap();
        invalid["presets"][0]["radios"][0]["routing"] = "surround".into();
        assert_eq!(
            decode(&serde_json::to_vec(&invalid).unwrap()),
            Err(StoreError::Invalid)
        );
    }
    #[test]
    fn preset_transfer_omits_connections_and_never_overwrites_files_or_name_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("presets.json");
        let data = example();
        export_presets(&path, &data).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        for excluded in [
            "servers",
            "127.0.0.1",
            "device:a",
            "preferences",
            "password",
            "SECRET",
            "callsign",
        ] {
            assert!(!text.contains(excluded));
        }
        let imported = import_presets(&path, Data::default()).unwrap();
        assert_eq!(imported.presets, data.presets);
        assert!(imported.servers.is_empty());
        assert_eq!(
            import_presets(&path, data.clone()),
            Err(StoreError::NameConflict)
        );
        assert_eq!(
            export_presets(&path, &Data::default()),
            Err(StoreError::Exists)
        );
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let profile_path = dir.path().join("profiles.json");
        let mut s = store(profile_path.clone());
        s.load().unwrap();
        s.save(imported).unwrap();
        assert_eq!(store(profile_path).load().unwrap().presets, data.presets);
    }
    #[test]
    fn malformed_and_oversized_transfers_do_not_change_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("presets.json");
        let mut value = serde_json::to_value(Bundle {
            format: BUNDLE_FORMAT.into(),
            version: 2,
            presets: example().presets,
        })
        .unwrap();
        value["presets"][0]["radios"][9]["volume"] = 101.into();
        let invalid_radio = serde_json::to_vec(&value).unwrap();
        value["presets"][0]["radios"][9]["volume"] = 100.into();
        value["password"] = "do not accept".into();
        for bytes in [
            invalid_radio,
            serde_json::to_vec(&value).unwrap(),
            vec![b' '; MAX_BYTES + 1],
            br#"{"format":"unknown","version":1,"presets":[]}"#.to_vec(),
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(import_presets(&path, example()), Err(StoreError::Invalid));
        }
    }
    #[test]
    fn persisted_setup_roundtrips_without_credentials_and_replaces_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        let mut s = store(path.clone());
        assert_eq!(s.load().unwrap(), Data::default());
        s.save(example()).unwrap();
        let disk = std::fs::read_to_string(&path).unwrap();
        assert!(
            !disk.contains("SECRET") && !disk.contains("password") && !disk.contains("generation")
        );
        assert_eq!(store(path.clone()).load().unwrap(), example());
        let radios = example().presets[0].radios();
        assert_eq!(radios[1].name, "Command");
        assert_eq!(radios[1].guard_hz, 243e6);
        assert_eq!(radios[2].modulation, Modulation::FM);
        s.save(Data::default()).unwrap();
        assert_eq!(store(path.clone()).load().unwrap(), Data::default());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    #[test]
    fn corrupt_future_oversized_and_invalid_files_are_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        let mut invalid = example();
        invalid.presets[0].radios[0].key = Some(0);
        let mut duplicate = example();
        duplicate.servers.push(duplicate.servers[0].clone());
        for bytes in [
            b"{broken".to_vec(),
            br#"{"version":6,"servers":[],"presets":[]}"#.to_vec(),
            vec![b' '; MAX_BYTES + 1],
            serde_json::to_vec(&invalid).unwrap(),
            serde_json::to_vec(&duplicate).unwrap(),
            br#"{"version":1,"servers":[],"presets":[],"password":"secret"}"#.to_vec(),
        ] {
            std::fs::write(&path, &bytes).unwrap();
            let mut s = store(path.clone());
            assert_eq!(s.load(), Err(StoreError::Invalid));
            assert_eq!(s.save(example()), Err(StoreError::Invalid));
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }
    #[test]
    fn concurrent_instances_cannot_overwrite_unseen_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        let mut first = store(path.clone());
        let mut second = store(path.clone());
        first.load().unwrap();
        second.load().unwrap();
        first.save(example()).unwrap();
        assert_eq!(second.save(Data::default()), Err(StoreError::Conflict));
        assert_eq!(second.load().unwrap(), example());
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))
            .unwrap();
        lock.try_lock().unwrap();
        assert_eq!(second.save(Data::default()), Err(StoreError::Busy));
        drop(lock);
        second.save(Data::default()).unwrap();
    }
    #[test]
    fn invalid_preset_is_rejected_as_a_whole() {
        let mut radios = example().presets[0].radios();
        radios[10].frequency_hz = f64::NAN;
        assert!(Preset::new("Bad", 1, &radios, &DEFAULT_MONITOR).is_err());
        assert!(Preset::new("", 1, &disabled_radios(), &DEFAULT_MONITOR).is_err());
        assert!(Preset::new("Bad", 0, &disabled_radios(), &DEFAULT_MONITOR).is_err());
        let mut data = Data::default();
        for index in 0..MAX_ENTRIES {
            data.save_preset(
                Preset::new(&index.to_string(), 1, &disabled_radios(), &DEFAULT_MONITOR).unwrap(),
            )
            .unwrap();
        }
        assert!(
            data.save_preset(
                Preset::new("overflow", 1, &disabled_radios(), &DEFAULT_MONITOR).unwrap()
            )
            .is_err()
        );
        assert!(
            data.save_preset(Preset::new("0", 2, &disabled_radios(), &DEFAULT_MONITOR).unwrap())
                .is_ok()
        );
    }
    #[test]
    fn v1_migrates_in_memory_only_and_receive_levels_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.json");
        let mut old = serde_json::to_value(example()).unwrap();
        old["version"] = 1.into();
        old.as_object_mut().unwrap().remove("preferences");
        for radio in old["presets"][0]["radios"].as_array_mut().unwrap() {
            radio.as_object_mut().unwrap().remove("routing");
        }
        for radio in old["presets"][0]["radios"].as_array_mut().unwrap() {
            radio.as_object_mut().unwrap().remove("volume");
            radio.as_object_mut().unwrap().remove("muted");
        }
        let bytes = serde_json::to_vec(&old).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let mut s = store(path.clone());
        let mut data = s.load().unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "loading rewrote v1");
        assert_eq!(data.presets[0].monitor().unwrap(), DEFAULT_MONITOR);
        let mut levels = DEFAULT_MONITOR;
        levels[2] = RadioLevel::new(35, true).unwrap();
        data.save_preset(Preset::new("TOC", 2, &data.presets[0].radios(), &levels).unwrap())
            .unwrap();
        s.save(data).unwrap();
        let data = store(path.clone()).load().unwrap();
        assert_eq!(data.version, 5);
        assert_eq!(data.presets[0].monitor().unwrap(), levels);
        let mut invalid = serde_json::to_value(data).unwrap();
        invalid["presets"][0]["radios"][1]["volume"] = 101.into();
        let bytes = serde_json::to_vec(&invalid).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(store(path.clone()).load(), Err(StoreError::Invalid));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        old["presets"][0]["radios"][0]["muted"] = true.into();
        assert_eq!(
            decode(&serde_json::to_vec(&old).unwrap()),
            Err(StoreError::Invalid),
            "invalid v1 field was silently migrated"
        );
    }
    #[test]
    fn worker_joins_with_unread_result() {
        let dir = tempfile::tempdir().unwrap();
        drop(Worker::start(Some(dir.path().join("profiles.json"))).unwrap());
    }
    #[test]
    fn background_bindings_roundtrip_and_v4_migrates_without_enabling_input() {
        let mut data = example();
        data.preferences.background_mode = 2;
        data.preferences.background_key = 12;
        data.preferences.background_modifiers = 6;
        data.preferences.joystick_guid = "0123456789abcdef0123456789abcdef".into();
        data.preferences.joystick_button = 127;
        let bytes = serde_json::to_vec(&data).unwrap();
        assert_eq!(decode(&bytes).unwrap(), data);
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(!text.contains("instance"));
        assert!(!text.contains("held"));
        assert!(!text.contains("background_enabled"));
        let mut legacy = serde_json::to_value(&data).unwrap();
        legacy["version"] = 4.into();
        legacy["preferences"] = serde_json::json!({"ptt_key": 12});
        let migrated = decode(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(migrated.presets, data.presets);
        assert_eq!(
            migrated.preferences,
            Preferences {
                ptt_key: 12,
                ..Default::default()
            }
        );
        legacy["preferences"]["background_mode"] = 2.into();
        assert_eq!(
            decode(&serde_json::to_vec(&legacy).unwrap()),
            Err(StoreError::Invalid)
        );
        for field in ["background_key", "background_modifiers", "joystick_button"] {
            let mut invalid = serde_json::to_value(&data).unwrap();
            invalid["preferences"][field] = 255.into();
            assert_eq!(
                decode(&serde_json::to_vec(&invalid).unwrap()),
                Err(StoreError::Invalid)
            );
        }
    }
}

#[cfg(all(test, target_os = "linux"))]
#[path = "dialog_tests.rs"]
mod dialog_tests;
