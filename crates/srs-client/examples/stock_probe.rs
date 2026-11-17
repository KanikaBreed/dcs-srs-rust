//! Automated conformance probe; synthetic credentials and loopback only. The
//! Python runner launches the unmodified release in a private network namespace.
use serde_json::Value;
use srs_client::{Coalition, Error, Identity, Options, Radio, Session, State, disabled_radios};
use srs_protocol::{
    control::{ControlMessage, MessageType},
    voice::{ClientId, Modulation},
};
use std::{error::Error as StdError, net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    time::timeout,
};

const BLUE_ID: &str = "AAAAAAAAAAAAAAAAAAAAAA";
const RED_ID: &str = "_____________________w";

fn identity(id: &str) -> Result<Identity, Box<dyn StdError>> {
    Ok(Identity::new(
        ClientId::from_bytes(id.as_bytes())?,
        "Synthetic Rust TOC",
    ))
}
fn require(condition: bool, label: &str) -> Result<(), Box<dyn StdError>> {
    if condition {
        Ok(())
    } else {
        Err(std::io::Error::other(label).into())
    }
}
async fn observe(
    client: &mut Session,
    id: &str,
    predicate: impl Fn(&Value) -> bool,
) -> Result<(), Box<dyn StdError>> {
    timeout(Duration::from_secs(5), async {
        loop {
            if let Some(peer) = client.clients().get(id)
                && predicate(&Value::Object(peer.clone()))
            {
                return Ok::<_, Error>(());
            }
            client.next_event().await?;
        }
    })
    .await??;
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn StdError>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: stock_probe <loopback-address:port> <enabled|disabled>".into());
    }
    let address: SocketAddr = args[1].parse()?;
    require(address.ip().is_loopback(), "probe requires loopback")?;
    let mut blue = Session::connect(address, identity(BLUE_ID)?, Options::default()).await?;
    require(
        blue.server_version() == Some("2.4.1.0"),
        "unexpected stock server version",
    )?;
    if args[2] == "disabled" {
        require(
            blue.authenticate("fixture-blue").await == Err(Error::EamDisabled),
            "disabled EAM was offered",
        )?;
        // Also send a direct request to confirm the stock server rejects it,
        // independently of our client's local policy check.
        let mut wire = ControlMessage::new(MessageType::EXTERNAL_AWACS_MODE_PASSWORD, "2.4.1.0");
        wire.client = blue.clients().get(BLUE_ID).cloned();
        wire.external_awacs_mode_password = Some("fixture-blue".into());
        blue.close();
        // Use a distinct identity so disconnect processing cannot remove it.
        wire.client
            .as_mut()
            .ok_or("missing own SYNC state")?
            .insert("ClientGuid".into(), Value::String(RED_ID.into()));
        let mut peer = BufReader::new(TcpStream::connect(address).await?);
        peer.get_mut().write_all(&wire.encode()?).await?;
        let reply = timeout(Duration::from_secs(5), async {
            loop {
                let mut line = String::new();
                if peer.read_line(&mut line).await? == 0 {
                    return Err(Error::Disconnected);
                }
                let reply = ControlMessage::decode_json(line.as_bytes())?;
                if reply.message_type == MessageType::EXTERNAL_AWACS_MODE_PASSWORD {
                    return Ok::<_, Error>(reply);
                }
            }
        })
        .await??;
        require(
            reply.message_type == MessageType::EXTERNAL_AWACS_MODE_PASSWORD
                && reply.client.as_ref().and_then(|c| c.get("Coalition")) == Some(&Value::from(0)),
            "stock disabled-mode response was not a rejection",
        )?;
        println!(
            "PASS: stock 2.4.1.0 disabled EAM; client sends no password and direct server request returns coalition 0"
        );
        return Ok(());
    }
    require(args[2] == "enabled", "unknown scenario")?;
    require(
        blue.authenticate("fixture-wrong").await == Err(Error::AuthenticationRejected),
        "wrong password accepted",
    )?;
    require(
        blue.state() == State::Connected,
        "rejected login authorized a coalition",
    )?;
    require(
        blue.authenticate("fixture-blue").await? == Coalition::Blue,
        "blue password assigned wrong coalition",
    )?;
    let mut red = Session::connect(address, identity(RED_ID)?, Options::default()).await?;
    require(
        red.authenticate("fixture-red").await? == Coalition::Red,
        "red password assigned wrong coalition",
    )?;
    let mut radios = disabled_radios();
    radios[1] = Radio::tuned(251_000_000.0, Modulation::AM);
    radios[1].guard_hz = 243_000_000.0;
    radios[1].encryption_key = Some(7);
    radios[2] = Radio::tuned(30_000_000.0, Modulation::FM);
    blue.set_radios(radios).await?;
    observe(&mut red, BLUE_ID, |c| {
        c["Coalition"] == 2
            && c["RadioInfo"]["radios"][1]["freq"] == 251_000_000.0
            && c["RadioInfo"]["radios"][1]["secFreq"] == 243_000_000.0
            && c["RadioInfo"]["radios"][1]["encKey"] == 7
            && c["RadioInfo"]["radios"][2]["modulation"] == 1
    })
    .await?;
    let mut radios = disabled_radios();
    radios[1] = Radio::tuned(305_000_000.0, Modulation::AM);
    red.set_radios(radios).await?;
    observe(&mut blue, RED_ID, |c| {
        c["Coalition"] == 1 && c["RadioInfo"]["radios"][1]["freq"] == 305_000_000.0
    })
    .await?;
    blue.leave_eam().await?;
    observe(&mut red, BLUE_ID, |c| {
        c["Coalition"] == 0 && c["RadioInfo"]["radios"][1]["modulation"] == 3
    })
    .await?;
    blue.close();
    timeout(Duration::from_secs(5), async {
        while red.clients().contains_key(BLUE_ID) {
            red.next_event().await?;
        }
        Ok::<_, Error>(())
    })
    .await??;
    red.close();
    let old = Session::connect(
        address,
        identity("BBBBBBBBBBBBBBBBBBBBBB")?,
        Options {
            compatibility_version: "1.0.0.0".into(),
            ..Options::default()
        },
    )
    .await;
    require(
        matches!(old, Err(Error::IncompatibleVersion)),
        "stock server did not reject old protocol version",
    )?;
    println!(
        "PASS: stock 2.4.1.0 SYNC/version, wrong password, Blue/Red login, two-way radio state, guard/key/modulation, EAM leave/reset, peer disconnect, old-version rejection"
    );
    Ok(())
}
