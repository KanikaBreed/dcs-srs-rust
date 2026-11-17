#![forbid(unsafe_code)]
use crate::{
    input::UiInput,
    session::{Config, Edit, Handle, Problem, frequency},
    ui::{MainWindow, RadioRow, Strings},
};
use slint::winit_030::{
    EventResult, WinitWindowAccessor,
    winit::{
        event::{ElementState, WindowEvent},
        keyboard::{Key, NamedKey},
    },
};
use slint::{ComponentHandle, ModelRc, SharedString, Timer, TimerMode, VecModel};
use srs_client::{RADIO_COUNT, Radio, commands::RadioChange, disabled_radios};
use srs_device::native::{self, DeviceInfo, Direction};
use srs_protocol::voice::Modulation;
use srs_toc::{Phase, held::Input};
use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

struct App {
    handle: Option<Handle>,
    ptt: UiInput,
    radios: [Radio; RADIO_COUNT],
    inputs: Vec<DeviceInfo>,
    outputs: Vec<DeviceInfo>,
    pending: Option<u64>,
    next_edit: u64,
    stopping: bool,
    last_problem: Problem,
    last_mixed: u64,
    rx_until: Instant,
}
fn rows(radios: &[Radio; RADIO_COUNT]) -> ModelRc<RadioRow> {
    Rc::new(VecModel::from(
        radios[1..]
            .iter()
            .map(|radio| RadioRow {
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
fn device_id(devices: &[DeviceInfo], index: i32) -> String {
    usize::try_from(index)
        .ok()
        .and_then(|i| i.checked_sub(1))
        .and_then(|i| devices.get(i))
        .map(|d| d.id.clone())
        .unwrap_or_else(|| "default".into())
}
impl App {
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
            ui.set_modulation(i32::from(radio.modulation == Modulation::FM));
        }
    }
    fn connect(&mut self, ui: &MainWindow) {
        if self.handle.is_some() {
            return;
        }
        self.invalidate(ui);
        let password = ui.get_password().to_string();
        ui.set_password("".into());
        let result = Config::parse(
            &ui.get_server(),
            &ui.get_callsign(),
            password,
            device_id(&self.inputs, ui.get_input_index()),
            device_id(&self.outputs, ui.get_output_index()),
        )
        .and_then(|config| {
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
                self.last_mixed = 0;
                self.radios = disabled_radios();
                ui.set_radios(rows(&self.radios));
                ui.set_problem(0);
                ui.set_busy(true);
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
        if self.pending.is_some() || !ui.get_ready() {
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
        let result = change.and_then(|change| {
            self.next_edit += 1;
            let handle = self.handle.as_ref().ok_or(Problem::Session)?;
            handle
                .edits
                .try_send(Edit {
                    id: self.next_edit,
                    at: Instant::now(),
                    change,
                })
                .map_err(|_| Problem::Backlog)?;
            self.pending = Some(self.next_edit);
            Ok(())
        });
        ui.set_problem(result.err().unwrap_or(Problem::None) as i32);
        ui.set_editing(self.pending.is_some());
    }
    fn tick(&mut self, ui: &MainWindow) {
        let now = Instant::now();
        self.ptt.tick(now);
        let Some(handle) = &self.handle else {
            return;
        };
        let snapshot = handle.view.borrow().clone();
        let finished = handle.finished();
        let ready = !self.stopping
            && matches!(
                snapshot.status.phase,
                Phase::Receiving | Phase::Transmitting
            );
        if !ready || self.pending.is_some() {
            self.invalidate(ui);
        }
        let acknowledged = self.pending.is_some_and(|id| id == snapshot.completed_edit);
        if acknowledged {
            self.pending = None;
            self.invalidate(ui);
            ui.set_problem(snapshot.problem as i32);
        }
        if self.radios != snapshot.radios {
            self.invalidate(ui);
            self.radios = snapshot.radios;
            ui.set_radios(rows(&self.radios));
            self.editor(ui);
        }
        if snapshot.problem != self.last_problem {
            self.last_problem = snapshot.problem;
            ui.set_problem(snapshot.problem as i32);
        }
        if snapshot.status.mixed_frames > self.last_mixed {
            self.rx_until = now + Duration::from_millis(200);
        }
        self.last_mixed = snapshot.status.mixed_frames;
        ui.set_rx_active(ready && self.rx_until > now);
        ui.set_ready(ready);
        ui.set_editing(self.pending.is_some() || snapshot.editing);
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
    let (ui, app, timer) = setup()?;
    let result = ui.run();
    timer.stop();
    // Explicitly drop/join even while the window/callback objects still exist.
    app.borrow_mut().handle.take();
    result
}
type Application = (MainWindow, Rc<RefCell<App>>, Timer);
fn setup() -> Result<Application, slint::PlatformError> {
    let selector = slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into());
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
    let inputs = native::devices(Direction::Input).unwrap_or_default();
    let outputs = native::devices(Direction::Output).unwrap_or_default();
    let default_label = ui.global::<Strings>().get_system_default();
    ui.set_inputs(device_model(&inputs, default_label.clone()));
    ui.set_outputs(device_model(&outputs, default_label));
    let radios = disabled_radios();
    ui.set_radios(rows(&radios));
    let app = Rc::new(RefCell::new(App {
        handle: None,
        ptt: UiInput::new(Instant::now()),
        radios,
        inputs,
        outputs,
        pending: None,
        next_edit: 0,
        stopping: false,
        last_problem: Problem::None,
        last_mixed: 0,
        rx_until: Instant::now(),
    }));
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
    callback!(on_edit_radio, |state, window, kind| {
        state.edit(&window, kind);
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
                WindowEvent::Occluded(true) => state.invalidate(&window),
                WindowEvent::KeyboardInput {
                    event,
                    is_synthetic,
                    ..
                } if event.logical_key == Key::Named(NamedKey::F8) => {
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
            state.disconnect(&window);
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
        let (ui, app, heartbeat) = setup().unwrap();
        ui.set_demonstration(true);
        let peer_address = address.parse().unwrap();
        ui.set_server(address.into());
        ui.set_callsign("Synthetic desktop".into());
        ui.set_password("fixture-blue".into());
        ui.invoke_connect_requested();
        assert!(ui.get_password().is_empty());
        assert!(!ui.get_ready());
        let step = Rc::new(RefCell::new(0));
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
            if ui.get_rx_active() {
                saw_rx = true;
            }
            match *step {
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
                    *done.borrow_mut() = true;
                    slint::quit_event_loop().unwrap();
                }
                _ => (),
            }
        });
        ui.run().unwrap();
        driver.stop();
        heartbeat.stop();
        app.borrow_mut().handle.take();
        assert!(
            *complete.borrow(),
            "window closed before validation completed"
        );
    }
}
