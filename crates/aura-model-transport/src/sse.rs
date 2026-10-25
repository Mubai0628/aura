use std::str;

use crate::ModelTransportError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SseLimits {
    pub max_line_bytes: usize,
    pub max_event_bytes: usize,
}

/// One event can carry a whole finished output item (the Responses API sends
/// it as one line, a tool call that writes a file included), so a line and an event are bounded by
/// the smallest response an agent attempt may receive (ADR 0001 §6: 32 MiB), not by 256 KiB.
const MAX_EVENT_BYTES: usize = 32 * 1024 * 1024;

impl Default for SseLimits {
    fn default() -> Self {
        Self { max_line_bytes: MAX_EVENT_BYTES, max_event_bytes: MAX_EVENT_BYTES }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ProviderStreamFrame {
    pub event: Option<String>,
    pub id: Option<String>,
    pub retry_ms: Option<u64>,
    pub data: String,
}

pub struct SseDecoder {
    limits: SseLimits,
    pending: Vec<u8>,
    /// How much of `pending` is known to hold no line end: a long line arriving in small pieces is
    /// scanned once, not once per piece.
    scanned: usize,
    frame: ProviderStreamFrame,
    data_lines: Vec<String>,
    frame_bytes: usize,
    has_fields: bool,
}

impl SseDecoder {
    #[must_use]
    pub fn new(limits: SseLimits) -> Self {
        Self {
            limits,
            pending: Vec::new(),
            scanned: 0,
            frame: ProviderStreamFrame::default(),
            data_lines: Vec::new(),
            frame_bytes: 0,
            has_fields: false,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<ProviderStreamFrame>, ModelTransportError> {
        let mut pending = std::mem::take(&mut self.pending);
        pending.extend_from_slice(bytes);
        let mut frames = Vec::new();
        // Every complete line is read in place; what follows the last line end is kept once.
        let mut start = 0;
        let mut from = self.scanned;
        while let Some(found) = pending[from..].iter().position(|byte| *byte == b'\n') {
            let end = from + found;
            let line = pending[start..end].strip_suffix(b"\r").unwrap_or(&pending[start..end]);
            if line.len() > self.limits.max_line_bytes {
                return Err(ModelTransportError::SseLineTooLarge);
            }
            if let Some(frame) = self.process_line(line)? {
                frames.push(frame);
            }
            start = end + 1;
            from = start;
        }
        pending.drain(..start);
        if pending.len() > self.limits.max_line_bytes {
            return Err(ModelTransportError::SseLineTooLarge);
        }
        self.scanned = pending.len();
        self.pending = pending;
        Ok(frames)
    }

    pub fn finish(&mut self) -> Result<Vec<ProviderStreamFrame>, ModelTransportError> {
        let mut frames = Vec::new();
        self.scanned = 0;
        if !self.pending.is_empty() {
            let mut line = std::mem::take(&mut self.pending);
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if line.len() > self.limits.max_line_bytes {
                return Err(ModelTransportError::SseLineTooLarge);
            }
            if let Some(frame) = self.process_line(&line)? {
                frames.push(frame);
            }
        }
        if let Some(frame) = self.dispatch_frame() {
            frames.push(frame);
        }
        Ok(frames)
    }

    fn process_line(
        &mut self,
        bytes: &[u8],
    ) -> Result<Option<ProviderStreamFrame>, ModelTransportError> {
        let line = str::from_utf8(bytes).map_err(|_| ModelTransportError::InvalidSseUtf8)?;
        if line.is_empty() {
            return Ok(self.dispatch_frame());
        }
        if line.starts_with(':') {
            return Ok(None);
        }
        let (field, value) = line
            .split_once(':')
            .map_or((line, ""), |(field, value)| (field, value.strip_prefix(' ').unwrap_or(value)));
        self.frame_bytes = self.frame_bytes.saturating_add(bytes.len());
        if self.frame_bytes > self.limits.max_event_bytes {
            return Err(ModelTransportError::SseEventTooLarge);
        }
        match field {
            "event" => {
                self.frame.event = Some(value.to_owned());
                self.has_fields = true;
            }
            "data" => {
                self.data_lines.push(value.to_owned());
                self.has_fields = true;
            }
            "id" if !value.contains('\0') => {
                self.frame.id = Some(value.to_owned());
                self.has_fields = true;
            }
            "retry" => {
                self.frame.retry_ms =
                    Some(value.parse::<u64>().map_err(|_| ModelTransportError::InvalidSseRetry)?);
                self.has_fields = true;
            }
            _ => {}
        }
        Ok(None)
    }

    fn dispatch_frame(&mut self) -> Option<ProviderStreamFrame> {
        if !self.has_fields {
            self.frame_bytes = 0;
            return None;
        }
        self.frame.data = self.data_lines.join("\n");
        self.data_lines.clear();
        self.frame_bytes = 0;
        self.has_fields = false;
        Some(std::mem::take(&mut self.frame))
    }
}

#[cfg(test)]
mod tests {
    use super::{ProviderStreamFrame, SseDecoder, SseLimits};

    #[test]
    fn decodes_chunked_utf8_multiline_data_and_standard_fields() {
        let mut decoder = SseDecoder::new(SseLimits::default());
        let mut frames = decoder.push(b": keepalive\r\nevent: message\r\nid: evt-1\r\nretry: 125\r\ndata: first\r\ndata: \xe4\xbd")
            .expect("first chunk");
        assert!(frames.is_empty());
        frames.extend(decoder.push(b"\xa0\r\n\r\n").expect("second chunk"));
        assert_eq!(
            frames,
            vec![ProviderStreamFrame {
                event: Some("message".to_owned()),
                id: Some("evt-1".to_owned()),
                retry_ms: Some(125),
                data: "first\n你".to_owned(),
            }]
        );
    }

    #[test]
    fn finish_dispatches_a_final_event_without_blank_line() {
        let mut decoder = SseDecoder::new(SseLimits::default());
        assert!(decoder.push(b"data: final").expect("chunk").is_empty());
        assert_eq!(decoder.finish().expect("finish")[0].data, "final");
    }

    #[test]
    fn rejects_invalid_utf8_and_bounded_lines() {
        let mut decoder = SseDecoder::new(SseLimits { max_line_bytes: 3, max_event_bytes: 8 });
        assert!(decoder.push(b"1234").is_err());
        let mut decoder = SseDecoder::new(SseLimits::default());
        assert!(decoder.push(b"data: \xff\n").is_err());
    }

    /// The
    /// Responses API sends a finished output item as one line, and a whole file written into a tool
    /// call made that line larger than 256 KiB. One event is bounded by the attempt's response
    /// limit, not by a quarter of a megabyte, and arrives in small pieces.
    #[test]
    fn an_event_line_of_several_megabytes_arriving_in_small_chunks_decodes() {
        let body = "x".repeat(3 * 1024 * 1024);
        let stream = format!("event: response.output_item.done\ndata: {body}\n\n");
        let mut decoder = SseDecoder::new(SseLimits::default());
        let mut frames = Vec::new();
        for chunk in stream.as_bytes().chunks(4096) {
            frames.extend(decoder.push(chunk).expect("chunk"));
        }
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0].event.as_deref(), Some("response.output_item.done"));
        assert_eq!(frames[0].data.len(), body.len());
    }

    #[test]
    fn many_events_in_one_chunk_decode_in_order() {
        let stream: String = (0..10_000).map(|n| format!("data: {n}\r\n\r\n")).collect();
        let mut decoder = SseDecoder::new(SseLimits::default());
        let frames = decoder.push(stream.as_bytes()).expect("chunk");
        assert_eq!(frames.len(), 10_000);
        assert!(frames.iter().enumerate().all(|(n, frame)| frame.data == n.to_string()));
    }

    #[test]
    fn a_line_beyond_the_limit_fails_closed_whether_or_not_it_has_ended() {
        let limits = || SseLimits { max_line_bytes: 1024, max_event_bytes: 4096 };
        let mut unended = SseDecoder::new(limits());
        assert!(unended.push(b"data: ").expect("start").is_empty());
        assert!(unended.push(&[b'a'; 1024]).is_err());
        let mut ended = SseDecoder::new(limits());
        let mut line = vec![b'a'; 1025];
        line.push(b'\n');
        assert!(ended.push(&line).is_err());
        let mut fits = SseDecoder::new(limits());
        let mut line = b"data: ".to_vec();
        line.extend([b'a'; 1018]);
        line.extend(b"\n\n");
        assert_eq!(fits.push(&line).expect("fits")[0].data.len(), 1018);
    }
}
