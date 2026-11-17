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
