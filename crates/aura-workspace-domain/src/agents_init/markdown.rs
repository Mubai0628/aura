//! Just enough Markdown to apply the `/init` rules: fenced code is skipped, headings are
//! found in both ATX and setext form, and link destinations are collected.

/// Lines outside fenced code blocks, with their 1-based line numbers.
pub(super) fn prose_lines(text: &str) -> Vec<(usize, &str)> {
    let mut fence: Option<(char, usize)> = None;
    let mut prose = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start_matches(' ');
        if line.len() - trimmed.len() <= 3
            && let Some(marker @ ('`' | '~')) = trimmed.chars().next()
        {
            let run = trimmed.chars().take_while(|c| *c == marker).count();
            if run >= 3 {
                match fence {
                    None => {
                        fence = Some((marker, run));
                        continue;
                    }
                    Some((open, length))
                        if open == marker && run >= length && trimmed[run..].trim().is_empty() =>
                    {
                        fence = None;
                        continue;
                    }
                    Some(_) => {}
                }
            }
        }
        if fence.is_none() {
            prose.push((index + 1, line));
        }
    }
    prose
}

/// The heading levels of the prose lines, ATX (`## x`) and setext (`x` over `---`).
pub(super) fn headings(text: &str) -> Vec<(usize, usize, String)> {
    let lines = prose_lines(text);
    let mut found = Vec::new();
    for (position, (number, line)) in lines.iter().enumerate() {
        if let Some(level) = atx_level(line) {
            found.push((*number, level, line.trim().to_owned()));
            continue;
        }
        let Some(level) = setext_level(line) else { continue };
        let Some((previous_number, previous)) = position.checked_sub(1).map(|p| lines[p]) else {
            continue;
        };
        if previous_number + 1 == *number && paragraph_text(previous) {
            found.push((*number, level, previous.trim().to_owned()));
        }
    }
    found
}

fn atx_level(line: &str) -> Option<usize> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    ((1..=6).contains(&hashes) && trimmed[hashes..].chars().next().is_none_or(char::is_whitespace))
        .then_some(hashes)
}

fn setext_level(line: &str) -> Option<usize> {
    let trimmed = line.trim();
    if line.len() - line.trim_start_matches(' ').len() > 3 || trimmed.is_empty() {
        return None;
    }
    if trimmed.chars().all(|c| c == '=') {
        Some(1)
    } else if trimmed.chars().all(|c| c == '-') {
        Some(2)
    } else {
        None
    }
}

/// A line that can be the text of a setext heading: not blank and not a block marker.
fn paragraph_text(line: &str) -> bool {
    let trimmed = line.trim_start();
    !trimmed.is_empty()
        && atx_level(line).is_none()
        && !trimmed.starts_with(['-', '*', '+', '>', '|'])
        && !trimmed.split_once('.').is_some_and(|(n, _)| n.chars().all(|c| c.is_ascii_digit()))
}

/// Link and image destinations in prose, excluding inline code: `[x](dest)`,
/// reference definitions `[x]: dest` and autolinks `<scheme:...>`.
pub(super) fn link_destinations(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (number, line) in prose_lines(text) {
        let line = without_code_spans(line);
        let trimmed = line.trim_start();
        if trimmed.starts_with('[')
            && let Some(close) = trimmed.find("]:")
            && !trimmed[1..close].contains(']')
        {
            let rest = trimmed[close + 2..].trim_start();
            let destination = rest.split_whitespace().next().unwrap_or_default();
            found.push((number, destination.trim_matches(['<', '>']).to_owned()));
        }
        let mut rest = line.as_str();
        while let Some(start) = rest.find("](") {
            let after = &rest[start + 2..];
            let destination = if let Some(inner) = after.strip_prefix('<') {
                inner.split('>').next().unwrap_or_default()
            } else {
                after.split([' ', ')', '\t']).next().unwrap_or_default()
            };
            found.push((number, destination.to_owned()));
            rest = after;
        }
        let mut rest = line.as_str();
        while let Some(start) = rest.find('<') {
            let after = &rest[start + 1..];
            if let Some(end) = after.find('>') {
                let candidate = &after[..end];
                if has_scheme(candidate) && !candidate.contains(char::is_whitespace) {
                    found.push((number, candidate.to_owned()));
                }
            }
            rest = after;
        }
    }
    found
}

/// `scheme:` as in a URL; a Windows drive letter (`C:`) also counts, so it is refused too.
pub(super) fn has_scheme(destination: &str) -> bool {
    let Some((scheme, _)) = destination.split_once(':') else { return false };
    let mut characters = scheme.chars();
    characters.next().is_some_and(|c| c.is_ascii_alphabetic())
        && characters.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '.' | '-'))
}

fn without_code_spans(line: &str) -> String {
    let mut kept = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find('`') {
        kept.push_str(&rest[..start]);
        let run = rest[start..].chars().take_while(|c| *c == '`').count();
        let fence = &rest[start..start + run];
        let after = &rest[start + run..];
        match after.find(fence) {
            Some(end) => rest = &after[end + run..],
            None => {
                kept.push_str(&rest[start..]);
                return kept;
            }
        }
    }
    kept.push_str(rest);
    kept
}
