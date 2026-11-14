//! What an Agent terminal shows, line by line, for `terminal.read`. Its output goes through a
//! screen model, so a line redrawn in place, wrapped at the terminal's width or erased reads as the
//! screen shows it. Lines that leave the top of the screen are kept as text; a full-screen
//! program's own screen keeps nothing once it closes.
use super::privacy::EchoPrivacy;
use aura_windows_platform::TerminalSize;
use std::collections::VecDeque;
use std::num::NonZeroU16;
use vt100::capture::{
    BasicFormattedCaptureRange, BasicFormattedCaptureState, RowContents, basic_formatted_to_plain,
};

/// How many lines are kept after they leave the screen, and how much text they may hold together.
const KEPT_LINES: usize = 5000;
const KEPT_BYTES: usize = 4 * 1024 * 1024;
/// The most text one page returns; a longer line is cut.
pub(super) const PAGE_BYTES: usize = 32 * 1024;

/// Rows that left the top of the main screen and are not yet split into lines, written as vt100's
/// basic capture writes them: a line break before a row unless the row above wrapped onto it.
#[derive(Default)]
struct Scrolled {
    capture: String,
    state: BasicFormattedCaptureState,
}

impl vt100::Callbacks for Scrolled {
    fn on_scroll(&mut self, row: RowContents<'_>, alternate_screen: bool) {
        if !alternate_screen {
            // Writing to a String cannot fail.
            let _ = row.write_formatted_basic(&mut self.capture, &mut self.state);
        }
    }
}

/// A page of lines; numbers count the terminal's first line with text as 1.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Page {
    pub lines: String,
    pub first: u64,
    pub last: u64,
    pub next: Option<u64>,
    pub earliest: u64,
}

pub(super) struct ScreenLines {
    /// Gone once the terminal ended: everything it showed is then in `kept`.
    parser: Option<vt100::Parser<Scrolled>>,
    /// Lines that left the screen, oldest first, with echoes of secure input masked.
    kept: VecDeque<String>,
    kept_bytes: usize,
    /// How many lines left before the oldest one kept.
    dropped: u64,
    /// Whether a line with text has been kept: blank lines before the first are not counted.
    started: bool,
    /// The output offset read up to, and the user's resizes waiting for the output printed after
    /// them (ConPTY prints the screen again at the new size).
    fed: u64,
    resizes: VecDeque<(u64, TerminalSize)>,
}

impl ScreenLines {
    /// A screen of `size`, reading the terminal's output from `offset` on.
    pub fn new(size: TerminalSize, offset: u64) -> Self {
        let (rows, columns) =
            dimensions(size).unwrap_or((vt100::Parser::DEFAULT_ROWS, vt100::Parser::DEFAULT_COLS));
        Self {
            parser: Some(vt100::Parser::new_with_callbacks(rows, columns, 0, Scrolled::default())),
            kept: VecDeque::new(),
            kept_bytes: 0,
            dropped: 0,
            started: false,
            fed: offset,
            resizes: VecDeque::new(),
        }
    }

    /// Lines of saved text, as a terminal of an earlier Core left it: no screen, only its text.
    pub fn saved(text: &str) -> Self {
        let mut lines = Self::new(TerminalSize { columns: 80, rows: 24 }, 0);
        lines.parser = None;
        for line in text.split('\n') {
            lines.keep(line.trim_end().to_owned());
        }
        while lines.kept.back().is_some_and(String::is_empty) {
            lines.kept.pop_back();
        }
        lines
    }

    /// The output from `start` on; resizes take effect at the offset they were made at.
    pub fn push(&mut self, start: u64, bytes: &[u8], privacy: &EchoPrivacy) {
        let Some(parser) = self.parser.as_mut() else {
            return;
        };
        let (mut start, mut bytes) = (start, bytes);
        while let Some(&(at, size)) = self.resizes.front() {
            if at > start + bytes.len() as u64 {
                break;
            }
            let split = (at.saturating_sub(start) as usize).min(bytes.len());
            parser.process(&bytes[..split]);
            resize(parser, size);
            self.resizes.pop_front();
            (start, bytes) = (start + split as u64, &bytes[split..]);
        }
        parser.process(bytes);
        self.fed = self.fed.max(start + bytes.len() as u64);
        self.harvest(privacy);
    }

    /// The user resized the terminal when its output had reached `at`.
    pub fn resize_at(&mut self, at: u64, size: TerminalSize, privacy: &EchoPrivacy) {
        if at > self.fed {
            self.resizes.push_back((at, size));
        } else if let Some(parser) = self.parser.as_mut() {
            resize(parser, size);
            self.harvest(privacy);
        }
    }

    /// The terminal ended: what it showed last is kept as lines, masked while the secure inputs
    /// typed into it are still known.
    pub fn end(&mut self, privacy: &EchoPrivacy) {
        let shown = self.shown(privacy);
        self.parser = None;
        self.resizes.clear();
        for line in shown {
            self.keep(line);
        }
    }

    /// The lines on the screen now, the line still leaving it first; trailing blank rows left out.
    pub fn shown(&self, privacy: &EchoPrivacy) -> Vec<String> {
        let Some(parser) = &self.parser else {
            return Vec::new();
        };
        let (screen, scrolled) = (parser.screen(), parser.callbacks());
        let mut capture = scrolled.capture.clone();
        // Writing to a String cannot fail.
        let _ = if screen.alternate_screen() {
            // A full-screen program's screen does not continue the shell's last line.
            if !capture.is_empty() {
                capture.push('\n');
            }
            screen.write_contents_formatted_basic(&mut capture, BasicFormattedCaptureRange::Visible)
        } else {
            let mut state = scrolled.state.clone();
            screen.write_contents_formatted_basic(
                &mut capture,
                BasicFormattedCaptureRange::Full(&mut state),
            )
        };
        let mut lines =
            capture.split('\n').map(|line| privacy.masked_line(&plain(line))).collect::<Vec<_>>();
        while lines.last().is_some_and(String::is_empty) {
            lines.pop();
        }
        if !self.started {
            let blank = lines.iter().take_while(|line| line.is_empty()).count();
            lines.drain(..blank);
        }
        lines
    }

    /// Up to `max_lines` lines from line `from` on, or the newest ones without it; `shown` is what
    /// [`Self::shown`] returned.
    pub fn page(&self, shown: &[String], from: Option<u64>, max_lines: usize) -> Page {
        self.page_within(shown, from, max_lines, PAGE_BYTES)
    }

    /// The number of the newest line with text: output that comes next continues it or follows.
    pub fn last_line(&self, shown: &[String]) -> u64 {
        self.dropped + (self.kept.len() + shown.len()) as u64
    }

    /// [`Self::page`] holding at most `max_bytes` of text; a longer line is cut to fit.
    pub fn page_within(
        &self,
        shown: &[String],
        from: Option<u64>,
        max_lines: usize,
        max_bytes: usize,
    ) -> Page {
        let earliest = self.dropped + 1;
        let last = self.last_line(shown);
        let line = |number: u64| -> &str {
            let index = (number - earliest) as usize;
            match self.kept.get(index) {
                Some(line) => line,
                None => &shown[index - self.kept.len()],
            }
        };
        let max_lines = max_lines.max(1) as u64;
        let (first, end) = match from {
            None => {
                let (mut first, mut bytes) = (last + 1, 0);
                while first > earliest && last + 1 - first < max_lines {
                    bytes += line(first - 1).len() + 1;
                    if bytes > max_bytes && first <= last {
                        break;
                    }
                    first -= 1;
                }
                (first, last + 1)
            }
            Some(from) => {
                let first = from.max(earliest);
                let (mut end, mut bytes) = (first, 0);
                while end <= last && end - first < max_lines {
                    bytes += line(end).len() + 1;
                    if bytes > max_bytes && end > first {
                        break;
                    }
                    end += 1;
                }
                (first, end)
            }
        };
        let lines =
            (first..end).map(|number| cut(line(number), max_bytes)).collect::<Vec<_>>().join("\n");
        Page { lines, first, last, next: (end <= last).then_some(end), earliest }
    }

    /// Splits the rows that left the screen into the lines they complete.
    fn harvest(&mut self, privacy: &EchoPrivacy) {
        let Some(parser) = self.parser.as_mut() else {
            return;
        };
        let capture = &mut parser.callbacks_mut().capture;
        let Some(end) = capture.rfind('\n') else {
            return;
        };
        let complete = capture.drain(..=end).collect::<String>();
        for line in complete[..end].split('\n') {
            self.keep(privacy.masked_line(&plain(line)));
        }
    }

    fn keep(&mut self, line: String) {
        if !self.started && line.is_empty() {
            return;
        }
        self.started = true;
        self.kept_bytes += line.len();
        self.kept.push_back(line);
        while self.kept.len() > KEPT_LINES || self.kept_bytes > KEPT_BYTES && self.kept.len() > 1 {
            let oldest = self.kept.pop_front().expect("more than one line kept");
            self.kept_bytes -= oldest.len();
            self.dropped += 1;
        }
    }
}

fn dimensions(size: TerminalSize) -> Option<(NonZeroU16, NonZeroU16)> {
    Some((NonZeroU16::new(size.rows)?, NonZeroU16::new(size.columns)?))
}

fn resize(parser: &mut vt100::Parser<Scrolled>, size: TerminalSize) {
    if let Some((rows, columns)) = dimensions(size) {
        parser.set_size(rows, columns);
    }
}

/// A row of vt100's basic capture as plain text, without its colours or trailing spaces.
fn plain(formatted: &str) -> String {
    let mut line = basic_formatted_to_plain(formatted).collect::<String>();
    line.truncate(line.trim_end().len());
    line
}

/// A line longer than `limit`, cut where the page ends.
fn cut(line: &str, limit: usize) -> String {
    if line.len() <= limit {
        return line.to_owned();
    }
    let mut end = limit.saturating_sub(128);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…[这一行还有 {} 字节没有显示]", &line[..end], line.len() - end)
}

#[cfg(test)]
#[path = "lines_tests.rs"]
mod tests;
