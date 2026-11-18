//! Synthetic UI inspection only: opens no audio devices or network sockets.
#![forbid(unsafe_code)]
use slint::{ComponentHandle, Timer, TimerMode, VecModel};
use srs_desktop::ui::{MainWindow, PeerRow, PolicyRow, RadioRow};
use std::{error::Error, io::Write, rc::Rc, time::Duration};
fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("provide an output .ppm path")?;
    let tx = std::env::args().any(|arg| arg == "--tx");
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .renderer_name("software".into())
        .select()?;
    let ui = MainWindow::new()?;
    ui.set_demonstration(true);
    if std::env::args().any(|arg| arg == "--compact") {
        ui.window().set_size(slint::LogicalSize::new(900., 800.));
    }
    ui.set_page(0);
    ui.set_app_version(env!("CARGO_PKG_VERSION").into());
    for arg in std::env::args() {
        if let Some(page) = arg.strip_prefix("--page=").and_then(|p| p.parse().ok()) {
            ui.set_page(page);
        }
    }
    if std::env::args().any(|a| a == "--global-ptt") {
        ui.set_background_mode(1);
        ui.set_background_enabled(true);
        ui.set_background_state(2);
        ui.set_background_trigger("Ctrl+F8".into());
        ui.set_background_ctrl(true);
    }
    if std::env::args().any(|a| a == "--joystick-ptt") {
        ui.set_background_mode(2);
        ui.set_joysticks(Rc::new(VecModel::from(vec!["Preview flight stick".into()])).into());
        ui.set_joystick_index(0);
        ui.set_joystick_button(2);
    }
    if std::env::args().any(|a| a == "--audio-test") {
        ui.set_diagnostic_active(true);
        ui.set_diagnostic_state(1);
        ui.set_diagnostic_mode(1);
        ui.set_diagnostic_peak(42.);
        ui.set_diagnostic_seconds(5);
    }
    ui.set_callsign("TOC DEMO".into());
    ui.set_server_version("2.4.1.0".into());
    ui.set_coalition(2);
    ui.set_client_count("3".into());
    ui.set_node_limit("3".into());
    ui.set_peers(
        Rc::new(VecModel::from(vec![
            PeerRow {
                name: "TOC DEMO".into(),
                coalition: 2,
            },
            PeerRow {
                name: "Tower".into(),
                coalition: 2,
            },
            PeerRow {
                name: "Flight lead".into(),
                coalition: 2,
            },
        ]))
        .into(),
    );
    ui.set_policy(
        Rc::new(VecModel::from(
            (0..15)
                .map(|index| PolicyRow {
                    index,
                    state: if index == 7 { 1 } else { 2 },
                })
                .collect::<Vec<_>>(),
        ))
        .into(),
    );
    ui.set_inputs(Rc::new(VecModel::from(vec!["Preview microphone".into()])).into());
    ui.set_outputs(Rc::new(VecModel::from(vec!["Preview headphones".into()])).into());
    ui.set_radios(
        Rc::new(VecModel::from(
            (0..10)
                .map(|index| RadioRow {
                    routing: if index == 1 {
                        1
                    } else if index == 2 {
                        2
                    } else {
                        0
                    },
                    volume: if index == 1 { 60 } else { 100 },
                    muted: index == 2,
                    rx: index == 1 && !tx,
                    name: match index {
                        0 => "Command",
                        1 => "Tower",
                        2 => "Ground",
                        _ => "",
                    }
                    .into(),
                    frequency: match index {
                        0 => "251.000000",
                        1 => "124.000000",
                        _ => "30.500000",
                    }
                    .into(),
                    mode: if index < 2 { "AM" } else { "FM" }.into(),
                    enabled: index < 3,
                    ..RadioRow::default()
                })
                .collect::<Vec<_>>(),
        ))
        .into(),
    );
    ui.set_saved_servers(Rc::new(VecModel::from(vec!["Training server".into()])).into());
    ui.set_server_index(0);
    ui.set_server_name("Training server".into());
    ui.set_saved_presets(Rc::new(VecModel::from(vec!["TOC standard".into()])).into());
    ui.set_preset_index(0);
    ui.set_preset_name("TOC standard".into());
    ui.set_radio_name("Command".into());
    if std::env::args().any(|arg| arg == "--profiles") {
        ui.set_page(2);
    }
    ui.set_busy(true);
    ui.set_ready(true);
    ui.set_armed(true);
    ui.set_phase(if tx { 3 } else { 2 });
    ui.set_rx_active(!tx);
    ui.set_sent("125".into());
    ui.set_received("250".into());
    if std::env::args().any(|a| a == "--audio-test") {
        ui.set_busy(false);
        ui.set_ready(false);
        ui.set_phase(0);
        ui.set_rx_active(false);
        ui.set_sent("0".into());
        ui.set_received("0".into());
    }
    if std::env::args().any(|a| a == "--offline") {
        ui.set_busy(false);
        ui.set_ready(false);
        ui.set_armed(false);
        ui.set_phase(0);
        ui.set_rx_active(false);
        ui.set_sent("0".into());
        ui.set_received("0".into());
    }
    let weak = ui.as_weak();
    let timer = Timer::default();
    timer.start(
        TimerMode::SingleShot,
        Duration::from_millis(300),
        move || {
            let result = (|| -> Result<(), Box<dyn Error>> {
                let ui = weak.upgrade().ok_or("preview closed")?;
                let pixels = ui.window().take_snapshot()?;
                let mut file = std::io::BufWriter::new(std::fs::File::create(&path)?);
                write!(file, "P6\n{} {}\n255\n", pixels.width(), pixels.height())?;
                for pixel in pixels.as_slice() {
                    file.write_all(&[pixel.r, pixel.g, pixel.b])?;
                }
                file.flush()?;
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!("Preview failed: {error}");
                std::process::exit(1);
            }
            let _ = slint::quit_event_loop();
        },
    );
    ui.run()?;
    Ok(())
}
