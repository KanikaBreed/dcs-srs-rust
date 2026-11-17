use std::{net::SocketAddr, time::Duration};

use srs_protocol::control::{ControlDecoder, ControlMessage};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::watch,
    time::timeout,
};

use crate::Error;

pub(crate) struct Transport {
    socket: TcpStream,
    decoder: ControlDecoder,
    buffer: [u8; 8192],
    start: usize,
    end: usize,
    write_timeout: Duration,
    alive: watch::Sender<bool>,
}

impl Transport {
    pub async fn connect(address: SocketAddr, deadline: Duration) -> Result<Self, Error> {
        let socket = timeout(deadline, TcpStream::connect(address))
            .await
            .map_err(|_| Error::Timeout)??;
        socket.set_nodelay(true)?;
        Ok(Self {
            socket,
            decoder: ControlDecoder::default(),
            buffer: [0; 8192],
            start: 0,
            end: 0,
            write_timeout: deadline,
            alive: watch::channel(true).0,
        })
    }

    pub fn liveness(&self) -> watch::Receiver<bool> {
        self.alive.subscribe()
    }

    pub async fn send(&mut self, message: &ControlMessage) -> Result<(), Error> {
        let bytes = message.encode()?;
        timeout(self.write_timeout, self.socket.write_all(&bytes))
            .await
            .map_err(|_| Error::Timeout)??;
        Ok(())
    }

    /// Cancellation-safe for heartbeat selection: decoder and unread coalesced
    /// bytes stay here; Tokio read does not consume bytes on cancellation.
    pub async fn receive(&mut self) -> Result<ControlMessage, Error> {
        loop {
            if self.start < self.end {
                let mut input = &self.buffer[self.start..self.end];
                let message = self.decoder.decode(&mut input)?;
                self.start = self.end - input.len();
                if let Some(message) = message {
                    return Ok(message);
                }
            }
            // Keep the consumed range intact while read is pending. Resetting
            // start before the await would replay the old buffer if a radio
            // edit or refresh cancelled this read before new bytes arrived.
            let count = self.socket.read(&mut self.buffer).await?;
            self.start = 0;
            self.end = count;
            if self.end == 0 {
                std::mem::take(&mut self.decoder).finish()?;
                return Err(Error::Disconnected);
            }
        }
    }
}

impl Drop for Transport {
    fn drop(&mut self) {
        self.alive.send_replace(false);
    }
}
