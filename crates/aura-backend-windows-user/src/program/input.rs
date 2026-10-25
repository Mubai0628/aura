//! A bounded writer keeps a runtime that refuses stdin from blocking cancel.
use anyhow::{Result, ensure};
use aura_windows_platform::NativeProcessStdin;
use std::{
    io::Write,
    sync::mpsc,
    thread::JoinHandle,
    time::{Duration, Instant},
};

pub(super) struct Input {
    sender: Option<mpsc::SyncSender<Vec<u8>>>,
    worker: JoinHandle<bool>,
    sent: usize,
}
impl Input {
    pub(super) fn open(mut pipe: NativeProcessStdin) -> Result<Self> {
        let (sender, receiver) = mpsc::sync_channel::<Vec<u8>>(2);
        let worker =
            std::thread::Builder::new().name("aura-package-stdin".into()).spawn(move || {
                for bytes in receiver {
                    if pipe.write_all(&bytes).is_err() {
                        return false;
                    }
                }
                true
            })?;
        Ok(Self { sender: Some(sender), worker, sent: 0 })
    }
    pub(super) fn send(&mut self, bytes: Vec<u8>) -> Result<()> {
        self.sent = self
            .sent
            .checked_add(bytes.len())
            .ok_or_else(|| anyhow::anyhow!("package.stdin_limit"))?;
        ensure!(self.sent <= 2 * 1024 * 1024, "package.stdin_limit");
        self.sender
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("package.stdin_closed"))?
            .try_send(bytes)
            .map_err(|_| anyhow::anyhow!("package.stdin_backpressure"))
    }
    /// Ends the input: the writer finishes what was sent and closes the pipe, so the
    /// program reads to its end of file.
    pub(super) fn close(&mut self) {
        self.sender.take();
    }
    /// Called after the Job has been stopped. No unbounded thread join may hold
    /// up shutdown; failure to observe the writer's end is reported as unknown.
    pub(super) fn finish(mut self) -> (bool, bool) {
        self.sender.take();
        let deadline = Instant::now() + Duration::from_secs(1);
        while !self.worker.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        if self.worker.is_finished() {
            (true, self.worker.join().unwrap_or(false))
        } else {
            (false, false)
        }
    }
}
