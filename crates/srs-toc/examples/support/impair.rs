//! Private probe relay: finite queues, deterministic UDP loss/jitter/duplicates.
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream, UdpSocket},
    task::JoinHandle,
    time::Instant,
};
#[derive(Default)]
pub struct Stats {
    pub lost: AtomicU64,
    pub duplicated: AtomicU64,
    pub overflow: AtomicU64,
}
pub struct Relay {
    pub address: SocketAddr,
    pub stats: Arc<Stats>,
    control: JoinHandle<()>,
    voice: JoinHandle<()>,
}
impl Relay {
    pub async fn start(server: SocketAddr) -> Result<Self, std::io::Error> {
        assert!(server.ip().is_loopback());
        let tcp = TcpListener::bind("127.0.0.1:0").await?;
        let address = tcp.local_addr()?;
        let udp = UdpSocket::bind(address).await?;
        let stats = Arc::new(Stats::default());
        let counts = stats.clone();
        let control = tokio::spawn(async move {
            let (mut local, _) = tcp.accept().await.expect("private relay accept");
            let mut remote = TcpStream::connect(server)
                .await
                .expect("private relay server");
            let _ = tokio::io::copy_bidirectional(&mut local, &mut remote).await;
        });
        let voice = tokio::spawn(async move {
            let mut client = None;
            let mut bytes = [0; 8192];
            let mut serial = 0u64;
            let mut pending: Vec<(Instant, Vec<u8>, SocketAddr)> = Vec::with_capacity(128);
            let mut timer = tokio::time::interval(Duration::from_millis(5));
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    result = udp.recv_from(&mut bytes) => {
                        let (n, from) = result.expect("private relay receive");
                        let destination = if from == server { client } else {
                            assert!(from.ip().is_loopback()); client = Some(from); Some(server)
                        };
                        let Some(destination) = destination else { continue };
                        // Identity keepalives are not impaired; only voice datagrams.
                        if n <= 22 { udp.send_to(&bytes[..n], destination).await.expect("keepalive"); continue; }
                        serial += 1;
                        if serial.is_multiple_of(17) { counts.lost.fetch_add(1, Ordering::Relaxed); continue; }
                        let copies = if serial.is_multiple_of(29) { counts.duplicated.fetch_add(1, Ordering::Relaxed); 2 } else { 1 };
                        for copy in 0..copies {
                            if pending.len() == 128 { counts.overflow.fetch_add(1, Ordering::Relaxed); continue; }
                            let delay = 10 + [0, 20, 60][serial as usize % 3] + copy * 5;
                            pending.push((Instant::now() + Duration::from_millis(delay), bytes[..n].to_vec(), destination));
                        }
                    }
                    _ = timer.tick() => {
                        let mut i = 0;
                        while i < pending.len() {
                            if pending[i].0 <= Instant::now() {
                                let (due, bytes, destination) = pending.swap_remove(i);
                                if Instant::now().duration_since(due) < Duration::from_millis(200) {
                                    udp.send_to(&bytes, destination).await.expect("private relay forward");
                                } else { counts.overflow.fetch_add(1, Ordering::Relaxed); }
                            } else { i += 1; }
                        }
                    }
                }
            }
        });
        Ok(Self {
            address,
            stats,
            control,
            voice,
        })
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.control.abort();
        self.voice.abort();
    }
}
