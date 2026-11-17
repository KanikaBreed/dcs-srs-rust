//! Test-only TCP/UDP relay. Runs solely in the private stock-server namespace.
#![forbid(unsafe_code)]
use std::{net::SocketAddr, thread::JoinHandle};
use tokio::{
    net::{TcpListener, TcpStream, UdpSocket},
    sync::watch,
};
pub struct Relay {
    pub address: SocketAddr,
    stop: watch::Sender<bool>,
    trip: watch::Sender<u64>,
    thread: Option<JoinHandle<()>>,
}
impl Relay {
    pub fn new(server: SocketAddr) -> Self {
        assert!(server.ip().is_loopback());
        let tcp = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = tcp.local_addr().unwrap();
        tcp.set_nonblocking(true).unwrap();
        let udp = std::net::UdpSocket::bind(address).unwrap();
        udp.set_nonblocking(true).unwrap();
        let (stop, mut stopped) = watch::channel(false);
        let (trip, mut tripped) = watch::channel(0_u64);
        let thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            runtime.block_on(async {
                let tcp = TcpListener::from_std(tcp).unwrap();
                let udp = UdpSocket::from_std(udp).unwrap();
                let control = async {
                    loop {
                        let (mut local, _) = tcp.accept().await.unwrap();
                        let mut remote = TcpStream::connect(server).await.unwrap();
                        tripped.borrow_and_update();
                        tokio::select! {
                            _ = tripped.changed() => (),
                            _ = tokio::io::copy_bidirectional(&mut local, &mut remote) => (),
                        }
                    }
                };
                let voice = async {
                    let mut client = None;
                    let mut bytes = [0_u8; 8192];
                    loop {
                        let (n, from) = udp.recv_from(&mut bytes).await.unwrap();
                        if from == server {
                            if let Some(client) = client {
                                udp.send_to(&bytes[..n], client).await.unwrap();
                            }
                        } else {
                            assert!(from.ip().is_loopback());
                            client = Some(from);
                            udp.send_to(&bytes[..n], server).await.unwrap();
                        }
                    }
                };
                tokio::select! { _ = stopped.changed() => (), _ = control => (), _ = voice => () }
            });
        });
        Self {
            address,
            stop,
            trip,
            thread: Some(thread),
        }
    }
    pub fn drop_connection(&self) {
        self.trip.send_modify(|n| *n += 1);
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.send_replace(true);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}
