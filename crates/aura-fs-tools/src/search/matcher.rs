use grep_matcher::Matcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};

use super::cancel::SearchCancel;
use crate::FsToolError;

pub(crate) fn compile_matcher(
    literal: bool,
    pattern: &str,
    case_sensitive: bool,
) -> Result<RegexMatcher, FsToolError> {
    RegexMatcherBuilder::new()
        .case_insensitive(!case_sensitive)
        .multi_line(false)
        .dot_matches_new_line(false)
        .fixed_strings(literal)
        .line_terminator(Some(b'\n'))
        .unicode(true)
        .build(pattern)
        .map_err(|error| FsToolError::InvalidRequest(format!("invalid search pattern: {error}")))
}

pub(crate) fn find_utf8_matches(
    matcher: &RegexMatcher,
    line: &str,
    cancel: &SearchCancel,
    max_matches: usize,
) -> Result<Vec<(usize, usize)>, FsToolError> {
    let bytes = line.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    loop {
        if cancel.is_cancelled() || found.len() >= max_matches {
            break;
        }
        let Some(matched) = matcher
            .find_at(bytes, at)
            .map_err(|error| FsToolError::Internal(format!("matcher: {error}")))?
        else {
            break;
        };
        found.push((matched.start(), matched.end()));
        at = if matched.end() > matched.start() {
            matched.end()
        } else {
            next_char_boundary(bytes, matched.start().max(at))
        };
        if at > bytes.len() {
            break;
        }
        if bytes.is_empty() {
            break;
        }
    }
    Ok(found)
}

fn next_char_boundary(bytes: &[u8], at: usize) -> usize {
    if at >= bytes.len() {
        return at.saturating_add(1);
    }
    let mut next = at.saturating_add(1);
    while next < bytes.len() && bytes[next] & 0b1100_0000 == 0b1000_0000 {
        next += 1;
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_line_matches_caret_dollar() {
        let matcher = compile_matcher(false, "^$", true).expect("pattern");
        let found = find_utf8_matches(&matcher, "", &SearchCancel::new(), 8).expect("matches");
        assert_eq!(found, vec![(0, 0)]);
    }

    #[test]
    fn dollar_matches_end_of_line() {
        let matcher = compile_matcher(false, "$", true).expect("pattern");
        let found = find_utf8_matches(&matcher, "abc", &SearchCancel::new(), 8).expect("matches");
        assert_eq!(found, vec![(3, 3)]);
    }

    #[test]
    fn zero_width_advances_on_multibyte() {
        let matcher = compile_matcher(false, "()", true).expect("pattern");
        let found = find_utf8_matches(&matcher, "éx", &SearchCancel::new(), 8).expect("matches");
        assert!(!found.is_empty());
        assert!(found.windows(2).all(|pair| pair[1].0 > pair[0].0 || pair[1].1 > pair[0].1));
    }
}
