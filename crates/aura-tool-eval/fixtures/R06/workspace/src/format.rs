use crate::Level;

pub(super) struct Header<'a> {
    pub timestamp: u64,
    pub level: Level,
    pub message: &'a str,
}

/// The header starts in column zero; indentation belongs to continuation lines.
pub(super) fn header(line: &str) -> Option<Header<'_>> {
    if line.starts_with([' ', '\t']) {
        return None;
    }
    let mut fields = line.splitn(3, ' ');
    let stamp = fields.next()?;
    if stamp.is_empty() || !stamp.chars().all(|value| value.is_ascii_digit()) {
        return None;
    }
    let timestamp = stamp.parse().ok()?;
    let level = Level::parse(fields.next()?)?;
    let message = fields.next().unwrap_or("");
    Some(Header { timestamp, level, message })
}

pub(super) fn continuation(line: &str) -> Option<&str> {
    line.starts_with([' ', '\t']).then(|| line.trim_start_matches([' ', '\t']))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_header() {
        let parsed = header("42 WARN disk almost full").expect("header");
        assert_eq!(parsed.timestamp, 42);
        assert_eq!(parsed.level, Level::Warn);
        assert_eq!(parsed.message, "disk almost full");
    }

    #[test]
    fn empty_message_is_valid() {
        assert_eq!(header("0 DEBUG").expect("empty message").message, "");
    }

    #[test]
    fn malformed_headers_are_ignored() {
        for line in ["abc INFO broken", "1 TRACE unknown", " 1 INFO nested", ""] {
            assert!(header(line).is_none(), "{line:?}");
        }
    }

    #[test]
    fn indentation_is_removed_only_from_continuations() {
        assert_eq!(continuation(" \t at frame"), Some("at frame"));
        assert_eq!(continuation("not indented"), None);
    }
}
