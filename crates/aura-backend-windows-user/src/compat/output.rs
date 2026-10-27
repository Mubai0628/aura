//! One combined retained-byte quota, bounded reads, and benchmark-only marker observation.
//! Credentials are hidden before a byte is retained, so the quota counts what is kept.
use super::redaction::Redactor;
use aura_windows_platform::NativeSandboxProcess;
use std::time::{Duration, Instant};

pub(super) struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub truncated: bool,
    pub marker: Option<Duration>,
    limit: usize,
    hidden: [Redactor; 2],
}

impl Output {
    pub fn new(limit: usize, hidden: [Redactor; 2]) -> Self {
        Self {
            stdout: Vec::new(),
            stderr: Vec::new(),
            truncated: false,
            marker: None,
            limit,
            hidden,
        }
    }

    /// Releases what was held back as the possible start of a value, once no process can write.
    pub fn finish(&mut self) {
        let [out, err] = &mut self.hidden;
        let (out, err) = (out.finish(), err.finish());
        self.keep(&out, &err);
    }

    fn keep(&mut self, out: &[u8], err: &[u8]) {
        let remaining = self.limit.saturating_sub(self.stdout.len() + self.stderr.len());
        self.truncated |= out.len() + err.len() > remaining;
        let kept = remaining.min(out.len());
        self.stdout.extend_from_slice(&out[..kept]);
        let kept = remaining.saturating_sub(kept).min(err.len());
        self.stderr.extend_from_slice(&err[..kept]);
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
            let out = self.hidden[0].push(&out);
            let err = self.hidden[1].push(&err);
            self.keep(&out, &err);
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
