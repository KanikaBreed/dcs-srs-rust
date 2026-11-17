//! Offline generated-tone exchange with the unchanged upstream C# wrappers.
use srs_audio::*;
use std::{error::Error, fs, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: codec_probe write|verify fixture-directory".into());
    }
    let root = Path::new(&args[2]);
    if args[1] == "write" {
        fs::create_dir_all(root)?;
        let mut encoder = Encoder::new()?;
        let mut output = [0; MAX_OPUS_BYTES];
        for frame in 0..20 {
            let pcm = std::array::from_fn(|i| {
                (12_000.0
                    * (std::f64::consts::TAU * 700.0 * (frame * INPUT_SAMPLES + i) as f64
                        / INPUT_RATE as f64)
                        .sin()) as i16
            });
            let len = encoder.encode(&pcm, &mut output)?;
            fs::write(root.join(format!("rust-{frame:02}.opus")), &output[..len])?;
        }
        println!("Generated 20 Rust Opus frames with {}", codec_version());
    } else if args[1] == "verify" {
        let mut decoder = Decoder::new()?;
        let mut output = [0; OUTPUT_SAMPLES];
        let mut minimum_fraction = 1.0_f64;
        for frame in 0..20 {
            let packet = fs::read(root.join(format!("csharp-{frame:02}.opus")))?;
            decoder.decode(&packet, &mut output)?;
            if frame > 0 {
                let energy: f64 = output.iter().map(|x| (*x as f64).powi(2)).sum();
                let sin: f64 = output
                    .iter()
                    .enumerate()
                    .map(|(i, x)| {
                        *x as f64
                            * (std::f64::consts::TAU * 700.0 * i as f64 / OUTPUT_RATE as f64).sin()
                    })
                    .sum();
                let cos: f64 = output
                    .iter()
                    .enumerate()
                    .map(|(i, x)| {
                        *x as f64
                            * (std::f64::consts::TAU * 700.0 * i as f64 / OUTPUT_RATE as f64).cos()
                    })
                    .sum();
                let fraction = 2.0 * (sin * sin + cos * cos) / (OUTPUT_SAMPLES as f64 * energy);
                if energy / (OUTPUT_SAMPLES as f64) < 4_000_000.0
                    || !fraction.is_finite()
                    || fraction < 0.5
                {
                    return Err("C#-encoded tone failed Rust decode/signal validation".into());
                }
                minimum_fraction = minimum_fraction.min(fraction);
            }
        }
        println!(
            "PASS: Rust decoded 20 C# frames at 48 kHz/40 ms; minimum 700 Hz energy fraction {minimum_fraction:.4}"
        );
    } else {
        return Err("unknown codec probe mode".into());
    }
    Ok(())
}
