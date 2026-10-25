//! Server-sent events, as a Streamable HTTP server answers: `data:` lines joined with line
//! breaks, an event ends at a blank line, lines starting with `:` are comments. Only events of
//! the default type (`message`) carry JSON-RPC.

/// One complete event.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
    pub id: Option<String>,
}

impl SseEvent {
    /// Whether this event carries a JSON-RPC message.
    #[must_use]
    pub fn is_message(&self) -> bool {
        self.event.as_deref().is_none_or(|kind| kind == "message")
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
#[error("服务器发来的事件超过 {0} 字节")]
pub struct SseTooLarge(pub usize);

/// An incremental parser with a size limit for one line and one event.
#[derive(Debug)]
pub struct SseParser {
    limit: usize,
    line: Vec<u8>,
    /// A `\r` ended the last line; a following `\n` belongs to it.
    after_cr: bool,
    data: Option<String>,
    event: Option<String>,
    id: Option<String>,
}

impl SseParser {
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self { limit, line: Vec::new(), after_cr: false, data: None, event: None, id: None }
    }

    /// Feeds bytes and returns the events they complete.
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<SseEvent>, SseTooLarge> {
        let mut events = Vec::new();
        for &byte in bytes {
            if self.after_cr {
                self.after_cr = false;
                if byte == b'\n' {
                    continue;
                }
            }
            match byte {
                b'\r' | b'\n' => {
                    self.after_cr = byte == b'\r';
                    let line = std::mem::take(&mut self.line);
                    if let Some(event) = self.line_done(&line)? {
                        events.push(event);
                    }
                }
                _ => {
                    self.line.push(byte);
                    if self.line.len() > self.limit {
                        return Err(SseTooLarge(self.limit));
                    }
                }
            }
        }
        Ok(events)
    }

    fn line_done(&mut self, line: &[u8]) -> Result<Option<SseEvent>, SseTooLarge> {
        if line.is_empty() {
            let event = self.data.take().map(|data| SseEvent {
                event: self.event.take(),
                data,
                id: self.id.clone(),
            });
            self.event = None;
            return Ok(event);
        }
        if line[0] == b':' {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(line);
        let (field, value) = match text.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (text.as_ref(), ""),
        };
        match field {
            "data" => {
                let data = match &mut self.data {
                    Some(existing) => {
                        existing.push('\n');
                        existing.push_str(value);
                        existing
                    }
                    None => self.data.insert(value.to_owned()),
                };
                if data.len() > self.limit {
                    return Err(SseTooLarge(self.limit));
                }
            }
            "event" => self.event = Some(value.to_owned()),
            "id" if !value.contains('\0') => self.id = Some(value.to_owned()),
            _ => {}
        }
        Ok(None)
    }
}
