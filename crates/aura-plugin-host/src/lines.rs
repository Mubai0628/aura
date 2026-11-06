//! Splitting what a service writes into lines. A line longer than the limit is dropped up to its
//! next line break and reported once; the lines after it are read as usual.

/// One line, or the report that one was too long.
#[derive(Debug, Eq, PartialEq)]
pub enum Line {
    Text(Vec<u8>),
    TooLong,
}

pub struct LineReader {
    limit: usize,
    buffer: Vec<u8>,
    /// Inside a line that was already too long: dropped until its line break.
    skipping: bool,
}

impl LineReader {
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self { limit, buffer: Vec::new(), skipping: false }
    }

    /// The whole lines `chunk` completes, in order. Empty lines are skipped; a `\r` before the
    /// line break is not part of the line.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<Line> {
        let mut lines = Vec::new();
        for part in chunk.split_inclusive(|byte| *byte == b'\n') {
            let ends = part.last() == Some(&b'\n');
            let content = if ends { &part[..part.len() - 1] } else { part };
            if self.skipping {
                if ends {
                    self.skipping = false;
                }
                continue;
            }
            self.buffer.extend_from_slice(content);
            let length =
                self.buffer.len() - usize::from(ends && self.buffer.last() == Some(&b'\r'));
            if length > self.limit {
                self.buffer.clear();
                self.skipping = !ends;
                lines.push(Line::TooLong);
                continue;
            }
            if ends {
                let mut line = std::mem::take(&mut self.buffer);
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                if !line.iter().all(u8::is_ascii_whitespace) {
                    lines.push(Line::Text(line));
                }
            }
        }
        lines
    }
}

/// Standard error split into lines for the log. Oversized lines are withheld whole because a
/// truncated secret cannot be recognized by the redactor that receives these lines.
pub struct ErrorLines {
    buffer: Vec<u8>,
    limit: usize,
    skipping: bool,
}

impl ErrorLines {
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self { buffer: Vec::new(), limit, skipping: false }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        let mut lines = Vec::new();
        for part in chunk.split_inclusive(|byte| *byte == b'\n') {
            let ends = part.last() == Some(&b'\n');
            if self.skipping {
                if ends {
                    self.skipping = false;
                }
                continue;
            }
            let content = if ends { &part[..part.len() - 1] } else { part };
            if self.buffer.len().saturating_add(content.len()) > self.limit.saturating_mul(4) {
                self.buffer.clear();
                self.skipping = !ends;
                lines.push("日志行过长，内容已隐藏".to_owned());
                continue;
            }
            self.buffer.extend_from_slice(content);
            if !ends {
                continue;
            }
            // A `\r` before the line break is part of a Windows line break, not of the line.
            if self.buffer.last() == Some(&b'\r') {
                self.buffer.pop();
            }
            if let Some(text) = self.finish() {
                lines.push(text);
            }
        }
        lines
    }

    /// What is left when the service ended.
    pub fn finish(&mut self) -> Option<String> {
        let text = String::from_utf8_lossy(&std::mem::take(&mut self.buffer)).into_owned();
        if self.skipping || text.trim().is_empty() {
            return None;
        }
        Some(if text.chars().count() > self.limit {
            "日志行过长，内容已隐藏".to_owned()
        } else {
            // Whitespace may belong to a secret, so preserve it until masking has run.
            text
        })
    }
}
