#![forbid(unsafe_code)]
use crate::{
    devices,
    input::UiInput,
    keyboard,
    profiles::{self, Command, Data, Preset, Server, Worker},
    session::{Config, Edit, Handle, Problem, frequency},
    ui::{MainWindow, PeerRow, PolicyRow, RadioRow, Strings},
};
use slint::winit_030::{
    EventResult, WinitWindowAccessor,
    winit::event::{ElementState, WindowEvent},
};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};
use srs_client::receive::{DEFAULT_MONITOR, Monitor, RadioLevel, Routing};
use srs_client::{
    RADIO_COUNT, Radio,
    commands::{RadioChange, RadioEdit},
    disabled_radios,
};
use srs_device::native::DeviceInfo;
use srs_protocol::voice::Modulation;
use srs_toc::{Phase, held::Input};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

struct App {
    handle: Option<Handle>,
    diagnostic: Option<srs_device::diagnostic::Probe>,
    ptt: UiInput,
    background: Option<srs_input::Worker>,
    background_revision: u64,
    joysticks: Vec<srs_input::Device>,
    wanted_joystick: String,
    radios: [Radio; RADIO_COUNT],
    inputs: Vec<DeviceInfo>,
    outputs: Vec<DeviceInfo>,
    discovery: Option<devices::Worker>,
    discovering: bool,
    wanted_input: String,
    wanted_output: String,
    missing_input: Option<String>,
    missing_output: Option<String>,
    ptt_key: u8,
    modifiers: bool,
    details: srs_toc::observation::SessionDetails,
    pending: Option<u64>,
    pending_selection: Option<usize>,
    pending_monitor: Option<Monitor>,
    monitor: Monitor,
    storage: Option<Worker>,
    storage_busy: bool,
    load_preferences: bool,
    transfer: i32,
    profiles: Data,
    next_edit: u64,
    stopping: bool,
    last_problem: Problem,
    last_received: [u64; RADIO_COUNT],
    restore_monitor: bool,
    rx_until: [Instant; RADIO_COUNT],
    rx_active: [bool; RADIO_COUNT],
}
fn rows(
    radios: &[Radio; RADIO_COUNT],
    monitor: &Monitor,
    rx: &[bool; RADIO_COUNT],
) -> ModelRc<RadioRow> {
    Rc::new(VecModel::from(
        radios[1..]
            .iter()
            .enumerate()
            .map(|(index, radio)| RadioRow {
                volume: monitor[index + 1].percent() as i32,
                routing: monitor[index + 1].routing() as i32,
                muted: monitor[index + 1].muted(),
                rx: rx[index + 1],
                name: radio.name.as_str().into(),
                frequency: format!("{:.6}", radio.frequency_hz / 1e6).into(),
                guard: if radio.guard_hz > 10_000.0 {
                    format!("{:.6}", radio.guard_hz / 1e6).into()
                } else {
                    SharedString::default()
                },
                key: radio
                    .encryption_key
                    .map(|key| key.to_string())
                    .unwrap_or_default()
                    .into(),
                mode: if radio.modulation == Modulation::FM {
                    "FM"
                } else {
                    "AM"
                }
                .into(),
                enabled: matches!(radio.modulation, Modulation::AM | Modulation::FM),
            })
            .collect::<Vec<_>>(),
    ))
    .into()
}
fn device_model(devices: &[DeviceInfo], default_label: SharedString) -> ModelRc<SharedString> {
    let mut labels = vec![default_label];
    labels.extend(devices.iter().map(|device| device.name.as_str().into()));
    Rc::new(VecModel::from(labels)).into()
}
fn device_id(devices: &[DeviceInfo], index: i32) -> Result<String, Problem> {
    if index == 0 {
        return Ok("default".into());
    }
    usize::try_from(index)
        .ok()
        .and_then(|i| i.checked_sub(1))
        .and_then(|i| devices.get(i))
        .map(|d| d.id.clone())
        .ok_or(Problem::Device)
}
fn device_index(devices: &[DeviceInfo], id: &str) -> Option<i32> {
    if id == "default" {
        Some(0)
    } else {
        devices
            .iter()
            .position(|d| d.id == id)
            .map(|i| i as i32 + 1)
    }
}
fn restored_device(
    devices: &[DeviceInfo],
    id: &str,
    ui: &MainWindow,
) -> (ModelRc<SharedString>, i32) {
    let strings = ui.global::<Strings>();
    let mut labels = vec![strings.get_system_default()];
    labels.extend(devices.iter().map(|d| d.name.as_str().into()));
    let index = device_index(devices, id).unwrap_or_else(|| {
        labels.push(strings.get_missing_device());
        devices.len() as i32 + 1
    });
    (Rc::new(VecModel::from(labels)).into(), index)
}
fn names(values: impl Iterator<Item = String>) -> ModelRc<SharedString> {
    Rc::new(VecModel::from(
        values.map(SharedString::from).collect::<Vec<_>>(),
    ))
    .into()
}
impl App {
    fn stop_diagnostic(&mut self) {
        if let Some(probe) = &mut self.diagnostic {
            probe.cancel();
        }
    }
    fn diagnostic_action(&mut self, ui: &MainWindow, action: i32) {
        self.invalidate(ui);
        if action == 0 {
            self.stop_diagnostic();
            return;
        }
        if self.handle.is_some() || self.discovering || self.diagnostic.is_some() {
            return;
        }
        use srs_device::diagnostic::{Mode, Probe};
        let mode = match action {
            1 => Mode::Capture,
            2 => Mode::Loopback,
            3 => Mode::Stereo,
            _ => return,
        };
        let selections = device_id(&self.inputs, ui.get_input_index()).and_then(|input| {
            device_id(&self.outputs, ui.get_output_index()).map(|output| (input, output))
        });
        let Ok((input, output)) = selections else {
            ui.set_diagnostic_state(5);
            return;
        };
        match Probe::start(mode, input, output) {
            Ok(probe) => {
                self.diagnostic = Some(probe);
                ui.set_diagnostic_active(true);
                ui.set_diagnostic_mode(action);
                ui.set_diagnostic_state(0);
                ui.set_diagnostic_clipped("0".into());
                ui.set_diagnostic_peak(0.);
            }
            Err(_) => ui.set_diagnostic_state(5),
        }
    }
    fn poll_diagnostic(&mut self, ui: &MainWindow) {
        if let Some(probe) = &self.diagnostic {
            probe.renew();
            let snapshot = probe.snapshot();
            ui.set_diagnostic_state(snapshot.state as i32);
            ui.set_diagnostic_peak(snapshot.peak_percent as f32);
            ui.set_diagnostic_clipped(snapshot.clipped.to_string().into());
            ui.set_diagnostic_seconds(snapshot.elapsed as i32);
            ui.set_diagnostic_channel(snapshot.channel as i32);
            ui.set_diagnostic_mono(snapshot.mono);
            if probe.finished() {
                self.diagnostic.take();
                ui.set_diagnostic_active(false);
            }
        }
    }
    fn controls(&self, ui: &MainWindow) -> profiles::Preferences {
        profiles::Preferences {
            ptt_key: self.ptt_key,
            background_mode: ui.get_background_mode().clamp(0, 2) as u8,
            background_key: (ui.get_background_key().clamp(0, 11) + 1) as u8,
            background_modifiers: u8::from(ui.get_background_shift())
                | (u8::from(ui.get_background_ctrl()) << 1)
                | (u8::from(ui.get_background_alt()) << 2)
                | (u8::from(ui.get_background_super()) << 3),
            joystick_guid: self
                .joysticks
                .get(ui.get_joystick_index() as usize)
                .map_or_else(|| self.wanted_joystick.clone(), |d| d.guid.clone()),
            joystick_button: (ui.get_joystick_button().clamp(1, 128) - 1) as u8,
        }
    }
    fn stop_background(&mut self, ui: &MainWindow) {
        self.invalidate(ui);
        ui.set_background_enabled(false);
        ui.set_background_trigger("".into());
        if let Some(worker) = &mut self.background
            && let Ok(revision) = worker.configure(srs_input::Mode::Off)
        {
            self.background_revision = revision;
        }
        ui.set_background_state(0);
    }
    fn background_action(&mut self, ui: &MainWindow, action: i32) {
        let was_enabled = ui.get_background_enabled();
        self.stop_background(ui);
        ui.set_controls_saved(self.controls(ui) == self.profiles.preferences);
        let controls = self.controls(ui);
        let mode = match action {
            0 => return,
            1 if was_enabled => return,
            1 if controls.background_mode == 1 => srs_input::Mode::Keyboard {
                description: ui.global::<Strings>().get_portal_ptt().to_string(),
                key: controls.background_key,
                modifiers: controls.background_modifiers,
            },
            1 if controls.background_mode == 2 => {
                let Some(device) = self.joysticks.get(ui.get_joystick_index() as usize) else {
                    ui.set_background_state(6);
                    return;
                };
                if u32::from(controls.joystick_button) >= device.buttons {
                    ui.set_background_state(6);
                    return;
                }
                srs_input::Mode::Joystick {
                    instance: device.instance,
                    button: u32::from(controls.joystick_button),
                }
            }
            2 => srs_input::Mode::ScanJoysticks,
            _ => return,
        };
        let Some(worker) = &mut self.background else {
            ui.set_background_state(3);
            return;
        };
        match worker.configure(mode) {
            Ok(revision) => {
                self.background_revision = revision;
                ui.set_background_enabled(action == 1);
                ui.set_background_state(1);
            }
            Err(_) => ui.set_background_state(6),
        }
    }
    fn poll_background(&mut self, ui: &MainWindow) -> Option<srs_input::Snapshot> {
        let worker = self.background.as_ref()?;
        if worker.view.has_changed().is_err() {
            ui.set_background_enabled(false);
            ui.set_background_state(5);
            return None;
        }
        let snapshot = worker.view.borrow().clone();
        if snapshot.revision != self.background_revision {
            return None;
        }
        if ui.get_background_enabled()
            && snapshot.state == srs_input::State::Ready
            && (snapshot.at.elapsed() >= srs_input::FRESHNESS
                || !std::time::SystemTime::now()
                    .duration_since(snapshot.wall)
                    .is_ok_and(|age| age < srs_input::FRESHNESS))
        {
            self.stop_background(ui);
            ui.set_background_state(5);
            return None;
        }
        let changed = self
            .joysticks
            .iter()
            .map(|d| d.instance)
            .ne(snapshot.devices.iter().map(|d| d.instance));
        if changed {
            let selected = self
                .joysticks
                .get(ui.get_joystick_index() as usize)
                .map(|d| d.instance);
            let matches: Vec<_> = snapshot
                .devices
                .iter()
                .enumerate()
                .filter(|(_, d)| d.guid == self.wanted_joystick)
                .map(|(i, _)| i)
                .collect();
            let index = snapshot
                .devices
                .iter()
                .position(|d| Some(d.instance) == selected)
                .or_else(|| (matches.len() == 1).then(|| matches[0]));
            ui.set_joysticks(names(
                snapshot
                    .devices
                    .iter()
                    .map(|d| format!("{} · {}", d.name, d.instance)),
            ));
            ui.set_joystick_index(index.map_or(-1, |i| i as i32));
            self.joysticks = snapshot.devices.clone();
        }
        ui.set_background_state(snapshot.state as i32);
        ui.set_background_trigger(snapshot.trigger.as_str().into());
        if matches!(
            snapshot.state,
            srs_input::State::Unavailable
                | srs_input::State::Denied
                | srs_input::State::Disconnected
                | srs_input::State::Failed
        ) {
            ui.set_background_enabled(false);
        }
        Some(snapshot)
    }
    fn rebind(&mut self, ui: &MainWindow, key: i32) {
        if let Ok(key) = u8::try_from(key)
            && key <= 12
        {
            self.invalidate(ui);
            self.ptt_key = key;
            ui.set_ptt_key(key as i32);
            ui.set_controls_saved(self.controls(ui) == self.profiles.preferences);
        }
    }
    fn refresh_devices(&mut self, ui: &MainWindow) {
        self.invalidate(ui);
        if self.handle.is_some() || self.discovering || self.diagnostic.is_some() {
            return;
        }
        let selected = |devices: &[DeviceInfo], index: i32, missing: &Option<String>| {
            device_id(devices, index).ok().or_else(|| {
                (index == devices.len() as i32 + 1)
                    .then(|| missing.clone())
                    .flatten()
            })
        };
        let Some(input) = selected(&self.inputs, ui.get_input_index(), &self.missing_input) else {
            ui.set_problem(Problem::Device as i32);
            return;
        };
        let Some(output) = selected(&self.outputs, ui.get_output_index(), &self.missing_output)
        else {
            ui.set_problem(Problem::Device as i32);
            return;
        };
        self.wanted_input = input;
        self.wanted_output = output;
        self.discovering = self
            .discovery
            .as_ref()
            .is_some_and(|worker| worker.refresh().is_ok());
        ui.set_device_busy(self.discovering);
        if !self.discovering {
            ui.set_problem(Problem::Device as i32);
        }
    }
    fn poll_devices(&mut self, ui: &MainWindow) {
        if !self.discovering {
            return;
        }
        let Some(result) = self.discovery.as_ref().and_then(devices::Worker::poll) else {
            return;
        };
        self.discovering = false;
        ui.set_device_busy(false);
        match result {
            Ok(list) => {
                self.inputs = list.inputs;
                self.outputs = list.outputs;
                let (inputs, input) = restored_device(&self.inputs, &self.wanted_input, ui);
                let (outputs, output) = restored_device(&self.outputs, &self.wanted_output, ui);
                self.missing_input = device_index(&self.inputs, &self.wanted_input)
                    .is_none()
                    .then(|| self.wanted_input.clone());
                self.missing_output = device_index(&self.outputs, &self.wanted_output)
                    .is_none()
                    .then(|| self.wanted_output.clone());
                ui.set_inputs(inputs);
                ui.set_outputs(outputs);
                ui.set_input_index(input);
                ui.set_output_index(output);
                ui.set_problem(
                    if self.missing_input.is_some() || self.missing_output.is_some() {
                        Problem::Device as i32
                    } else {
                        0
                    },
                );
            }
            Err(_) => ui.set_problem(Problem::Device as i32),
        }
    }
    fn publish_details(&mut self, ui: &MainWindow, details: srs_toc::observation::SessionDetails) {
        if self.details == details {
            return;
        }
        ui.set_server_version(details.version.as_str().into());
        ui.set_coalition(details.coalition.map_or(-1, |c| c as i32));
        ui.set_client_count(details.total_clients.to_string().into());
        ui.set_peers(
            Rc::new(VecModel::from(
                details
                    .clients
                    .iter()
                    .map(|peer| PeerRow {
                        name: peer.name.as_str().into(),
                        coalition: peer.coalition.map_or(-1, i32::from),
                    })
                    .collect::<Vec<_>>(),
            ))
            .into(),
        );
        ui.set_policy(
            Rc::new(VecModel::from(
                details
                    .policy
                    .iter()
                    .enumerate()
                    .map(|(index, value)| PolicyRow {
                        index: index as i32,
                        state: value.map_or(0, |value| if value { 1 } else { 2 }),
                    })
                    .collect::<Vec<_>>(),
            ))
            .into(),
        );
        ui.set_node_limit(
            details
                .retransmission_limit
                .map(|n| n.to_string())
                .unwrap_or_default()
                .into(),
        );
        self.details = details;
    }

    fn refresh_rows(&self, ui: &MainWindow) {
        ui.set_radios(rows(&self.radios, &self.monitor, &self.rx_active));
    }
    fn edit_monitor(&mut self, ui: &MainWindow, volume: i32, muted: bool) {
        self.invalidate(ui);
        if self.pending.is_some() || (self.handle.is_some() && !ui.get_ready()) {
            return;
        }
        let Some(level) = u8::try_from(volume)
            .ok()
            .and_then(|v| RadioLevel::new(v, muted))
        else {
            return;
        };
        let Some(radio) = self.monitor.get_mut(ui.get_selected() as usize) else {
            return;
        };
        *radio = level.with_routing(radio.routing());
        if let Some(handle) = &self.handle {
            handle.monitor.send_replace(self.monitor);
        }
        self.refresh_rows(ui);
        ui.set_volume(volume as f32);
        ui.set_muted(muted);
    }

    fn edit_routing(&mut self, ui: &MainWindow, routing: i32) {
        self.invalidate(ui);
        if self.pending.is_some() || (self.handle.is_some() && !ui.get_ready()) {
            return;
        }
        let routing = match routing {
            0 => Routing::Both,
            1 => Routing::Left,
            2 => Routing::Right,
            _ => return,
        };
        let Some(level) = self.monitor.get_mut(ui.get_selected() as usize) else {
            return;
        };
        *level = level.with_routing(routing);
        if let Some(handle) = &self.handle {
            handle.monitor.send_replace(self.monitor);
        }
        self.refresh_rows(ui);
        ui.set_routing(routing as i32);
    }

    fn publish(&self, ui: &MainWindow) {
        if let Some(handle) = &self.handle {
            handle
                .input
                .send_replace(self.ptt.intent(ui.get_selected() as usize, Instant::now()));
        }
    }
    fn invalidate(&mut self, ui: &MainWindow) {
        self.ptt.invalidate(Instant::now());
        self.publish(ui);
        ui.set_armed(false);
    }
    fn event(&mut self, ui: &MainWindow, source: usize, event: Input) {
        if self.transfer != 0 {
            self.invalidate(ui);
            return;
        }
        if ui.get_ready() && self.pending.is_none() && !self.stopping {
            self.ptt.event(source, event, Instant::now());
            self.publish(ui);
        } else {
            self.invalidate(ui);
        }
    }
    fn editor(&self, ui: &MainWindow) {
        if let Some(radio) = self.radios.get(ui.get_selected() as usize) {
            ui.set_frequency(if radio.frequency_hz > 10_000.0 {
                format!("{:.6}", radio.frequency_hz / 1e6).into()
            } else {
                "251.000".into()
            });
            ui.set_guard(if radio.guard_hz > 10_000.0 {
                format!("{:.6}", radio.guard_hz / 1e6).into()
            } else {
                "".into()
            });
            ui.set_key(
                radio
                    .encryption_key
                    .map(|key| key.to_string())
                    .unwrap_or_default()
                    .into(),
            );
            ui.set_radio_name(radio.name.as_str().into());
            let level = self.monitor[ui.get_selected() as usize];
            ui.set_volume(level.percent() as f32);
            ui.set_muted(level.muted());
            ui.set_routing(level.routing() as i32);
            ui.set_modulation(i32::from(radio.modulation == Modulation::FM));
        }
    }
    fn connect(&mut self, ui: &MainWindow) {
        if self.handle.is_some() || self.discovering || self.diagnostic.is_some() {
            return;
        }
        self.invalidate(ui);
        let password = ui.get_password().to_string();
        ui.set_password("".into());
        let result = self.config(ui, password).and_then(|mut config| {
            config.radios = self.radios.clone();
            config.monitor = self.monitor;
            config.retry = ui.get_retry();
            Handle::start(
                config,
                self.ptt.intent(ui.get_selected() as usize, Instant::now()),
            )
        });
        match result {
            Ok(handle) => {
                self.handle = Some(handle);
                self.stopping = false;
                self.pending = None;
                self.last_problem = Problem::None;
                self.last_received = [0; RADIO_COUNT];
                self.restore_monitor = false;
                self.rx_until = [Instant::now(); RADIO_COUNT];
                self.rx_active = [false; RADIO_COUNT];
                self.refresh_rows(ui);
                ui.set_problem(0);
                ui.set_busy(true);
                ui.set_page(0);
                ui.set_phase(1);
                ui.set_sent("0".into());
                ui.set_received("0".into());
            }
            Err(problem) => ui.set_problem(problem as i32),
        }
    }
    fn disconnect(&mut self, ui: &MainWindow) {
        self.invalidate(ui);
        if let Some(handle) = &self.handle {
            handle.stop();
        }
        self.stopping = true;
        ui.set_ready(false);
        ui.set_phase(5);
    }
    fn edit(&mut self, ui: &MainWindow, kind: i32) {
        self.invalidate(ui);
        if self.pending.is_some() || (self.handle.is_some() && !ui.get_ready()) {
            return;
        }
        let slot = ui.get_selected() as usize;
        let change = match kind {
            0 => frequency(&ui.get_frequency()).map(|hz| RadioChange::Tune {
                slot,
                hz,
                modulation: if ui.get_modulation() == 1 {
                    Modulation::FM
                } else {
                    Modulation::AM
                },
            }),
            1 => (if ui.get_guard().trim().is_empty() {
                Ok(1.0)
            } else {
                frequency(&ui.get_guard())
            })
            .map(|hz| RadioChange::Guard { slot, hz }),
            2 => {
                let key = ui.get_key();
                if key.trim().is_empty() {
                    Ok(RadioChange::Key { slot, key: None })
                } else {
                    key.trim()
                        .parse::<u8>()
                        .ok()
                        .filter(|key| *key != 0)
                        .map(|key| RadioChange::Key {
                            slot,
                            key: Some(key),
                        })
                        .ok_or(Problem::Radio)
                }
            }
            3 => Ok(RadioChange::Disable { slot }),
            _ => Err(Problem::Radio),
        };
        let change = if kind == 4 {
            let name = ui.get_radio_name().trim().to_owned();
            if name.len() > 64 || name.chars().any(char::is_control) {
                Err(Problem::Profile)
            } else {
                Ok(RadioEdit::Name { slot, name })
            }
        } else {
            change.map(RadioEdit::from)
        };
        let result = change.and_then(|change| self.apply_edit(ui, change, None, None));
        ui.set_problem(result.err().unwrap_or(Problem::None) as i32);
    }
    fn apply_edit(
        &mut self,
        ui: &MainWindow,
        change: RadioEdit,
        selected: Option<usize>,
        monitor: Option<Monitor>,
    ) -> Result<(), Problem> {
        self.invalidate(ui);
        if self.pending.is_some() || self.stopping && self.handle.is_some() {
            return Err(Problem::Backlog);
        }
        if let Some(handle) = &self.handle {
            if !ui.get_ready() {
                return Err(Problem::Session);
            }
            self.next_edit += 1;
            handle
                .edits
                .try_send(Edit {
                    id: self.next_edit,
                    at: Instant::now(),
                    change,
                    monitor,
                })
                .map_err(|_| Problem::Backlog)?;
            self.pending = Some(self.next_edit);
            self.pending_selection = selected;
            self.pending_monitor = monitor;
            ui.set_editing(true);
        } else {
            let radios = change.apply_to(&self.radios).map_err(|_| Problem::Radio)?;
            Preset::new(
                "validation",
                selected.unwrap_or(ui.get_selected() as usize),
                &radios,
                &monitor.unwrap_or(self.monitor),
            )?;
            self.radios = radios;
            if let Some(monitor) = monitor {
                self.monitor = monitor;
            }
            if let Some(selected) = selected {
                ui.set_selected(selected as i32);
            }
            self.refresh_rows(ui);
            self.editor(ui);
        }
        Ok(())
    }
    fn config(&self, ui: &MainWindow, password: String) -> Result<Config, Problem> {
        Config::parse(
            &ui.get_server(),
            &ui.get_callsign(),
            password,
            device_id(&self.inputs, ui.get_input_index())?,
            device_id(&self.outputs, ui.get_output_index())?,
        )
    }
    fn store_command(&mut self, ui: &MainWindow, command: Command) {
        let result = self
            .storage
            .as_ref()
            .ok_or(profiles::StoreError::Io)
            .and_then(|worker| worker.send(command));
        self.storage_busy = result.is_ok();
        ui.set_storage_busy(self.storage_busy);
        ui.set_storage_status(result.err().map_or(1, |e| e as i32));
    }
    fn profile_action(&mut self, ui: &MainWindow, action: i32) {
        self.invalidate(ui);
        if self.storage_busy
            || self.pending.is_some()
            || self.discovering
            || self.diagnostic.is_some()
        {
            return;
        }
        if action == 6 {
            self.load_preferences = true;
            self.store_command(ui, Command::Reload);
            return;
        }
        if action == 8 || action == 9 {
            let title = if action == 8 {
                ui.global::<Strings>().get_import_presets()
            } else {
                ui.global::<Strings>().get_export_presets()
            }
            .to_string();
            let filter = ui.global::<Strings>().get_preset_file().to_string();
            let data = self.profiles.clone();
            self.store_command(
                ui,
                if action == 8 {
                    Command::Import {
                        data,
                        title,
                        filter,
                    }
                } else {
                    Command::Export {
                        data,
                        title,
                        filter,
                    }
                },
            );
            if self.storage_busy {
                self.transfer = action;
                ui.set_transfer_busy(true);
            }
            return;
        }
        let result = self.profile_change(ui, action);
        ui.set_problem(result.err().unwrap_or(Problem::None) as i32);
    }
    fn profile_change(&mut self, ui: &MainWindow, action: i32) -> Result<(), Problem> {
        let mut data = self.profiles.clone();
        let server_index = usize::try_from(ui.get_server_index()).map_err(|_| Problem::Profile);
        let preset_index = usize::try_from(ui.get_preset_index()).map_err(|_| Problem::Profile);
        match action {
            0 if self.handle.is_none() => data.save_server(Server::new(
                &ui.get_server_name(),
                &self.config(ui, String::new())?,
            )?)?,
            1 if self.handle.is_none() => {
                let server = data.servers.get(server_index?).ok_or(Problem::Profile)?;
                ui.set_server(server.address.as_str().into());
                ui.set_callsign(server.callsign.as_str().into());
                ui.set_server_name(server.name.as_str().into());
                self.missing_input = device_index(&self.inputs, &server.input)
                    .is_none()
                    .then(|| server.input.clone());
                self.missing_output = device_index(&self.outputs, &server.output)
                    .is_none()
                    .then(|| server.output.clone());
                ui.set_password("".into());
                let (inputs, input) = restored_device(&self.inputs, &server.input, ui);
                let (outputs, output) = restored_device(&self.outputs, &server.output, ui);
                ui.set_inputs(inputs);
                ui.set_outputs(outputs);
                ui.set_input_index(input);
                ui.set_output_index(output);
                device_id(&self.inputs, input)?;
                device_id(&self.outputs, output)?;
                return Ok(());
            }
            2 if self.handle.is_none() => {
                let index = server_index?;
                if index >= data.servers.len() {
                    return Err(Problem::Profile);
                }
                data.servers.remove(index);
            }
            3 => data.save_preset(Preset::new(
                &ui.get_preset_name(),
                ui.get_selected() as usize,
                &self.radios,
                &self.monitor,
            )?)?,
            4 => {
                let preset = data.presets.get(preset_index?).ok_or(Problem::Profile)?;
                self.apply_edit(
                    ui,
                    RadioEdit::Replace(Box::new(preset.radios())),
                    Some(preset.selected),
                    Some(preset.monitor()?),
                )?;
                ui.set_preset_name(preset.name.as_str().into());
                return Ok(());
            }
            5 => {
                let index = preset_index?;
                if index >= data.presets.len() {
                    return Err(Problem::Profile);
                }
                data.presets.remove(index);
            }
            7 => data.preferences = self.controls(ui),
            _ => return Err(Problem::Profile),
        }
        self.store_command(ui, Command::Save(data));
        Ok(())
    }
    fn poll_storage(&mut self, ui: &MainWindow) {
        if !self.storage_busy {
            return;
        }
        let Some(result) = self.storage.as_ref().and_then(Worker::poll) else {
            return;
        };
        self.storage_busy = false;
        ui.set_storage_busy(false);
        let transfer = std::mem::take(&mut self.transfer);
        let load_preferences = std::mem::take(&mut self.load_preferences);
        ui.set_transfer_busy(false);
        match result {
            Ok(data) => {
                if load_preferences {
                    self.stop_background(ui);
                    self.rebind(ui, data.preferences.ptt_key as i32);
                    let prefs = &data.preferences;
                    ui.set_background_mode(i32::from(prefs.background_mode));
                    ui.set_background_key(i32::from(prefs.background_key) - 1);
                    ui.set_background_shift(prefs.background_modifiers & 1 != 0);
                    ui.set_background_ctrl(prefs.background_modifiers & 2 != 0);
                    ui.set_background_alt(prefs.background_modifiers & 4 != 0);
                    ui.set_background_super(prefs.background_modifiers & 8 != 0);
                    ui.set_joystick_button(i32::from(prefs.joystick_button) + 1);
                    self.wanted_joystick = prefs.joystick_guid.clone();
                }
                self.profiles = data;
                ui.set_controls_saved(self.controls(ui) == self.profiles.preferences);
                ui.set_saved_servers(names(self.profiles.servers.iter().map(|s| s.name.clone())));
                ui.set_saved_presets(names(self.profiles.presets.iter().map(|p| p.name.clone())));
                ui.set_server_index(
                    self.profiles
                        .servers
                        .iter()
                        .position(|s| s.name == ui.get_server_name().as_str())
                        .map_or(-1, |i| i as i32),
                );
                ui.set_preset_index(
                    self.profiles
                        .presets
                        .iter()
                        .position(|p| p.name == ui.get_preset_name().as_str())
                        .map_or(-1, |i| i as i32),
                );
                ui.set_storage_status(if transfer == 8 {
                    10
                } else if transfer == 9 {
                    11
                } else {
                    0
                });
            }
            Err(error) => ui.set_storage_status(error as i32),
        }
    }
    fn tick(&mut self, ui: &MainWindow) {
        self.poll_diagnostic(ui);
        self.poll_devices(ui);
        self.poll_storage(ui);
        let now = Instant::now();
        self.ptt.tick(now);
        let background = self.poll_background(ui);
        let Some(handle) = &self.handle else {
            self.ptt.invalidate(now);
            return;
        };
        let snapshot = handle.view.borrow().clone();
        let finished = handle.finished();
        self.publish_details(ui, snapshot.details.clone());
        ui.set_retry_attempt(snapshot.retry_attempt as i32);
        ui.set_rejected(snapshot.status.rejected_packets.to_string().into());
        ui.set_overflows(snapshot.status.playback_overflows.to_string().into());
        ui.set_clipped(snapshot.status.clipped_samples.to_string().into());
        ui.set_speakers(snapshot.status.peak_speakers.to_string().into());
        let ready = !self.stopping
            && matches!(
                snapshot.status.phase,
                Phase::Receiving | Phase::Transmitting
            );
        if !ready && snapshot.retry_attempt > 0 {
            self.restore_monitor = true;
            self.pending = None;
            self.pending_selection = None;
            self.pending_monitor = None;
        }
        if ready && std::mem::take(&mut self.restore_monitor) {
            self.monitor = snapshot.monitor;
            self.refresh_rows(ui);
            self.editor(ui);
        }
        if !ready || self.pending.is_some() {
            self.invalidate(ui);
        }
        let acknowledged = self.pending.is_some_and(|id| id == snapshot.completed_edit);
        if acknowledged {
            self.pending = None;
            if let Some(monitor) = self.pending_monitor.take()
                && snapshot.problem == Problem::None
            {
                self.monitor = monitor;
                self.refresh_rows(ui);
                self.editor(ui);
            }
            if let Some(selected) = self.pending_selection.take()
                && snapshot.problem == Problem::None
            {
                ui.set_selected(selected as i32);
                self.editor(ui);
            }
            self.invalidate(ui);
            ui.set_problem(snapshot.problem as i32);
        }
        if self.radios != snapshot.radios {
            self.invalidate(ui);
            self.radios = snapshot.radios;
            self.refresh_rows(ui);
            self.editor(ui);
        }
        if snapshot.problem != self.last_problem {
            self.last_problem = snapshot.problem;
            ui.set_problem(snapshot.problem as i32);
        }
        for (slot, until) in self.rx_until.iter_mut().enumerate() {
            if snapshot.status.received_frames[slot] > self.last_received[slot] {
                *until = now + Duration::from_millis(200);
            }
        }
        self.last_received = snapshot.status.received_frames;
        let active = std::array::from_fn(|slot| ready && self.rx_until[slot] > now);
        if active != self.rx_active {
            self.rx_active = active;
            self.refresh_rows(ui);
        }
        ui.set_rx_active(active.iter().any(|rx| *rx));
        ui.set_ready(ready);
        ui.set_editing(self.pending.is_some() || snapshot.editing);
        if ready
            && self.pending.is_none()
            && !snapshot.editing
            && self.transfer == 0
            && ui.get_background_enabled()
        {
            if let Some(sample) = background.as_ref() {
                self.ptt.background(sample, now);
            } else {
                self.ptt.invalidate_background();
            }
        } else {
            self.ptt.invalidate_background();
        }
        ui.set_armed(self.ptt.armed());
        ui.set_phase(if self.stopping && !finished {
            5
        } else {
            match snapshot.status.phase {
                Phase::Connecting => 1,
                Phase::Receiving => 2,
                Phase::Transmitting => 3,
                Phase::Stopped => 4,
            }
        });
        ui.set_sent(snapshot.status.sent_frames.to_string().into());
        ui.set_received(snapshot.status.mixed_frames.to_string().into());
        self.publish(ui);
        if finished {
            self.invalidate(ui);
            self.handle.take(); // Finished thread: join cannot wait on network or devices.
            self.pending = None;
            ui.set_busy(false);
            ui.set_ready(false);
            ui.set_editing(false);
            ui.set_phase(4);
            if !snapshot.finished {
                ui.set_problem(Problem::Runtime as i32);
            }
        }
    }
}

pub fn run() -> Result<(), slint::PlatformError> {
    let (ui, app, timer) = setup(Worker::default_path())?;
    let result = ui.run();
    timer.stop();
    // Explicitly drop/join even while the window/callback objects still exist.
    app.borrow_mut().diagnostic.take();
    app.borrow_mut().background.take();
    app.borrow_mut().handle.take();
    app.borrow_mut().storage.take();
    app.borrow_mut().discovery.take();
    result
}
type Application = (MainWindow, Rc<RefCell<App>>, Timer);
fn setup(profile_path: Option<std::path::PathBuf>) -> Result<Application, slint::PlatformError> {
    let selector = slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into());
    #[cfg(target_os = "linux")]
    let selector = selector.with_winit_window_attributes_hook(|attributes| {
        use slint::winit_030::winit::platform::{
            wayland::WindowAttributesExtWayland, x11::WindowAttributesExtX11,
        };
        let attributes = WindowAttributesExtWayland::with_name(
            attributes,
            srs_input::APPLICATION_ID,
            "srs-desktop",
        );
        WindowAttributesExtX11::with_name(attributes, srs_input::APPLICATION_ID, "srs-desktop")
    });
    #[cfg(all(test, target_os = "linux"))]
    let selector = {
        use slint::winit_030::winit::platform::x11::EventLoopBuilderExtX11;
        let mut builder = slint::winit_030::winit::event_loop::EventLoop::<
            slint::winit_030::SlintEvent,
        >::with_user_event();
        builder.with_any_thread(true);
        selector.with_winit_event_loop_builder(builder)
    };
    selector.select()?;
    let ui = MainWindow::new()?;
    let inputs = Vec::new();
    let outputs = Vec::new();
    let discovery = devices::Worker::start().ok();
    let default_label = ui.global::<Strings>().get_system_default();
    ui.set_inputs(device_model(&inputs, default_label.clone()));
    ui.set_outputs(device_model(&outputs, default_label));
    let radios = disabled_radios();
    ui.set_radios(rows(&radios, &DEFAULT_MONITOR, &[false; RADIO_COUNT]));
    let storage = Worker::start(profile_path).ok();
    ui.set_storage_busy(storage.is_some());
    ui.set_storage_status(if storage.is_some() { 1 } else { 2 });
    let app = Rc::new(RefCell::new(App {
        handle: None,
        diagnostic: None,
        ptt: UiInput::new(Instant::now()),
        background: srs_input::Worker::spawn().ok(),
        background_revision: 0,
        joysticks: vec![],
        wanted_joystick: String::new(),
        radios,
        inputs,
        outputs,
        discovery,
        discovering: false,
        wanted_input: "default".into(),
        wanted_output: "default".into(),
        missing_input: None,
        missing_output: None,
        ptt_key: 8,
        modifiers: false,
        details: srs_toc::observation::SessionDetails::default(),
        pending: None,
        pending_selection: None,
        pending_monitor: None,
        monitor: DEFAULT_MONITOR,
        storage_busy: storage.is_some(),
        load_preferences: true,
        transfer: 0,
        storage,
        profiles: Data::default(),
        next_edit: 0,
        stopping: false,
        last_problem: Problem::None,
        last_received: [0; RADIO_COUNT],
        restore_monitor: false,
        rx_until: [Instant::now(); RADIO_COUNT],
        rx_active: [false; RADIO_COUNT],
    }));
    app.borrow_mut().refresh_devices(&ui);
    ui.set_app_version(env!("CARGO_PKG_VERSION").into());
    // Weak UI references prevent callback cycles. try_borrow_mut handles Slint's
    // nested pointer cancellation when a state update disables the PTT pad;
    // all disabling paths invalidate ownership before changing UI properties.
    macro_rules! callback {
        ($name:ident, |$state:ident, $window:ident $(, $arg:ident)*| $body:block) => {{
            let state = app.clone(); let weak = ui.as_weak();
            ui.$name(move |$($arg),*| {
                if let Some($window) = weak.upgrade() && let Ok(mut $state) = state.try_borrow_mut() { $body }
            });
        }};
    }
    callback!(on_navigate, |state, window, page| {
        if (0..=6).contains(&page) {
            state.invalidate(&window);
            window.set_page(page);
        }
    });
    callback!(on_refresh_devices, |state, window| {
        state.refresh_devices(&window);
    });
    callback!(on_rebind, |state, window, key| {
        state.rebind(&window, key);
    });
    callback!(on_background_action, |state, window, action| {
        state.background_action(&window, action);
    });
    callback!(on_diagnostic_action, |state, window, action| {
        state.diagnostic_action(&window, action);
    });
    callback!(on_connect_requested, |state, window| {
        state.connect(&window);
    });
    callback!(on_disconnect_requested, |state, window| {
        state.disconnect(&window);
    });
    callback!(on_select_radio, |state, window, slot| {
        if (1..=10).contains(&slot) {
            state.invalidate(&window);
            window.set_selected(slot);
            state.editor(&window);
        }
    });
    callback!(on_edit_routing, |state, window, routing| {
        state.edit_routing(&window, routing);
    });
    callback!(on_edit_monitor, |state, window, volume, muted| {
        state.edit_monitor(&window, volume, muted);
    });
    callback!(on_edit_radio, |state, window, kind| {
        state.edit(&window, kind);
    });
    callback!(on_profile_action, |state, window, action| {
        state.profile_action(&window, action);
    });
    callback!(on_profile_selected, |state, window, kind| {
        state.invalidate(&window);
        if kind == 0 {
            if let Ok(index) = usize::try_from(window.get_server_index())
                && let Some(server) = state.profiles.servers.get(index)
            {
                window.set_server_name(server.name.as_str().into());
            }
        } else if let Ok(index) = usize::try_from(window.get_preset_index())
            && let Some(preset) = state.profiles.presets.get(index)
        {
            window.set_preset_name(preset.name.as_str().into());
        }
    });
    callback!(on_focused_input, |state, window, kind| {
        state.event(
            &window,
            2,
            match kind {
                0 => Input::Press,
                1 => Input::Release,
                _ => Input::Invalidate,
            },
        );
    });
    callback!(on_pointer_input, |state, window, kind| {
        state.event(
            &window,
            1,
            match kind {
                0 => Input::Press,
                1 => Input::Release,
                _ => Input::Invalidate,
            },
        );
    });
    let state = app.clone();
    let weak = ui.as_weak();
    ui.window().on_winit_window_event(move |_, event| {
        if let Some(window) = weak.upgrade()
            && let Ok(mut state) = state.try_borrow_mut()
        {
            match event {
                WindowEvent::Focused(focused) => {
                    state.ptt.focus(*focused, Instant::now());
                    state.publish(&window);
                }
                WindowEvent::ModifiersChanged(modifiers) => {
                    state.modifiers = !modifiers.state().is_empty();
                    if state.modifiers {
                        state.ptt.invalidate_foreground(Instant::now());
                        state.publish(&window);
                    }
                }
                WindowEvent::Occluded(true) => {
                    state.ptt.invalidate_foreground(Instant::now());
                    state.publish(&window);
                }
                WindowEvent::KeyboardInput {
                    event,
                    is_synthetic,
                    ..
                } if keyboard::matches(&event.logical_key, state.ptt_key) => {
                    if window.get_background_enabled() && window.get_background_mode() == 1 {
                        return EventResult::Propagate;
                    }
                    if state.modifiers {
                        state.invalidate(&window);
                        return EventResult::Propagate;
                    }
                    let input = match (event.state, event.repeat, is_synthetic) {
                        (ElementState::Released, _, _) => Input::Release,
                        (_, true, _) | (_, _, true) => Input::Repeat,
                        _ => Input::Press,
                    };
                    state.event(&window, 0, input);
                    return EventResult::PreventDefault;
                }
                _ => (),
            }
        }
        EventResult::Propagate
    });
    let state = app.clone();
    let weak = ui.as_weak();
    ui.window().on_close_requested(move || {
        if let Some(window) = weak.upgrade()
            && let Ok(mut state) = state.try_borrow_mut()
        {
            state.stop_diagnostic();
            state.stop_background(&window);
            state.disconnect(&window);
            if state.transfer != 0 {
                window.set_storage_status(9);
                return slint::CloseRequestResponse::KeepWindowShown;
            }
        }
        slint::CloseRequestResponse::HideWindow
    });
    let timer = Timer::default();
    let state = app.clone();
    let weak = ui.as_weak();
    timer.start(TimerMode::Repeated, Duration::from_millis(50), move || {
        if let Some(window) = weak.upgrade()
            && let Ok(mut state) = state.try_borrow_mut()
        {
            state.tick(&window);
        }
    });
    Ok((ui, app, timer))
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use slint::Model;
    struct Peer(Option<std::thread::JoinHandle<()>>);
    impl Drop for Peer {
        fn drop(&mut self) {
            if let Some(thread) = self.0.take() {
                let _ = thread.join();
            }
        }
    }
    fn generated_peer(address: std::net::SocketAddr) -> Peer {
        Peer(Some(std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                tokio::time::timeout(Duration::from_secs(5), async {
                    let id = srs_protocol::voice::ClientId::from_bytes(b"GGGGGGGGGGGGGGGGGGGGGG")
                        .unwrap();
                    let mut session = srs_client::Session::connect(
                        address,
                        srs_client::Identity::new(id, "Generated GUI receive test"),
                        srs_client::Options::default(),
                    )
                    .await
                    .unwrap();
                    session.authenticate("fixture-blue").await.unwrap();
                    let mut radios = disabled_radios();
                    radios[1] = Radio::tuned(251_125_000.0, Modulation::AM);
                    session.set_radios(radios).await.unwrap();
                    let mut voice = session.open_voice().await.unwrap();
                    voice.press_ptt(1).unwrap();
                    let mut encoder = srs_audio::Encoder::new().unwrap();
                    let mut encoded = [0; srs_audio::MAX_OPUS_BYTES];
                    for frame in 0..12 {
                        let pcm = std::array::from_fn(|index| {
                            let sample = frame * srs_audio::INPUT_SAMPLES + index;
                            (8000.0
                                * (sample as f32 * std::f32::consts::TAU * 440.0 / 16000.0).sin())
                                as i16
                        });
                        let count = encoder.encode(&pcm, &mut encoded).unwrap();
                        voice.send_frame(&encoded[..count]).await.unwrap();
                        tokio::time::sleep(Duration::from_millis(40)).await;
                    }
                    voice.release_ptt();
                    // Retain the owned TCP session throughout the short burst.
                    session.close();
                })
                .await
                .unwrap();
            });
        })))
    }

    #[test]
    #[ignore = "requires test_stock_server.py private server, Xvfb and process-local null PCM"]
    fn desktop_session_smoke() {
        let address =
            std::env::var("SRS_DESKTOP_TEST_SERVER").expect("private test server required");
        assert!(
            address
                .parse::<std::net::SocketAddr>()
                .unwrap()
                .ip()
                .is_loopback()
        );
        let alsa = std::fs::read_to_string(std::env::var("ALSA_CONFIG_PATH").unwrap()).unwrap();
        assert_eq!(alsa, "pcm.!default { type null }\n");
        let profile_dir = tempfile::tempdir().unwrap();
        let (ui, app, heartbeat) = setup(Some(profile_dir.path().join("profiles.json"))).unwrap();
        ui.set_demonstration(true);
        let peer_address: std::net::SocketAddr = address.parse().unwrap();
        let relay = crate::test_relay::Relay::new(peer_address);
        let relay_port = relay.address.port();
        ui.set_server(format!("localhost:{relay_port}").into());
        ui.set_retry(true);
        ui.set_callsign("Synthetic desktop".into());
        ui.set_password("fixture-blue".into());
        assert!(!ui.get_ready());
        let step = Rc::new(RefCell::new(-1));
        let complete = Rc::new(RefCell::new(false));
        let done = complete.clone();
        let weak = ui.as_weak();
        let state = app.clone();
        let began = Instant::now();
        let mut at = Instant::now();
        let mut sent_at_stall = 0;
        let mut saw_long_tx = false;
        let mut peer: Option<Peer> = None;
        let mut saw_rx = false;
        let mut saw_radio_rx = false;
        let driver = Timer::default();
        driver.start(TimerMode::Repeated, Duration::from_millis(50), move || {
            let ui = weak.upgrade().unwrap();
            assert!(
                began.elapsed() < Duration::from_secs(25),
                "desktop test timed out at step {}: GUI phase {}, held {:?}, worker {:?}",
                step.borrow(),
                ui.get_phase(),
                state.borrow().ptt.intent(1, Instant::now()).radio,
                state
                    .borrow()
                    .handle
                    .as_ref()
                    .map(|h| h.view.borrow().status)
            );
            assert_eq!(ui.get_problem(), 0, "desktop session reported an error");
            let mut step = step.borrow_mut();
            if *step == 9 && ui.get_phase() == 3 {
                saw_long_tx = true;
            }
            if ui.get_radios().row_data(0).is_some_and(|row| row.rx) {
                saw_radio_rx = true;
            }
            if ui.get_rx_active() {
                saw_rx = true;
            }
            match *step {
                -1 if !ui.get_device_busy() => {
                    ui.invoke_connect_requested();
                    assert!(ui.get_password().is_empty());
                    *step = 0;
                }
                0 if ui.get_ready()
                    && ui.window().with_winit_window(|window| window.has_focus()) == Some(true) =>
                {
                    assert!(
                        state
                            .borrow()
                            .radios
                            .iter()
                            .all(|r| r.modulation == Modulation::DISABLED)
                    );
                    assert_eq!(ui.get_server_version(), "2.4.1.0");
                    assert_eq!(ui.get_coalition(), 2);
                    assert_eq!(ui.get_policy().row_count(), 15);
                    assert_eq!(ui.get_policy().row_data(7).unwrap().state, 1);
                    for page in 0..=6 {
                        ui.invoke_navigate(page);
                        assert_eq!(ui.get_page(), page);
                    }
                    ui.invoke_navigate(0);
                    ui.set_frequency("251.125".into());
                    ui.invoke_edit_radio(0);
                    *step = 1;
                }
                1 if !ui.get_editing() => {
                    assert_eq!(
                        ui.window().with_winit_window(|window| window.has_focus()),
                        Some(true),
                        "private test window must own focus"
                    );
                    assert_eq!(state.borrow().radios[1].frequency_hz, 251_125_000.0);
                    peer = Some(generated_peer(peer_address));
                    *step = 12;
                }
                12 if peer
                    .as_ref()
                    .is_some_and(|peer| peer.0.as_ref().unwrap().is_finished()) =>
                {
                    peer.as_mut().unwrap().0.take().unwrap().join().unwrap();
                    peer = None;
                    assert!(
                        ui.get_received().parse::<u64>().unwrap() > 0,
                        "generated stock-server audio must reach native playout"
                    );
                    assert!(saw_rx, "received audio must appear in the UI");
                    assert!(saw_radio_rx, "chosen receive radio must show RX");
                    assert_eq!(state.borrow().last_received[2], 0);
                    ui.invoke_pointer_input(1);
                    ui.invoke_pointer_input(0);
                    assert!(
                        state.borrow().ptt.intent(1, Instant::now()).radio.is_some(),
                        "fresh pointer must own PTT after calibration"
                    );
                    *step = 2;
                }
                2 if ui.get_phase() == 3 && ui.get_sent().parse::<u64>().unwrap() > 0 => {
                    // The null PCM is unpaced; require a sent frame, not a real-time
                    // frame rate. Freeze only the UI thread: the native/session thread must
                    // release without receiving a GUI timer, key-up or quit.
                    std::thread::sleep(Duration::from_millis(400));
                    let snapshot = state
                        .borrow()
                        .handle
                        .as_ref()
                        .unwrap()
                        .view
                        .borrow()
                        .clone();
                    assert_eq!(snapshot.status.phase, Phase::Receiving);
                    sent_at_stall = snapshot.status.sent_frames;
                    // Simulate queued input after the stall; it cannot restart TX.
                    ui.invoke_pointer_input(1);
                    ui.invoke_pointer_input(0);
                    at = Instant::now();
                    *step = 3;
                }
                3 if at.elapsed() >= Duration::from_millis(400) => {
                    assert_eq!(ui.get_phase(), 2);
                    assert_eq!(ui.get_sent().parse::<u64>().unwrap(), sent_at_stall);
                    ui.invoke_pointer_input(1);
                    ui.invoke_pointer_input(0);
                    *step = 4;
                }
                4 if ui.get_phase() == 3 => {
                    ui.invoke_pointer_input(2);
                    at = Instant::now();
                    *step = 5;
                }
                5 if at.elapsed() >= Duration::from_millis(100) => {
                    assert_eq!(ui.get_phase(), 2);
                    ui.invoke_select_radio(2);
                    ui.set_frequency("253.125".into());
                    ui.set_modulation(1);
                    ui.invoke_edit_radio(0);
                    *step = 6;
                }
                6 if !ui.get_editing() => {
                    assert_eq!(state.borrow().radios[2].modulation, Modulation::FM);
                    ui.set_guard("243".into());
                    ui.invoke_edit_radio(1);
                    *step = 7;
                }
                7 if !ui.get_editing() => {
                    assert_eq!(state.borrow().radios[2].guard_hz, 243e6);
                    ui.set_key("7".into());
                    ui.invoke_edit_radio(2);
                    *step = 8;
                }
                8 if !ui.get_editing() => {
                    assert_eq!(state.borrow().radios[2].encryption_key, Some(7));
                    ui.invoke_pointer_input(1);
                    ui.invoke_pointer_input(0);
                    at = Instant::now();
                    *step = 9;
                }
                9 if at.elapsed() >= Duration::from_millis(5300) => {
                    assert!(saw_long_tx, "long hold must first enter TX");
                    assert_eq!(ui.get_phase(), 2, "hold cap must release TX");
                    ui.invoke_edit_radio(3);
                    *step = 10;
                }
                10 if !ui.get_editing() => {
                    assert_eq!(state.borrow().radios[2].modulation, Modulation::DISABLED);
                    ui.invoke_disconnect_requested();
                    *step = 11;
                }
                11 if !ui.get_busy() => {
                    assert!(!ui.get_ready());
                    assert!(state.borrow().handle.is_none());
                    ui.set_server_name("Local training".into());
                    ui.invoke_profile_action(0);
                    *step = 13;
                }
                13 if !ui.get_storage_busy() => {
                    assert_eq!(ui.get_storage_status(), 0);
                    ui.set_server("127.0.0.1:1".into());
                    ui.set_callsign("changed".into());
                    ui.set_password("must clear on load".into());
                    ui.set_server_index(0);
                    ui.invoke_profile_action(1);
                    assert_eq!(ui.get_server(), format!("localhost:{relay_port}"));
                    assert_eq!(ui.get_callsign(), "Synthetic desktop");
                    assert!(ui.get_password().is_empty());
                    // Prepare an entire preset offline, without opening audio.
                    ui.set_frequency("30.5".into());
                    ui.set_modulation(1);
                    ui.invoke_edit_radio(0);
                    ui.set_radio_name("Ground net".into());
                    ui.invoke_edit_radio(4);
                    ui.set_guard("243".into());
                    ui.invoke_edit_radio(1);
                    ui.set_key("7".into());
                    ui.invoke_edit_radio(2);
                    ui.invoke_edit_monitor(35, true);
                    ui.invoke_edit_routing(2);
                    ui.set_preset_name("TOC training".into());
                    ui.invoke_profile_action(3);
                    *step = 14;
                }
                14 if !ui.get_storage_busy() => {
                    assert_eq!(ui.get_storage_status(), 0);
                    ui.invoke_edit_radio(3);
                    ui.invoke_edit_monitor(100, false);
                    ui.invoke_profile_action(6);
                    *step = 15;
                }
                15 if !ui.get_storage_busy() => {
                    assert_eq!(ui.get_storage_status(), 0);
                    ui.set_preset_index(0);
                    ui.invoke_profile_action(4);
                    assert_eq!(state.borrow().radios[2].frequency_hz, 30.5e6);
                    assert_eq!(state.borrow().radios[2].name, "Ground net");
                    assert_eq!(state.borrow().radios[2].guard_hz, 243e6);
                    assert_eq!(state.borrow().radios[2].encryption_key, Some(7));
                    assert!(!ui.get_busy());
                    assert_eq!(ui.get_volume(), 35.);
                    assert!(ui.get_muted());
                    assert_eq!(
                        state.borrow().monitor[2],
                        RadioLevel::new(35, true)
                            .unwrap()
                            .with_routing(Routing::Right)
                    );
                    ui.set_password("fixture-blue".into());
                    ui.invoke_connect_requested();
                    *step = 16;
                }
                16 if ui.get_ready() => {
                    assert_eq!(
                        state.borrow().radios[2].frequency_hz,
                        30.5e6,
                        "offline preset lost during handshake"
                    );
                    assert_eq!(ui.get_volume(), 35.);
                    assert!(ui.get_muted());
                    ui.invoke_edit_monitor(80, false);
                    ui.set_frequency("31.5".into());
                    ui.invoke_edit_radio(0);
                    *step = 17;
                }
                17 if !ui.get_editing() && ui.get_ready() => {
                    ui.invoke_pointer_input(1);
                    at = Instant::now();
                    *step = 171;
                }
                171 if ui.get_armed() && !ui.get_editing() => {
                    ui.invoke_pointer_input(0);
                    *step = 18;
                }
                171 if at.elapsed() >= Duration::from_millis(200) => {
                    // A scheduling gap can legitimately discard calibration
                    // during the quiet-input interval. Send a fresh release
                    // after that interval; never retry or resume a held press.
                    ui.invoke_pointer_input(1);
                    at = Instant::now();
                }
                18 if ui.get_phase() == 3 => {
                    ui.invoke_profile_action(4);
                    assert!(state.borrow().ptt.intent(2, Instant::now()).radio.is_none());
                    *step = 19;
                }
                19 if !ui.get_editing() && ui.get_phase() == 2 => {
                    assert_eq!(state.borrow().radios[2].frequency_hz, 30.5e6);
                    assert_eq!(ui.get_volume(), 35.);
                    assert!(ui.get_muted());
                    ui.invoke_disconnect_requested();
                    *step = 20;
                }
                20 if !ui.get_busy() => {
                    ui.invoke_rebind(12);
                    assert_eq!(ui.get_ptt_key(), 12);
                    ui.invoke_profile_action(7);
                    *step = 21;
                }
                21 if !ui.get_storage_busy() => {
                    assert_eq!(ui.get_storage_status(), 0);
                    ui.invoke_rebind(0);
                    ui.invoke_profile_action(6);
                    *step = 22;
                }
                22 if !ui.get_storage_busy() => {
                    assert_eq!(ui.get_ptt_key(), 12);
                    ui.invoke_refresh_devices();
                    assert!(ui.get_device_busy());
                    *step = 23;
                }
                23 if !ui.get_device_busy() => {
                    assert_eq!(ui.get_input_index(), 0);
                    assert_eq!(ui.get_output_index(), 0);
                    ui.set_password("fixture-blue".into());
                    ui.invoke_connect_requested();
                    *step = 24;
                }
                24 if ui.get_ready() => {
                    ui.invoke_pointer_input(1);
                    ui.invoke_pointer_input(0);
                    *step = 25;
                }
                25 if ui.get_phase() == 3 => {
                    ui.invoke_navigate(3);
                    assert!(state.borrow().ptt.intent(2, Instant::now()).radio.is_none());
                    *step = 26;
                }
                26 if ui.get_phase() == 2 => {
                    ui.invoke_pointer_input(1);
                    ui.invoke_pointer_input(0);
                    *step = 27;
                }
                27 if ui.get_phase() == 3 => {
                    ui.invoke_rebind(0);
                    assert!(state.borrow().ptt.intent(2, Instant::now()).radio.is_none());
                    ui.invoke_disconnect_requested();
                    *step = 28;
                }
                28 if !ui.get_busy() => {
                    ui.set_password("fixture-blue".into());
                    ui.invoke_connect_requested();
                    *step = 29;
                }
                29 if ui.get_ready() => {
                    ui.invoke_pointer_input(1);
                    ui.invoke_pointer_input(0);
                    *step = 30;
                }
                30 if ui.get_phase() == 3 => {
                    relay.drop_connection();
                    *step = 31;
                }
                31 if ui.get_phase() == 1 && ui.get_retry_attempt() == 1 => {
                    assert!(!ui.get_ready());
                    assert!(state.borrow().ptt.intent(2, Instant::now()).radio.is_none());
                    *step = 32;
                }
                32 if ui.get_ready() => {
                    assert_eq!(
                        state
                            .borrow()
                            .handle
                            .as_ref()
                            .unwrap()
                            .view
                            .borrow()
                            .monitor[2],
                        RadioLevel::new(35, true)
                            .unwrap()
                            .with_routing(Routing::Right),
                        "stale GUI watch overwrote restored preset levels"
                    );
                    assert_eq!(state.borrow().radios[2].frequency_hz, 30.5e6);
                    assert_eq!(ui.get_volume(), 35.);
                    assert!(ui.get_muted());
                    at = Instant::now();
                    *step = 33;
                }
                33 if at.elapsed() >= Duration::from_millis(250) => {
                    assert_eq!(ui.get_sent(), "0", "recovery resumed a held transmission");
                    assert_eq!(ui.get_phase(), 2);
                    ui.invoke_pointer_input(1);
                    ui.invoke_pointer_input(0);
                    *step = 34;
                }
                34 if ui.get_phase() == 3 => {
                    ui.invoke_edit_routing(1);
                    assert_eq!(ui.get_routing(), 1);
                    assert!(state.borrow().ptt.intent(2, Instant::now()).radio.is_none());
                    ui.invoke_focus_talk();
                    assert!(ui.get_talk_focused());
                    ui.window()
                        .dispatch_event(slint::platform::WindowEvent::KeyReleased {
                            text: " ".into(),
                        });
                    ui.window()
                        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                            text: " ".into(),
                        });
                    *step = 35;
                }
                35 if ui.get_phase() == 3 => {
                    ui.window()
                        .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                            text: slint::platform::Key::Tab.into(),
                        });
                    assert!(!ui.get_talk_focused());
                    assert!(state.borrow().ptt.intent(2, Instant::now()).radio.is_none());
                    ui.invoke_disconnect_requested();
                    *step = 36;
                }
                36 if !ui.get_busy() => {
                    ui.invoke_navigate(6);
                    ui.invoke_diagnostic_action(1);
                    assert!(ui.get_diagnostic_active());
                    ui.invoke_connect_requested();
                    assert!(!ui.get_busy(), "diagnostics must exclude a session");
                    *step = 37;
                }
                37 if ui.get_diagnostic_state() == 1 => {
                    ui.invoke_diagnostic_action(0);
                    *step = 38;
                }
                38 if !ui.get_diagnostic_active() => {
                    assert_eq!(ui.get_diagnostic_state(), 3);
                    *done.borrow_mut() = true;
                    slint::quit_event_loop().unwrap();
                }
                _ => (),
            }
        });
        ui.run().unwrap();
        driver.stop();
        heartbeat.stop();
        app.borrow_mut().diagnostic.take();
        app.borrow_mut().background.take();
        app.borrow_mut().handle.take();
        app.borrow_mut().storage.take();
        app.borrow_mut().discovery.take();
        let saved = std::fs::read_to_string(profile_dir.path().join("profiles.json")).unwrap();
        assert!(
            saved.contains("Ground net")
                && !saved.contains("fixture-blue")
                && !saved.contains("password")
        );
        assert!(
            *complete.borrow(),
            "window closed before validation completed"
        );
    }
}

#[cfg(test)]
mod device_tests {
    use super::*;
    #[test]
    fn saved_devices_follow_identity_and_missing_devices_never_fall_back() {
        let devices = vec![
            DeviceInfo {
                id: "second".into(),
                name: "Same display name".into(),
            },
            DeviceInfo {
                id: "first".into(),
                name: "Same display name".into(),
            },
        ];
        assert_eq!(device_index(&devices, "first"), Some(2));
        assert_eq!(device_id(&devices, 2).unwrap(), "first");
        assert_eq!(device_index(&devices, "unplugged"), None);
        assert_eq!(device_id(&devices, 3), Err(Problem::Device));
        assert_eq!(device_id(&devices, -1), Err(Problem::Device));
        assert_eq!(device_id(&devices, 0).unwrap(), "default");
    }
}
