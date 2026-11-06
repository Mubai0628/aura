//! Paths typed after "@" in a message's words: "@" at the start of a line or after a space, then
//! a path up to the next space or Chinese punctuation, or a path in double quotes when it has
//! spaces. Whether one is a reference is decided by looking it up; the words are never changed.
use aura_domain_common::WorkspacePath;

/// The most paths typed after "@" that are looked up in one message; later ones are text.
pub(crate) const MOST_TYPED: usize = 200;

/// Chinese punctuation, which ends a typed path: Chinese text puts no space before what follows.
const CHINESE_PUNCTUATION: &[char] = &[
    '，', '。', '；', '：', '！', '？', '、', '（', '）', '【', '】', '「', '」', '『', '』', '《',
    '》', '“', '”', '‘', '’', '\u{3000}',
];

/// Punctuation that may follow a typed path in a sentence without belonging to it.
const TRAILING: &[char] = &['.', ',', ';', ':', '!', '?', ')', ']', '}', '>', '\'', '"'];

/// One path typed after "@": the ways to read it, the whole token first.
pub(crate) struct TypedPath {
    pub(crate) readings: Vec<WorkspacePath>,
}

/// The paths typed after "@" in `text`, in the order they appear, at most [`MOST_TYPED`].
pub(crate) fn typed_paths(text: &str) -> Vec<TypedPath> {
    let mut found = Vec::new();
    let mut previous: Option<char> = None;
    for (at, c) in text.char_indices() {
        let starts = c == '@' && previous.is_none_or(char::is_whitespace);
        previous = Some(c);
        if !starts {
            continue;
        }
        let rest = &text[at + 1..];
        let tokens = if let Some(quoted) = rest.strip_prefix('"') {
            // A quoted path ends on its line; without the closing quote it is text.
            let line = quoted.split('\n').next().unwrap_or_default();
            line.find('"').map(|end| vec![&line[..end]]).unwrap_or_default()
        } else {
            let end = rest
                .find(|c: char| c.is_whitespace() || CHINESE_PUNCTUATION.contains(&c))
                .unwrap_or(rest.len());
            let whole = &rest[..end];
            let trimmed = whole.trim_end_matches(TRAILING);
            if trimmed.len() < whole.len() { vec![whole, trimmed] } else { vec![whole] }
        };
        let readings: Vec<WorkspacePath> = tokens
            .into_iter()
            .filter(|token| !token.is_empty())
            .filter_map(|token| WorkspacePath::parse(token).ok())
            .collect();
        if !readings.is_empty() {
            found.push(TypedPath { readings });
            if found.len() == MOST_TYPED {
                break;
            }
        }
    }
    found
}
