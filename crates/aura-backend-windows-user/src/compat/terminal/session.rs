//! A monitor owns native resources, never its TerminalSession owner; Drop can join it.
use super::{
    TerminalEnd, TerminalEndReason, TerminalKind, TerminalOptions, TerminalOutputPage,
    output::OutputBuffer,
};
use crate::compat::redaction::Redactor;
use anyhow::{Result, ensure};
use aura_windows_platform::{
    NativeTerminal, ProcessIdentity, TerminalOutputCallback, TerminalSize, VerifiedFile,
};
use aura_windows_user_contract::ManagedShell;
use std::{
    sync::{Arc, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// A write or interrupt to a terminal that has ended. The Agent terminal's caller classifies the
/// input effect from its own Chinese wording, so this one claims nothing about it.
const ENDED: &str =
    "terminal.ended: the terminal has already ended; open a new terminal (terminal.open).";

struct Lifetime {
    native: Arc<NativeTerminal>,
    end: Mutex<Option<TerminalEnd>>,
    closing: Mutex<()>,
    bindings: Mutex<Option<Vec<VerifiedFile>>>,
    changed: tokio::sync::watch::Sender<()>,
    /// Releases output held back as the possible start of a hidden value.
    flush: Box<dyn Fn() + Send + Sync>,
}
impl Lifetime {
    fn finish(&self, reason: TerminalEndReason) -> Result<TerminalEnd> {
        let _closing = self.closing.lock().map_err(|_| anyhow::anyhow!("terminal.close_lock"))?;
        if let Some(end) =
            self.end.lock().map_err(|_| anyhow::anyhow!("terminal.state_lock"))?.clone()
        {
            return Ok(end);
        }
        let result = self.native.close();
        (self.flush)();
        let end = TerminalEnd {
            reason,
            exit_code: result.as_ref().ok().copied().flatten(),
            stop_confirmed: result.is_ok(),
            error: result.as_ref().err().map(ToString::to_string),
        };
        // A failed stop keeps the bindings until owner teardown; never claim they were released.
        if end.stop_confirmed {
            self.bindings.lock().map_err(|_| anyhow::anyhow!("terminal.binding_lock"))?.take();
        }
        *self.end.lock().map_err(|_| anyhow::anyhow!("terminal.state_lock"))? = Some(end.clone());
        self.changed.send_replace(());
        result?;
        Ok(end)
    }
}
pub struct TerminalSession {
    pub(super) kind: TerminalKind,
    pub(super) shell: ManagedShell,
    lifetime: Arc<Lifetime>,
    output: Arc<Mutex<OutputBuffer>>,
    monitor: Mutex<Option<JoinHandle<()>>>,
}
impl TerminalSession {
    pub(super) fn start(
        options: &TerminalOptions,
        bindings: Vec<VerifiedFile>,
        hidden: Option<Redactor>,
        spawn: impl FnOnce(TerminalOutputCallback) -> Result<NativeTerminal>,
    ) -> Result<Self> {
        let output = Arc::new(Mutex::new(OutputBuffer::new(options.output_limit)?));
        let (changed, _) = tokio::sync::watch::channel(());
        let output_changed = changed.clone();
        let buffer = Arc::clone(&output);
        let hidden = Arc::new(Mutex::new(hidden.filter(|redactor| !redactor.is_empty())));
        let (held, written) = (Arc::clone(&hidden), Arc::clone(&output));
        let flush = Box::new(move || {
            if let (Ok(mut held), Ok(mut written)) = (held.lock(), written.lock())
                && let Some(redactor) = held.as_mut()
            {
                written.append(&redactor.finish());
            }
        });
        let native = Arc::new(spawn(Arc::new(move |bytes| {
            let mut hidden =
                hidden.lock().map_err(|_| std::io::Error::other("terminal.output_lock"))?;
            let shown = match hidden.as_mut() {
                Some(redactor) => std::borrow::Cow::Owned(redactor.push(bytes)),
                None => std::borrow::Cow::Borrowed(bytes),
            };
            buffer
                .lock()
                .map_err(|_| std::io::Error::other("terminal.output_lock"))?
                .append(&shown);
            output_changed.send_replace(());
            Ok(())
        }))?);
        let lifetime = Arc::new(Lifetime {
            native,
            end: Mutex::new(None),
            closing: Mutex::new(()),
            bindings: Mutex::new(Some(bindings)),
            changed,
            flush,
        });
        let watched = Arc::clone(&lifetime);
        let monitor =
            std::thread::Builder::new().name("aura-terminal-monitor".into()).spawn(move || {
                match watched.native.wait_for_exit() {
                    Ok(true) => {
                        let _ = watched.finish(TerminalEndReason::Exited);
                    }
                    Ok(false) => {} // The caller owns the close reason and final drain.
                    Err(_) => {
                        let _ = watched.finish(TerminalEndReason::OutputFailed);
                    }
                }
            });
        match monitor {
            Ok(monitor) => Ok(Self {
                kind: options.kind,
                shell: options.shell,
                lifetime,
                output,
                monitor: Mutex::new(Some(monitor)),
            }),
            Err(error) => {
                let _ = lifetime.finish(TerminalEndReason::OwnerDropped);
                Err(error.into())
            }
        }
    }
    pub fn identity(&self) -> &ProcessIdentity {
        self.lifetime.native.identity()
    }
    pub fn subscribe_output(&self) -> tokio::sync::watch::Receiver<()> {
        self.lifetime.changed.subscribe()
    }
    pub fn process_identities(&self) -> Result<Vec<ProcessIdentity>> {
        Ok(self.lifetime.native.process_identities()?)
    }
    pub fn end(&self) -> Result<Option<TerminalEnd>> {
        Ok(self.lifetime.end.lock().map_err(|_| anyhow::anyhow!("terminal.state_lock"))?.clone())
    }
    pub fn read(&self, offset: u64, max_bytes: usize) -> Result<TerminalOutputPage> {
        self.output
            .lock()
            .map_err(|_| anyhow::anyhow!("terminal.output_lock"))?
            .read(offset, max_bytes)
    }
    /// The offset the next output will start at.
    pub fn output_end(&self) -> Result<u64> {
        Ok(self.output.lock().map_err(|_| anyhow::anyhow!("terminal.output_lock"))?.end())
    }
    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        ensure!(
            bytes.len() <= 65_536,
            "terminal.input_too_large: one input is at most 65536 bytes (this one is {} bytes); nothing was written. Send it in several parts.",
            bytes.len()
        );
        ensure!(self.end()?.is_none(), ENDED);
        self.lifetime.native.write(bytes)?;
        Ok(())
    }
    pub fn interrupt(&self) -> Result<()> {
        ensure!(self.end()?.is_none(), ENDED);
        self.lifetime.native.interrupt()?;
        Ok(())
    }
    pub fn resize(&self, size: TerminalSize) -> Result<bool> {
        Ok(self.lifetime.native.resize(size)?)
    }
    /// The size the terminal has now: the one it opened with, or the last resize.
    pub fn size(&self) -> Result<TerminalSize> {
        Ok(self.lifetime.native.size()?)
    }
    pub fn close(&self, reason: TerminalEndReason) -> Result<TerminalEnd> {
        let result = self.lifetime.finish(reason);
        if let Some(monitor) =
            self.monitor.lock().map_err(|_| anyhow::anyhow!("terminal.monitor_lock"))?.take()
        {
            monitor.join().map_err(|_| anyhow::anyhow!("terminal.monitor_panicked"))?;
        }
        let end = result?;
        ensure!(
            end.stop_confirmed,
            "terminal.stop_unconfirmed: Aura could not confirm that every process the terminal started has stopped; some may still be running. Do not replay its input."
        );
        Ok(end)
    }
    pub fn wait_ended(&self, timeout: Duration) -> Result<TerminalEnd> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(end) = self.end()? {
                return Ok(end);
            }
            ensure!(Instant::now() < deadline, "terminal.wait_timeout");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for TerminalSession {
    fn drop(&mut self) {
        if let Err(error) = self.close(TerminalEndReason::OwnerDropped) {
            eprintln!("terminal cleanup unconfirmed: {error}");
        }
    }
}
