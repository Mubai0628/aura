//! One combined retained-byte quota, bounded reads, and benchmark-only marker observation.
use aura_windows_platform::NativeSandboxProcess;
use std::time::{Duration, Instant};

pub(super) struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub truncated: bool,
    pub marker: Option<Duration>,
    limit: usize,
}

impl Output {
    pub fn new(limit: usize) -> Self {
        Self { stdout: Vec::new(), stderr: Vec::new(), truncated: false, marker: None, limit }
    }
    pub fn drain(
        &mut self,
        process: &NativeSandboxProcess,
        entered: Instant,
    ) -> std::io::Result<()> {
        for _ in 0..4 {
            let remaining = self.limit.saturating_sub(self.stdout.len() + self.stderr.len());
            let out = process.drain_stdout((remaining + 1).min(8192))?;
            let err = process.drain_stderr((remaining.saturating_sub(out.len()) + 1).min(8192))?;
            let empty = out.is_empty() && err.is_empty();
            self.truncated |= out.len() + err.len() > remaining;
            let keep = remaining.min(out.len());
            self.stdout.extend_from_slice(&out[..keep]);
            let keep = remaining.saturating_sub(keep).min(err.len());
            self.stderr.extend_from_slice(&err[..keep]);
            const MARKER: &[u8] = b"__AURA_COMPAT_START__";
            if self.marker.is_none()
                && self.stdout.windows(MARKER.len()).any(|bytes| bytes == MARKER)
            {
                self.marker = Some(entered.elapsed());
            }
            if empty || self.truncated {
                break;
            }
        }
        Ok(())
    }
}
