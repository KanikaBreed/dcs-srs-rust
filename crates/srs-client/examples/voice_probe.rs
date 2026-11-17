//! Generated audio only. Invoked by the isolated stock-server runner.
use srs_audio::{Encoder, INPUT_RATE, INPUT_SAMPLES, MAX_OPUS_BYTES, OUTPUT_SAMPLES};
use srs_client::{
    Identity, Options, Radio, Session, disabled_radios,
    receive::{Playout, QueueOutcome},
    voice::{VoiceConnection, VoiceError, VoiceEvent},
};
use srs_protocol::voice::{ClientId, Modulation};
use std::{error::Error, net::SocketAddr, time::Duration};
use tokio::time::{Instant, sleep_until, timeout};

const TX: &str = "CCCCCCCCCCCCCCCCCCCCCC";
const RX: &str = "DDDDDDDDDDDDDDDDDDDDDD";
const RED: &str = "EEEEEEEEEEEEEEEEEEEEEE";
const FRAMES: usize = 25;
fn check(ok: bool, label: &str) -> Result<(), Box<dyn Error>> {
    if ok {
        Ok(())
    } else {
        Err(std::io::Error::other(label).into())
    }
}
async fn session(address: SocketAddr, id: &str, password: &str) -> Result<Session, Box<dyn Error>> {
    let identity = Identity::new(
        ClientId::from_bytes(id.as_bytes())?,
        "Synthetic voice probe",
    );
    let mut session = Session::connect(address, identity, Options::default()).await?;
    session.authenticate(password).await?;
    tune(&mut session, 251_000_000.0, 1.0).await?;
    Ok(session)
}
async fn tune(session: &mut Session, hz: f64, guard: f64) -> Result<(), Box<dyn Error>> {
    let mut radios = disabled_radios();
    radios[1] = Radio::tuned(hz, Modulation::AM);
    radios[1].guard_hz = guard;
    session.set_radios(radios).await?;
    Ok(())
}
async fn observed(
    session: &mut Session,
    id: &str,
    hz: f64,
    guard: f64,
) -> Result<(), Box<dyn Error>> {
    timeout(Duration::from_secs(3), async {
        loop {
            if let Some(peer) = session.clients().get(id)
                && peer["RadioInfo"]["radios"][1]["freq"] == hz
                && peer["RadioInfo"]["radios"][1]["secFreq"] == guard
            {
                return Ok::<_, srs_client::Error>(());
            }
            session.next_event().await?;
        }
    })
    .await??;
    Ok(())
}
async fn next_voice(
    voice: &mut VoiceConnection,
) -> Result<srs_client::voice::ReceivedVoice, VoiceError> {
    loop {
        if let VoiceEvent::Voice(frame) = voice.next_event().await? {
            return Ok(frame);
        }
    }
}
async fn next_playout(
    voice: &mut VoiceConnection,
) -> Result<(Playout, [i16; OUTPUT_SAMPLES], f64), Box<dyn Error>> {
    loop {
        let mut pcm = [0; OUTPUT_SAMPLES];
        let before = Instant::now();
        if let Some(frame) = voice.poll_audio(&mut pcm)? {
            return Ok((frame, pcm, before.elapsed().as_secs_f64() * 1e6));
        }
        let deadline = voice
            .audio_deadline()?
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(3));
        tokio::select! {
            biased;
            _ = sleep_until(deadline) => {},
            event = voice.next_event() => if let VoiceEvent::Voice(packet) = event? {
                let outcome = voice.buffer_voice(&packet)?;
                if !matches!(outcome, QueueOutcome::Queued | QueueOutcome::Resynchronized) {
                    return Err(format!("unexpected voice receive outcome: {outcome:?}").into());
                }
            }
        }
    }
}

async fn burst(
    sender: &mut VoiceConnection,
    receiver: &mut VoiceConnection,
    direction: &str,
) -> Result<(), Box<dyn Error>> {
    let mut encoder = Encoder::new()?;
    sender.press_ptt(1)?;
    let send = async {
        let start = Instant::now();
        let mut times = Vec::with_capacity(FRAMES);
        let mut encode_us = 0.0;
        for frame in 0..FRAMES {
            sleep_until(start + Duration::from_millis(frame as u64 * 40)).await;
            let pcm = std::array::from_fn(|i| {
                (12_000.0
                    * (std::f64::consts::TAU * 700.0 * (frame * INPUT_SAMPLES + i) as f64
                        / INPUT_RATE as f64)
                        .sin()) as i16
            });
            let mut bytes = [0; MAX_OPUS_BYTES];
            let sent_at = Instant::now();
            let len = encoder.encode(&pcm, &mut bytes)?;
            encode_us += sent_at.elapsed().as_secs_f64() * 1e6;
            sender.send_frame(&bytes[..len]).await?;
            times.push(sent_at);
        }
        sender.release_ptt();
        Ok::<_, Box<dyn Error>>((times, encode_us))
    };
    let receive = async {
        let mut times = Vec::with_capacity(FRAMES);
        let mut decode_us = 0.0;
        let mut sequence = None;
        for index in 0..FRAMES {
            let (frame, pcm, worker_us) =
                timeout(Duration::from_secs(3), next_playout(receiver)).await??;
            check(
                frame.kind == srs_audio::jitter::FrameKind::Decoded,
                "unexpected concealment in loopback burst",
            )?;
            if let Some(previous) = sequence {
                check(
                    frame.packet_number == previous + 1,
                    "loss/reordering in loopback voice burst",
                )?;
            }
            sequence = Some(frame.packet_number);
            decode_us += worker_us;
            if index > 0 {
                check(
                    pcm.iter().map(|x| (*x as f64).powi(2)).sum::<f64>() / OUTPUT_SAMPLES as f64
                        > 4_000_000.0,
                    "decoded signal energy missing",
                )?;
            }
            times.push(Instant::now());
        }
        Ok::<_, Box<dyn Error>>((times, decode_us))
    };
    let (sent, received) = tokio::join!(send, receive);
    let (sent, encode_us) = sent?;
    let (received, decode_us) = received?;
    let mut latency: Vec<_> = received
        .iter()
        .zip(&sent)
        .map(|(r, s)| r.duration_since(*s).as_secs_f64() * 1000.0)
        .collect();
    latency.sort_by(f64::total_cmp);
    let spacing_error = received
        .windows(2)
        .map(|w| (w[1].duration_since(w[0]).as_secs_f64() * 1000.0 - 40.0).abs())
        .sum::<f64>()
        / (FRAMES - 1) as f64;
    println!(
        "{{\"direction\":\"{direction}\",\"frames\":{FRAMES},\"missing\":0,\"encode_start_to_playout_p50_ms\":{:.3},\"encode_start_to_playout_p95_ms\":{:.3},\"max_ms\":{:.3},\"mean_abs_spacing_error_ms\":{spacing_error:.3},\"mean_encode_us\":{:.1},\"mean_playout_worker_us\":{:.1}}}",
        latency[FRAMES / 2],
        latency[FRAMES * 95 / 100],
        latency[FRAMES - 1],
        encode_us / FRAMES as f64,
        decode_us / FRAMES as f64
    );
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let address: SocketAddr = std::env::args()
        .nth(1)
        .ok_or("usage: voice_probe loopback-address:port")?
        .parse()?;
    check(address.ip().is_loopback(), "probe requires loopback")?;
    let mut tx = session(address, TX, "fixture-blue").await?;
    let mut rx = session(address, RX, "fixture-blue").await?;
    let red = session(address, RED, "fixture-red").await?;
    observed(&mut tx, RX, 251_000_000.0, 1.0).await?;
    observed(&mut rx, TX, 251_000_000.0, 1.0).await?;
    let mut tx_voice = tx.open_voice().await?;
    let mut rx_voice = rx.open_voice().await?;
    let mut red_voice = red.open_voice().await?;
    burst(&mut tx_voice, &mut rx_voice, "tx-to-rx").await?;
    burst(&mut rx_voice, &mut tx_voice, "rx-to-tx").await?;
    check(
        timeout(Duration::from_millis(150), next_voice(&mut red_voice))
            .await
            .is_err(),
        "opposing coalition received voice",
    )?;
    tune(&mut rx, 305_000_000.0, 1.0).await?;
    observed(&mut tx, RX, 305_000_000.0, 1.0).await?;
    let mut encoder = Encoder::new()?;
    let mut bytes = [0; MAX_OPUS_BYTES];
    let len = encoder.encode(&[0; INPUT_SAMPLES], &mut bytes)?;
    tx_voice.press_ptt(1)?;
    tx_voice.send_frame(&bytes[..len]).await?;
    check(
        timeout(Duration::from_millis(150), next_voice(&mut rx_voice))
            .await
            .is_err(),
        "untuned radio received voice",
    )?;
    tune(&mut rx, 305_000_000.0, 251_000_000.0).await?;
    observed(&mut tx, RX, 305_000_000.0, 251_000_000.0).await?;
    tx_voice.send_frame(&bytes[..len]).await?;
    let (guard, _, _) = timeout(Duration::from_secs(3), next_playout(&mut rx_voice)).await??;
    check(
        guard.secondary,
        "guard packet did not reach secondary playout",
    )?;
    // SRS forwards wrong-key packets: prove local eligibility rejects them.
    for (session, key) in [(&mut tx, 7), (&mut rx, 8)] {
        let mut radios = disabled_radios();
        radios[1] = Radio::tuned(251_000_000.0, Modulation::AM);
        radios[1].encryption_key = Some(key);
        session.set_radios(radios).await?;
    }
    for (session, peer, key) in [(&mut tx, RX, 8), (&mut rx, TX, 7)] {
        timeout(Duration::from_secs(3), async {
            while session
                .clients()
                .get(peer)
                .is_none_or(|p| p["RadioInfo"]["radios"][1]["encKey"] != key)
            {
                session.next_event().await?;
            }
            Ok::<_, srs_client::Error>(())
        })
        .await??;
    }
    tx_voice.press_ptt(1)?;
    tx_voice.send_frame(&bytes[..len]).await?;
    let wrong_key = timeout(Duration::from_secs(3), next_voice(&mut rx_voice)).await??;
    check(
        rx_voice.buffer_voice(&wrong_key)? == QueueOutcome::Ineligible,
        "wrong key reached clear-audio queue",
    )?;
    check(
        rx_voice.audio_deadline()?.is_none(),
        "retune retained guard backlog",
    )?;
    tx.leave_eam().await?;
    check(!tx_voice.is_transmitting(), "EAM leave retained PTT")?;
    check(
        tx_voice.send_frame(&bytes[..len]).await.is_err(),
        "transmission allowed after EAM leave",
    )?;
    rx.close();
    check(!rx_voice.is_ready(), "TCP close retained UDP readiness")?;
    println!(
        "PASS: generated Opus both directions, coalition/frequency isolation, guard playout, wrong-key filtering, PTT release, TCP-to-UDP invalidation; no devices opened"
    );
    Ok(())
}
