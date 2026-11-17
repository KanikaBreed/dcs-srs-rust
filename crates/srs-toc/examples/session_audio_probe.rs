//! Synthetic callback bridges through stock SRS. No hardware. Invoked only by
//! scripts/test_stock_server.py inside its private network namespace.
use srs_client::{Identity, Options, Radio, Session, disabled_radios};
use srs_device::bridge::{capture_bridge, playback_bridge};
use srs_protocol::voice::{ClientId, Modulation};
use srs_toc::{Phase, Status};
use std::{error::Error, net::SocketAddr, time::Duration};
use tokio::{
    sync::watch,
    time::{Instant, sleep_until, timeout},
};
async fn connect(address: SocketAddr, id: u8) -> Result<Session, Box<dyn Error>> {
    let mut session = Session::connect(
        address,
        Identity::new(
            ClientId::from_bytes(&[id; 22])?,
            "Synthetic session audio probe",
        ),
        Options::default(),
    )
    .await?;
    session.authenticate("fixture-blue").await?;
    let mut radios = disabled_radios();
    radios[1] = Radio::tuned(251e6, Modulation::AM);
    session.set_radios(radios).await?;
    Ok(session)
}
async fn phase(status: &mut watch::Receiver<Status>, phase: Phase) -> Result<(), Box<dyn Error>> {
    timeout(Duration::from_secs(3), async {
        loop {
            if status.borrow_and_update().phase == phase {
                return Ok::<_, watch::error::RecvError>(());
            }
            status.changed().await?;
        }
    })
    .await??;
    Ok(())
}
async fn exercise(address: SocketAddr, id: u8) -> Result<(), Box<dyn Error>> {
    let sender = connect(address, id).await?;
    let receiver = connect(address, id + 1).await?;
    let now = Instant::now().into_std();
    let (mut input, mut capture, input_signal) = capture_bridge(1, now)?;
    let (mut writer, mut reverse_output, reverse_output_signal) = playback_bridge(2)?;
    let (mut reverse_input, mut receive_input, reverse_input_signal) = capture_bridge(1, now)?;
    let (mut receive_output, mut output, output_signal) = playback_bridge(2)?;
    let (mut owner, commands) = srs_toc::controls();
    let (mut receive_owner, receive_commands) = srs_toc::controls();
    let mut levels = srs_client::receive::DEFAULT_MONITOR;
    levels[1] = levels[1].with_routing(srs_client::receive::Routing::Left);
    owner.set_monitor(levels);
    levels[1] = levels[1].with_routing(srs_client::receive::Routing::Right);
    receive_owner.set_monitor(levels);
    let (status, mut tx_status) = srs_toc::status_channel();
    let (receive_status, mut rx_status) = srs_toc::status_channel();
    let tx = tokio::spawn(async move {
        srs_toc::run(sender, &mut capture, &mut writer, commands, status).await
    });
    let rx = tokio::spawn(async move {
        srs_toc::run(
            receiver,
            &mut receive_input,
            &mut receive_output,
            receive_commands,
            receive_status,
        )
        .await
    });
    // Abort tasks on any early validation error, including timeout.
    struct Tasks(tokio::task::AbortHandle, tokio::task::AbortHandle);
    impl Drop for Tasks {
        fn drop(&mut self) {
            self.0.abort();
            self.1.abort();
        }
    }
    let _tasks = Tasks(tx.abort_handle(), rx.abort_handle());
    phase(&mut tx_status, Phase::Receiving).await?;
    phase(&mut rx_status, Phase::Receiving).await?;
    burst(
        &mut owner,
        &mut input,
        &mut output,
        &mut tx_status,
        &mut rx_status,
        "forward",
        1,
    )
    .await?;
    burst(
        &mut receive_owner,
        &mut reverse_input,
        &mut reverse_output,
        &mut rx_status,
        &mut tx_status,
        "reverse",
        0,
    )
    .await?;
    drop(owner);
    drop(receive_owner);
    let sent = timeout(Duration::from_secs(3), tx).await???;
    let received = timeout(Duration::from_secs(3), rx).await???;
    if sent.sent_frames != 25
        || received.sent_frames != 25
        || sent.playback_overflows != 0
        || received.playback_overflows != 0
        || [
            input_signal,
            output_signal,
            reverse_input_signal,
            reverse_output_signal,
        ]
        .iter()
        .any(|s| s.statistics().dropped != 0 || !s.is_stopped())
    {
        return Err("session audio queue drop, frame count or teardown check failed".into());
    }
    println!(
        "PASS: same two sessions exchanged generated capture in both directions; isolated left/right reception, zero queue drops and all bridges stopped."
    );
    Ok(())
}
async fn burst(
    owner: &mut srs_toc::Controller,
    input: &mut srs_device::bridge::CaptureCallback,
    output: &mut srs_device::bridge::PlaybackCallback,
    sender_status: &mut watch::Receiver<Status>,
    receiver_status: &mut watch::Receiver<Status>,
    label: &str,
    receive_channel: usize,
) -> Result<(), Box<dyn Error>> {
    owner.press(1)?;
    phase(sender_status, Phase::Transmitting).await?;
    let began = Instant::now();
    let mut peak_worker_us = 0;
    let mut first_audio_ms = None;
    let mut audible_callbacks = 0;
    let before_sent = sender_status.borrow().sent_frames;
    let before_mixed = receiver_status.borrow().mixed_frames;
    for tick in 0..125 {
        sleep_until(began + Duration::from_millis(tick * 10)).await;
        let now = Instant::now();
        if tick < 100 {
            owner.renew()?;
            let pcm = std::array::from_fn::<_, 480, _>(|i| {
                0.3 * (std::f32::consts::TAU * 700.0 * (tick * 480 + i as u64) as f32 / 48000.0)
                    .sin()
            });
            input.process(&pcm, now.into_std());
        } else if tick == 100 {
            owner.release()?;
        }
        let start = Instant::now();
        let mut played = [0f32; 960];
        output.process(&mut played, now.into_std());
        if played
            .as_chunks::<2>()
            .0
            .iter()
            .any(|pair| pair[1 - receive_channel] != 0.0)
        {
            return Err("radio audio leaked into the unselected stereo channel".into());
        }
        peak_worker_us = peak_worker_us.max(start.elapsed().as_micros());
        if played.iter().any(|n| n.abs() > 0.02) {
            first_audio_ms.get_or_insert(began.elapsed().as_millis());
            audible_callbacks += 1;
        }
    }
    let sent = sender_status.borrow().sent_frames - before_sent;
    let mixed = receiver_status.borrow().mixed_frames - before_mixed;
    if sent != 25 || mixed < 25 || audible_callbacks < 80 {
        return Err(format!(
            "session audio counts differ: sent={sent} mixed={mixed} audible={audible_callbacks}"
        )
        .into());
    }
    println!(
        "{{\"direction\":\"{label}\",\"receive_channel\":{receive_channel},\"sent_frames\":{sent},\"mixed_frames_including_plc\":{mixed},\"audible_callbacks\":{audible_callbacks},\"first_capture_to_audible_ms\":{},\"max_playback_bridge_us\":{peak_worker_us}}}",
        first_audio_ms.ok_or("no audio")?
    );
    Ok(())
}
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let address: SocketAddr = std::env::args()
        .nth(1)
        .ok_or("usage: session_audio_probe loopback-ip:port")?
        .parse()?;
    if !address.ip().is_loopback() {
        return Err("synthetic probe requires loopback".into());
    }
    exercise(address, b'K').await?;
    Ok(())
}
