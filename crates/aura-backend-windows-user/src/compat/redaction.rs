//! Hides the values of the user's credentials in the output of a command Aura runs.
//!
//! Output arrives in pieces, so a value split across two pieces is held back until the next piece
//! decides it. Only what is held may be the start of a value; everything else passes at once.

/// What a hidden value is replaced with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cover {
    /// `[已隐藏:NAME]`, for output that is read as text.
    Label,
    /// One `*` per character, for a terminal that lays out its screen by the original length.
    Stars,
}

/// Values shorter than this are left as they are: they occur in ordinary output too often.
pub const MIN_HIDDEN_CHARS: usize = 4;

struct Pattern {
    bytes: Vec<u8>,
    cover: Vec<u8>,
}

pub struct Redactor {
    patterns: Vec<Pattern>,
    held: Vec<u8>,
}

impl Redactor {
    /// A redactor for `(variable name, value)` pairs; each value is also hidden URL-encoded.
    pub fn new(secrets: &[(String, String)], cover: Cover) -> Self {
        let mut patterns = Vec::new();
        for (name, value) in secrets {
            if value.chars().count() < MIN_HIDDEN_CHARS {
                continue;
            }
            let encoded = url_encoded(value);
            for text in [value.as_str(), encoded.as_str()] {
                if patterns.iter().any(|pattern: &Pattern| pattern.bytes == text.as_bytes()) {
                    continue;
                }
                let cover = match cover {
                    Cover::Label => format!("[已隐藏:{name}]").into_bytes(),
                    Cover::Stars => vec![b'*'; text.chars().count()],
                };
                patterns.push(Pattern { bytes: text.as_bytes().to_vec(), cover });
            }
        }
        // The longest value that starts at a place wins, so a value inside another is not
        // uncovered by the shorter one's replacement.
        patterns.sort_by_key(|pattern| std::cmp::Reverse(pattern.bytes.len()));
        Self { patterns, held: Vec::new() }
    }

    /// A redactor with nothing to hide.
    pub fn none() -> Self {
        Self { patterns: Vec::new(), held: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// The part of the output so far that can be decided now, with every value replaced.
    pub fn push(&mut self, input: &[u8]) -> Vec<u8> {
        if self.patterns.is_empty() {
            return input.to_vec();
        }
        self.held.extend_from_slice(input);
        let (out, decided) = self.scan(false);
        self.held.drain(..decided);
        out
    }

    /// Everything still held, once no more output will come.
    pub fn finish(&mut self) -> Vec<u8> {
        let (out, _) = self.scan(true);
        self.held.clear();
        out
    }

    fn scan(&self, last: bool) -> (Vec<u8>, usize) {
        let data = &self.held;
        let mut out = Vec::with_capacity(data.len());
        let mut at = 0;
        while at < data.len() {
            let rest = &data[at..];
            if let Some(pattern) =
                self.patterns.iter().find(|pattern| rest.starts_with(&pattern.bytes))
            {
                out.extend_from_slice(&pattern.cover);
                at += pattern.bytes.len();
                continue;
            }
            if !last
                && self.patterns.iter().any(|pattern| {
                    pattern.bytes.len() > rest.len() && pattern.bytes.starts_with(rest)
                })
            {
                return (out, at);
            }
            out.push(data[at]);
            at += 1;
        }
        (out, data.len())
    }
}

/// Percent-encodes everything but the unreserved characters, as a URL query would carry a value.
fn url_encoded(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}
