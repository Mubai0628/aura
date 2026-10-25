//! Bounded raw VT retention. Offsets are bytes; decoding belongs to the consumer.
use anyhow::{Result, ensure};
use std::collections::VecDeque;

#[derive(Clone, Debug)]
pub struct TerminalOutputPage {
    pub bytes: Vec<u8>,
    pub start_offset: u64,
    pub next_offset: u64,
    pub truncated: bool,
}
pub(super) struct OutputBuffer {
    bytes: VecDeque<u8>,
    limit: usize,
    start: u64,
}
impl OutputBuffer {
    pub(super) fn new(limit: usize) -> Result<Self> {
        ensure!(
            (1..=32 * 1024 * 1024).contains(&limit),
            "terminal.output_limit must be between 1 byte and 32 MiB"
        );
        Ok(Self { bytes: VecDeque::new(), limit, start: 0 })
    }
    pub(super) fn append(&mut self, input: &[u8]) {
        let removed = self.bytes.len().saturating_add(input.len()).saturating_sub(self.limit);
        let from_old = removed.min(self.bytes.len());
        self.bytes.drain(..from_old);
        self.start = self.start.saturating_add(removed as u64);
        self.bytes.extend(&input[removed - from_old..]);
    }
    pub(super) fn read(&self, offset: u64, max_bytes: usize) -> Result<TerminalOutputPage> {
        ensure!(
            max_bytes > 0 && max_bytes <= 32 * 1024 * 1024,
            "terminal.read_limit must be between 1 byte and 32 MiB"
        );
        let end = self.start + self.bytes.len() as u64;
        ensure!(offset <= end, "terminal.offset_out_of_range");
        let start_offset = offset.max(self.start);
        let start = (start_offset - self.start) as usize;
        let count = max_bytes.min(self.bytes.len() - start);
        Ok(TerminalOutputPage {
            bytes: self.bytes.range(start..start + count).copied().collect(),
            start_offset,
            next_offset: start_offset + count as u64,
            truncated: offset < self.start,
        })
    }
}
