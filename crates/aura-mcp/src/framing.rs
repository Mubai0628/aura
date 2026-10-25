//! stdio framing: one JSON-RPC message per line (UTF-8, `\n`; a trailing `\r` is ignored).
//! A line longer than the limit ends the connection instead of growing without bound.

pub const MAX_MESSAGE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum FrameError {
    #[error("服务器发来的单条消息超过 {MAX_MESSAGE_BYTES} 字节")]
    TooLarge,
}

#[derive(Debug, Default)]
pub struct LineFramer {
    pending: Vec<u8>,
}

impl LineFramer {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Complete lines from these bytes, in order; empty lines are skipped.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, FrameError> {
        let mut lines = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                let mut line = std::mem::take(&mut self.pending);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if !line.iter().all(u8::is_ascii_whitespace) {
                    lines.push(line);
                }
            } else {
                if self.pending.len() >= MAX_MESSAGE_BYTES {
                    self.pending.clear();
                    return Err(FrameError::TooLarge);
                }
                self.pending.push(byte);
            }
        }
        Ok(lines)
    }

    /// Bytes of an unfinished line (for diagnostics only).
    #[must_use]
    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }
}
