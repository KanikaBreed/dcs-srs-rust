//! Export Rust-encoded fixtures for independent verification by the C# harness.
use std::{env, error::Error, fs, path::PathBuf};

use srs_protocol::{control::ControlDecoder, voice::Datagram};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() != 2 {
        return Err("usage: roundtrip_fixtures <fixture-directory> <output-directory>".into());
    }
    let source = PathBuf::from(&args[0]).canonicalize()?;
    let target = PathBuf::from(&args[1]);
    fs::create_dir_all(&target)?;
    let target = target.canonicalize()?;
    if source == target {
        return Err("output must differ from input fixture directory".into());
    }
    let mut count = 0;
    for entry in fs::read_dir(source)? {
        let path = entry?.path();
        let extension = path.extension().and_then(|s| s.to_str());
        if !matches!(extension, Some("bin" | "jsonl" | "json")) {
            continue;
        }
        let bytes = fs::read(&path)?;
        let encoded = match extension {
            Some("bin") => Datagram::decode(&bytes)?.encode()?,
            Some("jsonl") => {
                let mut decoder = ControlDecoder::default();
                let mut input = bytes.as_slice();
                let mut output = Vec::new();
                while !input.is_empty() {
                    if let Some(message) = decoder.decode(&mut input)? {
                        output.extend(message.encode()?);
                    }
                }
                decoder.finish()?;
                output
            }
            _ => bytes, // Enum maps are reference data, not control envelopes.
        };
        let name = path.file_name().ok_or("fixture has no file name")?;
        fs::write(target.join(name), encoded)?;
        count += 1;
    }
    println!("Exported {count} fixture files; no sockets or audio devices were opened.");
    Ok(())
}
