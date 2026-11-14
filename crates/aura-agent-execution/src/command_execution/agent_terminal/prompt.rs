//! The line a terminal shows, so a secure input is typed only while the terminal still shows the
//! prompt it was asked at. A line is kept as a digest under this terminal's own random keys, never
//! as text: comparing two of them is all it is for.
use std::collections::hash_map::{DefaultHasher, RandomState};
use std::hash::{BuildHasher, Hasher};

/// What the terminal showed at one moment: how far its output had come, its last line with text,
/// and how many times the user had resized it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct Shown {
    pub offset: u64,
    pub line: Option<u64>,
    pub resizes: u64,
}

impl Shown {
    /// Whether the terminal still shows what it showed: nothing printed since; or, once the user
    /// has resized it (ConPTY then prints the whole screen again), still the same line with text.
    pub fn still(&self, now: &Shown) -> bool {
        now.offset == self.offset
            || now.resizes != self.resizes && now.line.is_some() && now.line == self.line
    }
}

/// What a terminal shows, for the check before a secure input is typed.
#[derive(Default)]
pub(super) struct Screen {
    line: PromptLine,
    /// How many times the user resized the terminal.
    resizes: u64,
}

impl Screen {
    pub fn push(&mut self, bytes: &[u8]) {
        self.line.push(bytes);
    }

    pub fn resized(&mut self) {
        self.resizes += 1;
    }

    /// What it shows with its output read up to `offset`.
    pub fn shown(&self, offset: u64) -> Shown {
        Shown { offset, line: self.line.shown(), resizes: self.resizes }
    }
}

#[derive(Clone, Copy, Default)]
enum Escape {
    #[default]
    None,
    Started,
    /// Inside a control sequence, until its final byte.
    Sequence,
    /// Inside a string (a title, a device control), until BEL or ESC \.
    Text,
    /// ESC inside a string: a backslash after it ends the string.
    TextEnding,
    /// The one byte after a character set choice.
    Charset,
}

/// The last line with text of a terminal's output. A line ends at a line feed, a carriage return,
/// or a cursor move to another line or place on the screen; spaces, controls and escape sequences
/// are left out, so ConPTY painting the same screen again gives the same lines.
struct PromptLine {
    keys: RandomState,
    escape: Escape,
    current: DefaultHasher,
    text: bool,
    last: Option<u64>,
}

impl Default for PromptLine {
    fn default() -> Self {
        let keys = RandomState::new();
        Self { current: keys.build_hasher(), keys, escape: Escape::None, text: false, last: None }
    }
}

impl PromptLine {
    pub fn push(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.escape = match self.escape {
                Escape::None => match byte {
                    0x1b => Escape::Started,
                    b'\n' | b'\r' | 0x0b | 0x0c => {
                        self.end_line();
                        Escape::None
                    }
                    b' ' | 0..=0x1f | 0x7f => Escape::None,
                    _ => {
                        self.current.write_u8(byte);
                        self.text = true;
                        Escape::None
                    }
                },
                Escape::Started => match byte {
                    b'[' => Escape::Sequence,
                    b']' | b'P' | b'X' | b'^' | b'_' => Escape::Text,
                    b'(' | b')' | b'*' | b'+' => Escape::Charset,
                    // Index, next line, reverse index, restore the cursor, reset.
                    b'D' | b'E' | b'M' | b'8' | b'c' => {
                        self.end_line();
                        Escape::None
                    }
                    _ => Escape::None,
                },
                Escape::Sequence => {
                    if (0x40..=0x7e).contains(&byte) {
                        // Cursor position, up, down, to a row, next or previous line, erase the
                        // display. Moves along the same line (forward, back, to a column) and
                        // erasing in it keep the line.
                        if matches!(byte, b'H' | b'f' | b'A' | b'B' | b'd' | b'E' | b'F' | b'J') {
                            self.end_line();
                        }
                        Escape::None
                    } else {
                        Escape::Sequence
                    }
                }
                Escape::Text => match byte {
                    0x07 => Escape::None,
                    0x1b => Escape::TextEnding,
                    _ => Escape::Text,
                },
                Escape::TextEnding => {
                    if byte == b'\\' {
                        Escape::None
                    } else {
                        Escape::Text
                    }
                }
                Escape::Charset => Escape::None,
            };
        }
    }

    /// The line the cursor is on when it has text, or else the last one above it that had.
    pub fn shown(&self) -> Option<u64> {
        if self.text { Some(self.current.finish()) } else { self.last }
    }

    fn end_line(&mut self) {
        if self.text {
            self.last = Some(self.current.finish());
        }
        self.current = self.keys.build_hasher();
        self.text = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(prompt: &mut PromptLine, bytes: &str) -> Option<u64> {
        prompt.push(bytes.as_bytes());
        prompt.shown()
    }

    #[test]
    fn nothing_shown_has_no_line() {
        let mut prompt = PromptLine::default();
        assert_eq!(line(&mut prompt, ""), None);
        assert_eq!(line(&mut prompt, "\r\n  \r\n\x1b[2J\x1b[H\x1b]0;title\x07"), None);
    }

    #[test]
    fn a_repaint_of_the_same_screen_shows_the_same_line() {
        let mut prompt = PromptLine::default();
        let asked =
            line(&mut prompt, "Microsoft Windows\r\n\r\nC:\\work>set /p X=secret: \r\nsecret: ");
        let repainted = line(
            &mut prompt,
            "\x1b[?25l\x1b[H\x1b[2JMicrosoft Windows\x1b[K\r\n\x1b[K\r\nC:\\work>set /p X=secret: \
             \x1b[K\r\nsecret:\x1b[1C\x1b[K\x1b[J\x1b[4;9H\x1b[?25h",
        );
        assert!(asked.is_some());
        assert_eq!(asked, repainted);
    }

    #[test]
    fn a_line_split_across_reads_or_coloured_is_the_same_line() {
        let mut prompt = PromptLine::default();
        let whole = line(&mut prompt, "[sudo] password for aura: ");
        prompt.push(b"\r\n[su");
        prompt.push(b"do] pass\x1b[1mword\x1b[0m for ");
        assert_eq!(line(&mut prompt, "aura: "), whole);
    }

    #[test]
    fn what_follows_the_prompt_is_another_line() {
        let mut prompt = PromptLine::default();
        let asked = line(&mut prompt, "[sudo] password for aura: ");
        let after =
            line(&mut prompt, "\r\nsudo: timed out reading password\r\naura@host:~$ \x1b[?25h");
        assert!(after.is_some());
        assert_ne!(after, asked);
        // Printed on the prompt's own line, it changes that line too.
        let mut late = PromptLine::default();
        let asked = line(&mut late, "secret: ");
        assert_ne!(line(&mut late, "LATE\r\n"), asked);
    }

    #[test]
    fn the_cursor_moving_elsewhere_ends_a_line() {
        let mut prompt = PromptLine::default();
        let first = line(&mut prompt, "first");
        assert_ne!(line(&mut prompt, "\x1b[5;1Hsecond"), first);
        let second = prompt.shown();
        // Forward, back and to a column stay on the line; erasing in it keeps what it shows.
        prompt.push(b"\x1b[3C\x1b[2D\x1b[10G\x1b[K");
        assert_eq!(prompt.shown(), second);
    }

    #[test]
    fn still_shown_after_no_output_or_after_a_resize_with_the_same_line() {
        let asked = Shown { offset: 10, line: Some(1), resizes: 0 };
        let same = |offset, line, resizes| asked.still(&Shown { offset, line, resizes });
        assert!(same(10, Some(1), 0));
        assert!(!same(30, Some(1), 0), "output without a resize");
        assert!(same(30, Some(1), 1), "a repaint of the same line");
        assert!(!same(30, Some(2), 1), "another line after the resize");
        let nothing = Shown { offset: 10, line: None, resizes: 0 };
        assert!(!nothing.still(&Shown { offset: 30, line: None, resizes: 1 }));
    }

    #[test]
    fn a_screen_counts_resizes_and_reports_its_line() {
        let mut screen = Screen::default();
        screen.push(b"secret: ");
        let asked = screen.shown(8);
        screen.resized();
        screen.push(b"\x1b[H\x1b[2Jsecret: \x1b[1;9H");
        assert!(asked.still(&screen.shown(30)));
        screen.push(b"\r\nPS C:\\> ");
        assert!(!asked.still(&screen.shown(45)));
    }
}
