#![forbid(unsafe_code)]
//! Versioned user configuration. No passwords, session identity or input state.
//! All disk I/O runs on one owned worker; writes replace atomically under a
//! nonblocking sidecar lock and reject changes made by another application.
use crate::session::{Config, Problem};
use serde::{Deserialize, Serialize};
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
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetRadio {
    name: String,
    frequency_hz: f64,
    guard_hz: f64,
    mode: Mode,
    key: Option<u8>,
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
pub struct Data {
    version: u32,
    pub servers: Vec<Server>,
    pub presets: Vec<Preset>,
}
impl Default for Data {
    fn default() -> Self {
        Self {
            version: 1,
            servers: vec![],
            presets: vec![],
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
    ) -> Result<Self, Problem> {
        let preset = Self {
            name: name.trim().into(),
            selected,
            radios: std::array::from_fn(|index| {
                let r = &radios[index + 1];
                PresetRadio {
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
            })
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
        self.version == 1
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
            Some(bytes) => serde_json::from_slice(bytes).map_err(|_| StoreError::Invalid)?,
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
pub enum Command {
    Reload,
    Save(Data),
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
        data.save_preset(Preset::new("TOC", 2, &radios).ok().unwrap())
            .unwrap();
        data
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
            br#"{"version":2,"servers":[],"presets":[]}"#.to_vec(),
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
        assert!(Preset::new("Bad", 1, &radios).is_err());
        assert!(Preset::new("", 1, &disabled_radios()).is_err());
        assert!(Preset::new("Bad", 0, &disabled_radios()).is_err());
        let mut data = Data::default();
        for index in 0..MAX_ENTRIES {
            data.save_preset(Preset::new(&index.to_string(), 1, &disabled_radios()).unwrap())
                .unwrap();
        }
        assert!(
            data.save_preset(Preset::new("overflow", 1, &disabled_radios()).unwrap())
                .is_err()
        );
        assert!(
            data.save_preset(Preset::new("0", 2, &disabled_radios()).unwrap())
                .is_ok()
        );
    }
    #[test]
    fn worker_joins_with_unread_result() {
        let dir = tempfile::tempdir().unwrap();
        drop(Worker::start(Some(dir.path().join("profiles.json"))).unwrap());
    }
}
