#![forbid(unsafe_code)]
//! Metadata discovery only. Device streams remain owned by the session worker.
use srs_device::native::{self, DeviceInfo, Direction};
use std::{
    sync::mpsc::{self, Receiver, SyncSender},
    thread::JoinHandle,
};
pub struct Listing {
    pub inputs: Vec<DeviceInfo>,
    pub outputs: Vec<DeviceInfo>,
}
pub struct Worker {
    request: Option<SyncSender<()>>,
    result: Option<Receiver<Result<Listing, ()>>>,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    pub fn start() -> Result<Self, ()> {
        let (request, commands) = mpsc::sync_channel(1);
        let (results, result) = mpsc::sync_channel(1);
        let thread = std::thread::Builder::new()
            .name("srs-devices".into())
            .spawn(move || {
                while commands.recv().is_ok() {
                    let listing = (|| {
                        Ok(Listing {
                            inputs: native::devices(Direction::Input).map_err(|_| ())?,
                            outputs: native::devices(Direction::Output).map_err(|_| ())?,
                        })
                    })();
                    if results.send(listing).is_err() {
                        break;
                    }
                }
            })
            .map_err(|_| ())?;
        Ok(Self {
            request: Some(request),
            result: Some(result),
            thread: Some(thread),
        })
    }
    pub fn refresh(&self) -> Result<(), ()> {
        self.request
            .as_ref()
            .ok_or(())?
            .try_send(())
            .map_err(|_| ())
    }
    pub fn poll(&self) -> Option<Result<Listing, ()>> {
        match self.result.as_ref()?.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err(())),
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.result.take();
        self.request.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
