#![forbid(unsafe_code)]
//! Endpoint syntax is checked on the UI thread; DNS runs only in the worker.
use crate::session::Problem;
use std::net::SocketAddr;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    host: String,
    port: u16,
    literal: Option<SocketAddr>,
}
impl Endpoint {
    pub fn parse(value: &str) -> Result<Self, Problem> {
        let value = value.trim();
        if let Ok(address) = value.parse::<SocketAddr>() {
            if !permitted(address) {
                return Err(Problem::Address);
            }
            return Ok(Self {
                host: address.ip().to_string(),
                port: address.port(),
                literal: Some(address),
            });
        }
        let (host, port) = value.rsplit_once(':').ok_or(Problem::Address)?;
        let port = port
            .parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or(Problem::Address)?;
        if host.is_empty()
            || host.len() > 253
            || !host.is_ascii()
            || host.parse::<std::net::IpAddr>().is_ok()
            || host
                .strip_suffix('.')
                .unwrap_or(host)
                .split('.')
                .any(|label| {
                    label.is_empty()
                        || label.len() > 63
                        || label.starts_with('-')
                        || label.ends_with('-')
                        || !label
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                })
        {
            return Err(Problem::Address);
        }
        Ok(Self {
            host: host.to_ascii_lowercase(),
            port,
            literal: None,
        })
    }
    pub async fn resolve(&self) -> Result<Vec<SocketAddr>, Problem> {
        if let Some(address) = self.literal {
            return Ok(vec![address]);
        }
        let addresses = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            tokio::net::lookup_host((self.host.as_str(), self.port)),
        )
        .await
        .map_err(|_| Problem::Resolve)?
        .map_err(|_| Problem::Resolve)?;
        let mut result = Vec::new();
        for address in addresses.filter(|a| permitted(*a)).take(16) {
            if !result.contains(&address) {
                result.push(address);
            }
            if result.len() == 4 {
                break;
            }
        }
        if result.is_empty() {
            return Err(Problem::Resolve);
        }
        Ok(result)
    }
}
fn permitted(address: SocketAddr) -> bool {
    address.port() != 0 && !address.ip().is_unspecified() && !address.ip().is_multicast()
}
impl std::fmt::Display for Endpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(address) = self.literal {
            address.fmt(f)
        } else {
            write!(f, "{}:{}", self.host, self.port)
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoints_require_explicit_port_and_bounded_dns_labels() {
        for value in [
            "localhost:5002",
            "srs.example.org:5002",
            "127.0.0.1:5002",
            "[::1]:5002",
        ] {
            assert!(Endpoint::parse(value).is_ok(), "{value}");
        }
        for value in [
            "https://srs.test:5002",
            "srs.test",
            "srs.test:0",
            "0.0.0.0:5002",
            "[::]:5002",
            "224.0.0.1:5002",
            "name:99999",
            "bad name:5",
            "a..b:5",
            "-bad.test:5",
            "::1:5002",
            "bad/:5",
        ] {
            assert!(Endpoint::parse(value).is_err(), "{value}");
        }
        assert!(Endpoint::parse(&format!("{}.test:5002", "x".repeat(64))).is_err());
    }
    #[tokio::test]
    async fn localhost_resolution_is_bounded_without_public_network_access() {
        let addresses = Endpoint::parse("localhost:5002")
            .unwrap()
            .resolve()
            .await
            .unwrap();
        assert!(
            addresses.len() <= 4
                && addresses
                    .iter()
                    .all(|a| a.ip().is_loopback() && a.port() == 5002)
        );
    }
}
