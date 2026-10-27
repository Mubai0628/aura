//! A model that thinks in circles never answers. One recorded step streamed 18 minutes of reasoning
//! and no answer, one paragraph coming back 24 times, until the user stopped it. Before the answer
//! starts, a paragraph of reasoning (blank-line separated, at least [`MIN_PARAGRAPH_CHARS`]) that
//! comes back [`REASONING_LOOP_REPEATS`] times ends the attempt as a broken stream: the step is
//! asked again, and a loop that persists ends the turn with its reason.
//!
//! This catches repeated paragraphs only. Lines repeated without blank lines between them, and
//! text that repeats in other shapes, are not detected here; the stream's other limits bound them.
//! CR and CRLF line ends count as LF, each byte is searched once, and a paragraph longer than
//! [`MAX_PARAGRAPH_BYTES`] is skipped rather than kept, so a stream without blank lines costs
//! linear time and bounded memory.
use std::collections::HashMap;

use aura_model_domain::ModelStreamEvent;

/// How many times one paragraph of reasoning may appear before the attempt stops.
pub const REASONING_LOOP_REPEATS: u32 = 5;
/// Shorter paragraphs ("Checking the file.") may honestly repeat.
const MIN_PARAGRAPH_CHARS: usize = 40;
/// Distinct paragraphs remembered; a long, varied reasoning never needs more.
const MAX_PARAGRAPHS: usize = 4_096;
/// The longest paragraph tracked, in bytes.
const MAX_PARAGRAPH_BYTES: usize = 16 * 1024;

#[derive(Default)]
pub(crate) struct ReasoningLoopGuard {
    partial: String,
    /// Where the next search for a blank line starts in `partial`.
    scanned: usize,
    /// The last character was a CR, so a following LF ends the same line.
    after_cr: bool,
    /// The paragraph in progress grew past [`MAX_PARAGRAPH_BYTES`]; its end is not counted.
    skipping: bool,
    seen: HashMap<String, u32>,
    answering: bool,
}

impl ReasoningLoopGuard {
    /// Takes one decoded batch; true once the reasoning has looped.
    pub(crate) fn observe(&mut self, events: &[ModelStreamEvent]) -> bool {
        for event in events {
            match event {
                ModelStreamEvent::ReasoningDelta { text, .. }
                | ModelStreamEvent::ReasoningSummaryDelta { text, .. }
                    if !self.answering =>
                {
                    if self.push(text) {
                        return true;
                    }
                }
                ModelStreamEvent::AssistantTextDelta { .. }
                | ModelStreamEvent::ToolCallStarted { .. }
                | ModelStreamEvent::RefusalDelta { .. } => {
                    self.answering = true;
                    self.partial.clear();
                    self.scanned = 0;
                    self.seen.clear();
                }
                _ => {}
            }
        }
        false
    }

    fn push(&mut self, text: &str) -> bool {
        for ch in text.chars() {
            let after_cr = std::mem::replace(&mut self.after_cr, ch == '\r');
            match ch {
                '\r' => self.partial.push('\n'),
                '\n' if after_cr => {}
                _ => self.partial.push(ch),
            }
        }
        while let Some(found) = self.partial[self.scanned..].find("\n\n") {
            let end = self.scanned + found;
            // The same length rule as for a paragraph still arriving (below), so a text gives the
            // same answer however its deltas cut it.
            let untracked = std::mem::take(&mut self.skipping) || end > MAX_PARAGRAPH_BYTES;
            let paragraph = if untracked {
                String::new()
            } else {
                self.partial[..end].split_whitespace().collect::<Vec<_>>().join(" ")
            };
            self.partial.drain(..end + 2);
            self.scanned = 0;
            if untracked || paragraph.chars().count() < MIN_PARAGRAPH_CHARS {
                continue;
            }
            if self.seen.len() >= MAX_PARAGRAPHS && !self.seen.contains_key(&paragraph) {
                continue;
            }
            let count = self.seen.entry(paragraph).or_default();
            *count += 1;
            if *count >= REASONING_LOOP_REPEATS {
                return true;
            }
        }
        // A blank line may begin at the last LF; everything before it has been searched, and it is
        // all paragraph: once that is longer than a tracked one, the paragraph is.
        let ends_in_lf = self.partial.ends_with('\n');
        self.scanned = self.partial.len() - usize::from(ends_in_lf);
        if self.scanned > MAX_PARAGRAPH_BYTES {
            self.partial.clear();
            if ends_in_lf {
                self.partial.push('\n');
            }
            self.scanned = 0;
            self.skipping = true;
        }
        false
    }
}

#[cfg(test)]
#[path = "reasoning_loop_tests.rs"]
mod tests;
