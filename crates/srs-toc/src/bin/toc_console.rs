//! Explicit development console with timed or foreground held-key PTT.
#[path = "../console/keyboard.rs"]
mod keyboard;
use srs_client::commands::RadioChange;
use srs_client::{Identity, Options, Radio, Session, disabled_radios};
use srs_device::native::{Input, Output};
use srs_protocol::voice::{ClientId, Modulation};
use srs_toc::{Controller, Phase, Status};
use std::{
    error::Error,
    io::{self, Read, Write},
    net::SocketAddr,
    time::Duration,
};
use tokio::{
    sync::{mpsc, watch},
    time::{Instant, MissedTickBehavior, interval, sleep_until},
};

const HELP: &str = "Usage:
  toc_console --server <IP:port> --id <unique-22-character-SRS-ID> --name <callsign> --mhz <frequency> --mode <am|fm> --input <device-id|default> --output <device-id|default> --seconds <1..3600> [--ptt <timed|keyboard>]

Requires SRS_EAM_PASSWORD in this process's environment. Never pass it as an argument.
This explicitly opens the selected microphone/output and connects to the selected server.
Timed-mode commands: tx <100..5000 milliseconds>, rx, select <1..10>, tune <slot> <MHz> <am|fm>, guard <slot> <MHz|off>, key <slot> <1..255|off>, off <slot>, quit.
Keyboard mode: tap/release F8 to arm, then hold F8 (maximum 5s); 1-9/0 select, f tunes, m changes modulation, x disables, q quits.
TX is a bounded microphone transmission; rx releases it early. Use headphones.
Keyboard mode requires key-release reporting and remains foreground-only. No global/HID PTT, automatic reconnect, or recording.
Startup/help opens no devices or sockets. Only connect to servers you may use.";
struct Config {
    address: SocketAddr,
    id: ClientId,
    name: String,
    hz: f64,
    mode: Modulation,
    input: String,
    output: String,
    seconds: u64,
    keyboard: bool,
}
fn parse(args: &[String]) -> Result<Config, &'static str> {
    let (pairs, remainder) = args.as_chunks::<2>();
    if !remainder.is_empty() {
        return Err("option needs a value");
    }
    let mut values = std::collections::BTreeMap::new();
    for [key, value] in pairs {
        if ![
            "--server",
            "--id",
            "--name",
            "--mhz",
            "--mode",
            "--input",
            "--output",
            "--seconds",
            "--ptt",
        ]
        .contains(&key.as_str())
            || values.insert(key.as_str(), value.as_str()).is_some()
        {
            return Err("unknown or duplicate option");
        }
    }
    let get = |key| values.get(key).copied().ok_or("missing required option");
    let hz = get("--mhz")?
        .parse::<f64>()
        .map_err(|_| "invalid frequency")?
        * 1e6;
    if !hz.is_finite() || !(10_001.0..=10_000_000_000.0).contains(&hz) {
        return Err("invalid frequency");
    }
    let seconds = get("--seconds")?
        .parse::<u64>()
        .map_err(|_| "invalid duration")?;
    if !(1..=3600).contains(&seconds) {
        return Err("duration must be 1..3600 seconds");
    }
    Ok(Config {
        address: get("--server")?
            .parse()
            .map_err(|_| "server must be an IP address and port")?,
        id: ClientId::from_bytes(get("--id")?.as_bytes()).map_err(|_| "invalid client ID")?,
        name: get("--name")?.to_owned(),
        hz,
        mode: match get("--mode")? {
            "am" => Modulation::AM,
            "fm" => Modulation::FM,
            _ => return Err("mode must be am or fm"),
        },
        input: get("--input")?.to_owned(),
        output: get("--output")?.to_owned(),
        seconds,
        keyboard: match values.get("--ptt").copied().unwrap_or("timed") {
            "timed" => false,
            "keyboard" => true,
            _ => return Err("ptt must be timed or keyboard"),
        },
    })
}
#[derive(Clone, Copy, Debug, PartialEq)]
enum Command {
    Tx(Duration),
    Rx,
    Quit,
    Select(usize),
    Change(RadioChange),
}
fn command(line: &str) -> Option<Command> {
    fn slot(word: &str) -> Option<usize> {
        word.parse().ok().filter(|s| (1..=10).contains(s))
    }
    fn hz(word: &str) -> Option<f64> {
        word.parse::<f64>()
            .ok()
            .map(|n| n * 1e6)
            .filter(|n| n.is_finite() && (10_001.0..=10e9).contains(n))
    }
    let mut words = line.split_whitespace();
    let result = match words.next()? {
        "tx" => {
            let ms = words.next()?.parse::<u64>().ok()?;
            if !(100..=5000).contains(&ms) {
                return None;
            }
            Command::Tx(Duration::from_millis(ms))
        }
        "select" => Command::Select(slot(words.next()?)?),
        "tune" => Command::Change(RadioChange::Tune {
            slot: slot(words.next()?)?,
            hz: hz(words.next()?)?,
            modulation: match words.next()? {
                "am" => Modulation::AM,
                "fm" => Modulation::FM,
                _ => return None,
            },
        }),
        "guard" => Command::Change(RadioChange::Guard {
            slot: slot(words.next()?)?,
            hz: match words.next()? {
                "off" => 1.0,
                value => hz(value)?,
            },
        }),
        "key" => Command::Change(RadioChange::Key {
            slot: slot(words.next()?)?,
            key: match words.next()? {
                "off" => None,
                value => Some(value.parse::<u8>().ok().filter(|v| *v > 0)?),
            },
        }),
        "off" => Command::Change(RadioChange::Disable {
            slot: slot(words.next()?)?,
        }),
        "rx" => Command::Rx,
        "quit" => Command::Quit,
        _ => return None,
    };
    words.next().is_none().then_some(result)
}
fn announce(message: &str) -> io::Result<()> {
    if matches!(crossterm::terminal::is_raw_mode_enabled(), Ok(true)) {
        print!("\r{message}\r\n");
    } else {
        println!("{message}");
    }
    io::stdout().flush()
}
fn show_radio(slot: usize, radio: &Radio) -> io::Result<()> {
    let mode = match radio.modulation {
        Modulation::AM => "AM",
        Modulation::FM => "FM",
        _ => "OFF",
    };
    announce(&format!(
        "RADIO {slot}: {:.6} MHz {mode}; guard {:.6} MHz; key {}",
        radio.frequency_hz / 1e6,
        radio.guard_hz / 1e6,
        radio
            .encryption_key
            .map_or_else(|| "off".to_owned(), |k| k.to_string())
    ))
}
// One blocking stdin reader, never an audio callback. Bounded lines and queue;
// EOF/error/overflow signals immediate owner loss. No queued action is replayed.
fn stdin_commands() -> (mpsc::Receiver<(Instant, Command)>, watch::Receiver<bool>) {
    let (sender, receiver) = mpsc::channel(8);
    let (alive, closed) = watch::channel(true);
    std::thread::spawn(move || {
        let mut input = io::stdin().lock();
        let mut byte = [0];
        let mut line = Vec::with_capacity(128);
        while matches!(input.read(&mut byte), Ok(1)) {
            if byte[0] == b'\n' {
                let parsed = std::str::from_utf8(&line).ok().and_then(command);
                line.clear();
                let Some(command) = parsed else {
                    eprintln!(
                        "Invalid command; see --help for TX, tuning, guard, key and selection commands."
                    );
                    continue;
                };
                if sender.try_send((Instant::now(), command)).is_err() {
                    break;
                }
                if command == Command::Quit {
                    break;
                }
            } else {
                if line.len() == 128 {
                    break;
                }
                line.push(byte[0]);
            }
        }
        drop(alive);
    });
    (receiver, closed)
}
async fn operate(
    mut owner: Controller,
    mut status: watch::Receiver<Status>,
    seconds: u64,
    mut radios: [Radio; srs_client::RADIO_COUNT],
) -> Result<(), Box<dyn Error>> {
    let (mut commands, mut alive) = stdin_commands();
    let mut selected = 1;
    show_radio(selected, &radios[selected])?;
    let end = Instant::now() + Duration::from_secs(seconds);
    let mut ticker = interval(Duration::from_millis(100));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let mut transmit_until = None;
    let mut phase = Phase::Connecting;
    loop {
        tokio::select! {
            biased;
            _ = alive.changed() => return Ok(()),
            _ = sleep_until(end) => return Ok(()),
            _ = sleep_until(transmit_until.unwrap_or(end)), if transmit_until.is_some() => {
                owner.release()?; transmit_until = None;
            },
            changed = status.changed() => {
                if changed.is_err() { return Ok(()); }
                let current = status.borrow_and_update().phase;
                if current != phase {
                    if current != Phase::Transmitting && phase == Phase::Transmitting { transmit_until = None; }
                    phase = current;
                    announce(match phase {
                        Phase::Connecting => "CONNECTING",
                        Phase::Receiving => "RX READY: microphone transmission is released.",
                        Phase::Transmitting => "TX LOCAL: microphone transmission enabled; delivery is not acknowledged.",
                        Phase::Stopped => "STOPPED: devices and PTT released.",
                    })?;
                    if phase == Phase::Stopped { return Ok(()); }
                }
            },
            action = commands.recv() => {
                let Some((issued, action)) = action else { return Ok(()); };
                if Instant::now().duration_since(issued) >= srs_toc::INPUT_LEASE {
                    owner.release()?; transmit_until = None; continue;
                }
                match action {
                    Command::Quit => return Ok(()),
                    Command::Rx => { owner.release()?; transmit_until = None; }
                    Command::Tx(duration) if matches!(phase, Phase::Receiving | Phase::Transmitting) && radios[selected].frequency_hz > 10_000.0 && radios[selected].modulation != Modulation::DISABLED => {
                        owner.press(selected)?; transmit_until = Some(Instant::now() + duration);
                    },
                    Command::Tx(_) => eprintln!("Wait for RX READY and select an enabled radio before transmitting."),
                    Command::Select(slot) => { owner.release()?; transmit_until = None; selected = slot; show_radio(slot, &radios[slot])?; },
                    Command::Change(change) => {
                        transmit_until = None;
                        match owner.change_radio(change).await {
                            Ok(snapshot) => { radios = snapshot; for (slot, radio) in radios.iter().enumerate().skip(1) { show_radio(slot, radio)?; } },
                            Err(srs_toc::Error::Control(srs_client::Error::InvalidRadio | srs_client::Error::PolicyDenied)) => announce("RADIO UPDATE REJECTED: invalid settings or server policy.")?,
                            Err(error) => return Err(error.into()),
                        }
                    },
                }
            },
            _ = ticker.tick() => if let Some(until) = transmit_until
                && (Instant::now() >= until || owner.renew().is_err()) {
                    owner.release()?; transmit_until = None;
            },
        }
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] {
        println!("{HELP}");
        return Ok(());
    }
    let config = parse(&args).map_err(io::Error::other)?;
    let password = std::env::var("SRS_EAM_PASSWORD").map_err(|_| "SRS_EAM_PASSWORD is required")?;
    let _terminal = if config.keyboard {
        Some(keyboard::TerminalGuard::open()?)
    } else {
        None
    };
    announce("CONNECT: opening the selected SRS session. Audio will start after authentication.")?;
    let mut session = Session::connect(
        config.address,
        Identity::new(config.id, config.name),
        Options::default(),
    )
    .await?;
    session.authenticate(&password).await?;
    drop(password);
    let mut radios = disabled_radios();
    radios[1] = Radio::tuned(config.hz, config.mode);
    session.set_radios(radios.clone()).await?;
    announce(
        "AUDIO: opening the selected microphone/output. Starting receive-only. Commands: tx <100..5000>, rx, quit.",
    )?;
    let mut input = Input::open(&config.input)?;
    let mut output = Output::open(&config.output)?;
    input.start()?;
    output.start()?;
    let (owner, inputs) = srs_toc::controls();
    let (status, observed) = srs_toc::status_channel();
    let snapshot = observed.clone();
    // Dropping either future stops its counterpart; run's guard invalidates
    // bridges/PTT on cancellation. Native owners then close their streams.
    let result: Result<(), Box<dyn Error>> = tokio::select! {
        result = srs_toc::run(session, input.reader(), output.writer(), inputs, status) => result.map(|_| ()).map_err(Into::into),
        result = async {
            if config.keyboard { keyboard::operate(owner, observed, config.seconds, radios).await }
            else { operate(owner, observed, config.seconds, radios).await }
        } => result,
    };
    let stats = *snapshot.borrow();
    drop(input);
    drop(output);
    println!(
        "STOPPED sent={} mixed={} peak_speakers={} rejected={} playback_overflows={}",
        stats.sent_frames,
        stats.mixed_frames,
        stats.peak_speakers,
        stats.rejected_packets,
        stats.playback_overflows
    );
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn console_requires_explicit_devices_and_bounds_commands() {
        for line in [
            "tx",
            "tx 0",
            "tx 5001",
            "tx NaN",
            "tx 100 extra",
            "rx extra",
            "press",
        ] {
            assert!(command(line).is_none());
        }
        assert_eq!(
            command("tx 200"),
            Some(Command::Tx(Duration::from_millis(200)))
        );
        assert_eq!(command("rx"), Some(Command::Rx));
        assert!(parse(&[]).is_err());
        let args = "--server 127.0.0.1:5002 --id ZZZZZZZZZZZZZZZZZZZZZZ --name Test --mhz 251 --mode am --input default --output default --seconds 30".split_whitespace().map(str::to_owned).collect::<Vec<_>>();
        assert!(parse(&args).is_ok());
        assert!(parse(&args[..args.len() - 2]).is_err());
    }
}
