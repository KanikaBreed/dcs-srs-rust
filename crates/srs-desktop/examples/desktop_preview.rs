//! Synthetic UI inspection only: opens no audio devices or network sockets.
#![forbid(unsafe_code)]
use slint::{ComponentHandle, Timer, TimerMode, VecModel};
use srs_desktop::ui::{MainWindow, RadioRow};
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
    ui.set_callsign("TOC DEMO".into());
    ui.set_inputs(Rc::new(VecModel::from(vec!["Preview microphone".into()])).into());
    ui.set_outputs(Rc::new(VecModel::from(vec!["Preview headphones".into()])).into());
    ui.set_radios(
        Rc::new(VecModel::from(
            (0..10)
                .map(|index| RadioRow {
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
        ui.set_profiles_open(true);
        ui.window().set_size(slint::LogicalSize::new(1080., 1060.));
    }
    ui.set_busy(true);
    ui.set_ready(true);
    ui.set_armed(true);
    ui.set_phase(if tx { 3 } else { 2 });
    ui.set_rx_active(!tx);
    ui.set_sent("125".into());
    ui.set_received("250".into());
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
