//! What a reply becomes on its channel: Markdown turned into plain text where the platform cannot
//! show it, and split into the pieces the platform takes at once. A piece ends at a blank line,
//! else a line break, else the end of a sentence, else a space, else where it has to; the line
//! breaks and spaces at a cut go, a line's indentation stays, and no piece is blank. In Markdown
//! that is kept, a code block cut between two pieces is closed at the end of the first and opened
//! again at the start of the next.
use aura_plugin_host::{ChannelAbilities, MAX_CHANNEL_TEXT_CHARS, ReplyFormat};

/// Where a sentence ends. `.` is left out: it is as often part of a number or a name.
const SENTENCE_ENDS: &[char] = &['。', '！', '？', '；', '!', '?', ';'];
/// Room kept at the end of a piece of Markdown to close a code block cut there.
const CLOSE_ROOM: usize = 8;
/// With less room than this left for the text itself, Markdown is cut as plain text is.
const MIN_ROOM: usize = 10;

/// The pieces of `text` its channel takes, each with the format it goes as. For a channel the
/// service did not report, the text keeps its format and is split only to fit one protocol line.
#[must_use]
pub fn outgoing(
    text: &str,
    format: ReplyFormat,
    channel: Option<&ChannelAbilities>,
) -> Vec<(String, ReplyFormat)> {
    let (text, format, most) = match channel {
        Some(channel) if format == ReplyFormat::Markdown && !channel.markdown => {
            (plain_text(text), ReplyFormat::Text, channel.max_text_chars)
        }
        Some(channel) => (text.to_owned(), format, channel.max_text_chars),
        None => (text.to_owned(), format, MAX_CHANNEL_TEXT_CHARS),
    };
    let most = usize::try_from(most).unwrap_or(usize::MAX);
    let pieces = match format {
        ReplyFormat::Markdown => split_markdown(&text, most),
        ReplyFormat::Text => split_text(&text, most),
    };
    pieces.into_iter().map(|piece| (piece, format)).collect()
}

#[derive(Clone, Copy)]
enum Cut {
    Line,
    Space,
    Hard,
}

/// `text` in pieces of at most `most` characters (at least one).
#[must_use]
pub fn split_text(text: &str, most: usize) -> Vec<String> {
    let most = most.max(1);
    let mut pieces = Vec::new();
    let mut rest = text;
    while let Some((end, _)) = rest.char_indices().nth(most) {
        let (piece, after) = cut(rest, end);
        if !piece.trim().is_empty() {
            pieces.push(piece.to_owned());
        }
        rest = after;
    }
    if !rest.trim().is_empty() || pieces.is_empty() {
        pieces.push(rest.to_owned());
    }
    pieces
}

/// Markdown in pieces as [`split_text`] cuts it, each within `most` characters with the fences
/// that close and open again a code block cut there.
fn split_markdown(text: &str, most: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    let mut rest = text;
    let mut fence: Option<Fence> = None;
    loop {
        let reopen = fence.as_ref().map_or_else(String::new, |open| format!("{}\n", open.line));
        let room = most.saturating_sub(reopen.chars().count());
        if rest.chars().count() <= room {
            if !rest.trim().is_empty() || pieces.is_empty() {
                pieces.push(format!("{reopen}{rest}"));
            }
            break;
        }
        let room = room.saturating_sub(CLOSE_ROOM);
        let Some((end, _)) = rest.char_indices().nth(room).filter(|_| room >= MIN_ROOM) else {
            return split_text(text, most);
        };
        let (piece, after) = cut(rest, end);
        let open = fence_after(piece, fence.take());
        if !piece.trim().is_empty() {
            let close = open.as_ref().map_or_else(String::new, |open| format!("\n{}", open.marker));
            pieces.push(format!("{reopen}{piece}{close}"));
        }
        fence = open;
        rest = after;
    }
    if pieces.iter().any(|piece| piece.chars().count() > most) {
        return split_text(text, most);
    }
    pieces
}

/// The piece cut from the start of `rest` within its first `end` bytes, and what is left after.
fn cut(rest: &str, end: usize) -> (&str, &str) {
    let (at, kind) = cut_in(&rest[..end]);
    (trim_end(&rest[..at], kind), trim_start(&rest[at..], kind))
}

/// Where in `window` the piece ends, and what kind of place that is. A place at the very start
/// does not count: the piece would be empty, or only a punctuation mark.
fn cut_in(window: &str) -> (usize, Cut) {
    if let Some(at) = window.rfind("\n\n").filter(|at| *at > 0) {
        return (at, Cut::Line);
    }
    if let Some(at) = window.rfind('\n').filter(|at| *at > 0) {
        return (at, Cut::Line);
    }
    let sentence = window.char_indices().rev().find(|(at, c)| *at > 0 && SENTENCE_ENDS.contains(c));
    if let Some((at, end)) = sentence {
        return (at + end.len_utf8(), Cut::Space);
    }
    if let Some(at) = window.rfind(' ').filter(|at| *at > 0) {
        return (at, Cut::Space);
    }
    (window.len(), Cut::Hard)
}

fn trim_end(piece: &str, cut: Cut) -> &str {
    let piece = piece.trim_end_matches('\n');
    match cut {
        Cut::Space => piece.trim_end_matches(' '),
        Cut::Line | Cut::Hard => piece,
    }
}

fn trim_start(rest: &str, cut: Cut) -> &str {
    match cut {
        Cut::Space => rest.trim_start_matches([' ', '\n']),
        Cut::Line | Cut::Hard => rest.trim_start_matches('\n'),
    }
}

/// A code block's opening line, and the run of backticks or tildes that ends the block.
struct Fence {
    line: String,
    marker: String,
}

impl Fence {
    /// `line` ends the block: its own mark at least as many times, and nothing else.
    fn closed_by(&self, line: &str) -> bool {
        let end = line.trim();
        end.starts_with(&self.marker) && end.trim_start_matches(&self.marker[..1]).is_empty()
    }
}

/// The code block `line` opens, if it opens one: three or more backticks or tildes.
fn opens(line: &str) -> Option<Fence> {
    let start = line.trim();
    let mark = start.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let marker: String = start.chars().take_while(|c| *c == mark).collect();
    (marker.len() >= 3).then(|| Fence { line: start.to_owned(), marker })
}

/// The code block `text` ends inside of, given the one it starts inside of.
fn fence_after(text: &str, mut fence: Option<Fence>) -> Option<Fence> {
    for line in text.split('\n') {
        fence = match fence {
            Some(open) if open.closed_by(line) => None,
            Some(open) => Some(open),
            None => opens(line),
        };
    }
    fence
}

/// Markdown as plain text: the marks of headings, bold, strikethrough and inline code taken off,
/// code blocks without their fences, a link as its text with the address after it. A word with
/// `_` in it, such as `__init__`, is left as it is.
#[must_use]
pub fn plain_text(markdown: &str) -> String {
    let mut lines = Vec::new();
    let mut fence: Option<Fence> = None;
    for line in markdown.split('\n') {
        fence = match fence {
            Some(open) if open.closed_by(line) => None,
            Some(open) => {
                lines.push(line.to_owned());
                Some(open)
            }
            None => {
                let open = opens(line);
                if open.is_none() {
                    lines.push(inline(heading(line)));
                }
                open
            }
        };
    }
    lines.join("\n")
}

fn heading(line: &str) -> &str {
    let start = line.trim_start();
    let marks = start.bytes().take_while(|b| *b == b'#').count();
    if (1..=6).contains(&marks) && start[marks..].starts_with(' ') {
        start[marks..].trim_start()
    } else {
        line
    }
}

/// One line outside a code block: inline code as it is, the text around it without its marks.
fn inline(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        let Some(close) = rest[open + 1..].find('`') else { break };
        out.push_str(&prose(&rest[..open]));
        out.push_str(&rest[open + 1..open + 1 + close]);
        rest = &rest[open + 1 + close + 1..];
    }
    out.push_str(&prose(rest));
    out
}

fn prose(text: &str) -> String {
    links(text).replace("**", "").replace("~~", "")
}

/// Links and images as their text and address, an address in `<…>` as itself.
fn links(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(next) = rest.chars().next() {
        if let Some(after) = rest.strip_prefix('<')
            && (after.starts_with("http://") || after.starts_with("https://"))
            && let Some(close) = after.find('>')
        {
            out.push_str(&after[..close]);
            rest = &after[close + 1..];
            continue;
        }
        let label = if rest.starts_with("![") {
            2
        } else if rest.starts_with('[') {
            1
        } else {
            0
        };
        if label > 0
            && let Some(middle) = rest[label..].find("](")
            && !rest[label..label + middle].contains(['[', ']'])
            && let Some(close) = closing(&rest[label + middle + 2..])
        {
            let words = &rest[label..label + middle];
            let inside = &rest[label + middle + 2..label + middle + 2 + close];
            // A title after the address is left out.
            let address = inside.split_whitespace().next().unwrap_or_default();
            if address.is_empty() || words == address {
                out.push_str(words);
            } else if words.is_empty() {
                out.push_str(address);
            } else {
                out.push_str(&format!("{words}（{address}）"));
            }
            rest = &rest[label + middle + 2 + close + 1..];
            continue;
        }
        out.push(next);
        rest = &rest[next.len_utf8()..];
    }
    out
}

/// Where the `)` that ends a link's address is, past the pairs of brackets inside it.
fn closing(address: &str) -> Option<usize> {
    let mut depth = 0_usize;
    for (at, c) in address.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(at),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}
