use super::policy::MAX_EXCERPT_CHARS;

pub(crate) struct Excerpt {
    pub(crate) text: String,
    pub(crate) start_character_offset: u64,
    pub(crate) truncated_before: bool,
    pub(crate) truncated_after: bool,
}

/// Build the excerpt window for a match given by its byte span and its
/// character offsets.
///
/// The window is at most [`MAX_EXCERPT_CHARS`] scalars, so the context is
/// collected by walking *outwards from the match* — bounded by the window size —
/// instead of skipping forward from the start of the line. On a 1 MiB line with
/// hundreds of hits, skipping from the start per hit is what made projecting a
/// long dense line quadratic; the bounded walk is the same result at
/// `O(window)` per hit.
///
/// `total_chars` is the character count of the trimmed line, so a short line is
/// still returned whole, exactly as the previous implementation did.
pub(crate) fn excerpt_around_bytes(
    line: &str,
    match_start_byte: usize,
    match_end_byte: usize,
    match_start_chars: usize,
    match_len_chars: usize,
    total_chars: usize,
) -> Excerpt {
    let line = line.trim_end_matches(['\r', '\n']);
    if total_chars <= MAX_EXCERPT_CHARS {
        return Excerpt {
            text: line.to_owned(),
            start_character_offset: 0,
            truncated_before: false,
            truncated_after: false,
        };
    }
    let start_byte = match_start_byte.min(line.len());
    let end_byte = match_end_byte.clamp(start_byte, line.len());
    if match_len_chars >= MAX_EXCERPT_CHARS {
        // The match alone fills the window: take its first window of scalars.
        let (window_end, _) = walk_forward(line, start_byte, MAX_EXCERPT_CHARS);
        return Excerpt {
            text: line[start_byte..window_end].to_owned(),
            start_character_offset: u64::try_from(match_start_chars).unwrap_or(u64::MAX),
            truncated_before: start_byte > 0,
            truncated_after: window_end < line.len(),
        };
    }
    let extra = MAX_EXCERPT_CHARS - match_len_chars;
    let before_wanted = extra / 2;
    let after_wanted = extra - before_wanted;
    let (mut window_start, mut chars_before) = walk_back(line, start_byte, before_wanted);
    let (mut window_end, mut chars_after) = walk_forward(line, end_byte, after_wanted);
    // A line that ends before the forward context is complete is filled from the
    // left instead, so the window is as large as the line allows.
    let mut shortfall = MAX_EXCERPT_CHARS
        .saturating_sub(chars_before.saturating_add(match_len_chars).saturating_add(chars_after));
    if shortfall > 0 {
        let (start, filled) = walk_back(line, window_start, shortfall);
        window_start = start;
        chars_before = chars_before.saturating_add(filled);
        shortfall = shortfall.saturating_sub(filled);
    }
    if shortfall > 0 {
        let (end, filled) = walk_forward(line, window_end, shortfall);
        window_end = end;
        chars_after = chars_after.saturating_add(filled);
    }
    // Defensive: the walks above cannot exceed the bound, but a caller that
    // passes a byte span wider than its character span must still get a bounded
    // window, so an over-wide window is trimmed from the left.
    let window_chars = chars_before.saturating_add(match_len_chars).saturating_add(chars_after);
    if window_chars > MAX_EXCERPT_CHARS {
        let (start, dropped) = walk_forward(line, window_start, window_chars - MAX_EXCERPT_CHARS);
        window_start = start;
        chars_before = chars_before.saturating_sub(dropped);
    }
    Excerpt {
        text: line[window_start..window_end].to_owned(),
        start_character_offset: u64::try_from(match_start_chars.saturating_sub(chars_before))
            .unwrap_or(u64::MAX),
        truncated_before: window_start > 0,
        truncated_after: window_end < line.len(),
    }
}

/// Walk back from `byte` by at most `chars` scalars. Returns the byte offset the
/// walk reached and how many scalars it actually covered.
fn walk_back(line: &str, byte: usize, chars: usize) -> (usize, usize) {
    let byte = byte.min(line.len());
    let mut cursor = byte;
    let mut taken = 0_usize;
    if chars == 0 {
        return (cursor, 0);
    }
    for (index, _) in line[..byte].char_indices().rev() {
        if taken >= chars {
            break;
        }
        cursor = index;
        taken += 1;
    }
    (cursor, taken)
}

/// Walk forward from `byte` by at most `chars` scalars. Returns the byte offset
/// the walk reached and how many scalars it actually covered.
fn walk_forward(line: &str, byte: usize, chars: usize) -> (usize, usize) {
    let byte = byte.min(line.len());
    let mut cursor = byte;
    let mut taken = 0_usize;
    if chars == 0 {
        return (cursor, 0);
    }
    for (index, scalar) in line[byte..].char_indices() {
        if taken >= chars {
            break;
        }
        cursor = byte.saturating_add(index).saturating_add(scalar.len_utf8());
        taken += 1;
    }
    (cursor, taken)
}

/// Character offset of a byte offset, for tests and differential oracles.
#[cfg(test)]
#[must_use]
pub(crate) fn byte_to_char(line: &str, byte_offset: usize) -> usize {
    line.get(..byte_offset.min(line.len())).map(|prefix| prefix.chars().count()).unwrap_or(0)
}

/// Character offsets for every match boundary on one line, resolved in a single
/// ordered walk.
///
/// Counting from the start of the line for each boundary costs
/// `matches × line length`, which is what makes a long line with many hits
/// expensive. `find_utf8_matches` returns matches in ascending, non-overlapping
/// byte order, so one walk yields every offset in `line length + matches`.
pub(crate) fn char_boundaries(line: &str, matches: &[(usize, usize)]) -> Vec<(usize, usize)> {
    fn advance_to(line: &str, byte: usize, chars_seen: &mut usize, cursor: &mut usize) -> usize {
        let byte = byte.min(line.len());
        if byte > *cursor {
            *chars_seen += line[*cursor..byte].chars().count();
            *cursor = byte;
        }
        *chars_seen
    }

    let mut boundaries = Vec::with_capacity(matches.len());
    let mut chars_seen = 0_usize;
    let mut cursor = 0_usize;
    for (start, end) in matches {
        let start_chars = advance_to(line, *start, &mut chars_seen, &mut cursor);
        let end_chars = advance_to(line, *end, &mut chars_seen, &mut cursor);
        boundaries.push((start_chars, end_chars));
    }
    boundaries
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Byte span for a match described in character offsets, for tests.
    fn bytes_for(line: &str, start_chars: usize, len_chars: usize) -> (usize, usize) {
        let mut start = line.len();
        let mut end = line.len();
        for (index, (byte, _)) in line.char_indices().enumerate() {
            if index == start_chars {
                start = byte;
            }
            if index == start_chars + len_chars {
                end = byte;
                break;
            }
        }
        (start, end)
    }

    fn fast(line: &str, start_chars: usize, len_chars: usize) -> Excerpt {
        let line = line.trim_end_matches(['\r', '\n']);
        let total = line.chars().count();
        let (start, end) = bytes_for(line, start_chars, len_chars);
        excerpt_around_bytes(line, start, end, start_chars, len_chars, total)
    }

    /// The character-skipping implementation this module replaced, kept as the
    /// oracle the bounded walk must reproduce exactly.
    fn reference(line: &str, match_start_chars: usize, match_len_chars: usize) -> Excerpt {
        let line = line.trim_end_matches(['\r', '\n']);
        let total = line.chars().count();
        if total <= MAX_EXCERPT_CHARS {
            return Excerpt {
                text: line.to_owned(),
                start_character_offset: 0,
                truncated_before: false,
                truncated_after: false,
            };
        }
        let match_start = match_start_chars.min(total);
        let match_end = match_start.saturating_add(match_len_chars).min(total);
        let match_len = match_end.saturating_sub(match_start);
        let (start, end) = if match_len >= MAX_EXCERPT_CHARS {
            (match_start, match_start.saturating_add(MAX_EXCERPT_CHARS).min(total))
        } else {
            let extra = MAX_EXCERPT_CHARS.saturating_sub(match_len);
            let before = extra / 2;
            let after = extra.saturating_sub(before);
            let mut start = match_start.saturating_sub(before);
            let mut end = (match_end + after).min(total);
            if end.saturating_sub(start) < MAX_EXCERPT_CHARS {
                start = end.saturating_sub(MAX_EXCERPT_CHARS);
            }
            end = start.saturating_add(MAX_EXCERPT_CHARS).min(total);
            start = end.saturating_sub(MAX_EXCERPT_CHARS.min(end));
            (start, end)
        };
        let text: String = line.chars().skip(start).take(end.saturating_sub(start)).collect();
        Excerpt {
            text,
            start_character_offset: u64::try_from(start).unwrap_or(u64::MAX),
            truncated_before: start > 0,
            truncated_after: end < total,
        }
    }

    #[test]
    fn long_match_is_truncated_to_1024_scalars() {
        let line: String = "é".repeat(2048);
        let excerpt = fast(&line, 0, 2048);
        assert_eq!(excerpt.text.chars().count(), 1024);
        assert!(!excerpt.truncated_before);
        assert!(excerpt.truncated_after);
        assert_eq!(excerpt.start_character_offset, 0);
    }

    #[test]
    fn context_window_stays_within_1024() {
        let line: String = format!("{}NEEDLE{}", "a".repeat(2000), "b".repeat(2000));
        let excerpt = fast(&line, 2000, 6);
        assert!(excerpt.text.chars().count() <= 1024);
        assert!(excerpt.text.contains("NEEDLE"));
        assert!(excerpt.truncated_before);
        assert!(excerpt.truncated_after);
    }

    #[test]
    fn the_bounded_walk_matches_the_character_skipping_oracle() {
        // Positions, match lengths and scripts that exercise every branch: whole
        // short line, match at each end, window clipped on one side, a match that
        // fills the window, and multi-byte scalars where bytes and characters
        // disagree.
        let cases: Vec<(String, usize, usize)> = vec![
            ("short line".to_owned(), 0, 5),
            ("x".repeat(3000), 0, 1),
            ("x".repeat(3000), 2990, 1),
            ("x".repeat(3000), 1500, 3),
            ("x".repeat(3000), 1200, 1024),
            ("x".repeat(3000), 1200, 900),
            (format!("{}NEEDLE{}", "é".repeat(1500), "β".repeat(1500)), 1500, 6),
            (format!("{}NEEDLE", "γ".repeat(2000)), 2000, 6),
            (format!("NEEDLE{}", "δ".repeat(2000)), 0, 6),
            (format!("{}\r\n", "x".repeat(2024)), 2000, 4),
            ("x".repeat(1024), 500, 4),
            ("x".repeat(1025), 500, 4),
            ("x".repeat(1026), 1019, 6),
        ];
        for (line, start, len) in cases {
            let expected = reference(&line, start, len);
            let actual = fast(&line, start, len);
            assert_eq!(
                actual.text,
                expected.text,
                "text for {start}/{len} of {} chars",
                line.chars().count()
            );
            assert_eq!(
                actual.start_character_offset, expected.start_character_offset,
                "offset for {start}/{len}"
            );
            assert_eq!(
                actual.truncated_before, expected.truncated_before,
                "truncated_before for {start}/{len}"
            );
            assert_eq!(
                actual.truncated_after, expected.truncated_after,
                "truncated_after for {start}/{len}"
            );
        }
    }

    #[test]
    fn one_pass_offsets_match_counting_from_the_start() {
        let line: String = "αβγ needle δε needle ζη needle\n".to_owned();
        let needle = "needle";
        let matches: Vec<(usize, usize)> =
            line.match_indices(needle).map(|(start, found)| (start, start + found.len())).collect();
        assert_eq!(matches.len(), 3);
        let boundaries = char_boundaries(&line, &matches);
        for ((start, end), (start_chars, end_chars)) in matches.iter().zip(&boundaries) {
            assert_eq!(*start_chars, byte_to_char(&line, *start));
            assert_eq!(*end_chars, byte_to_char(&line, *end));
        }
    }

    #[test]
    fn a_long_line_with_many_hits_is_projected_in_one_pass() {
        // The removed hotspot: resolving each hit by walking from the start of
        // the line. This measures both shapes on the same input; the bounded walk
        // must be far faster, and the ratio is what justifies the change instead
        // of a faster machine.
        const HITS: usize = 400;
        let mut line = String::new();
        let mut matches = Vec::with_capacity(HITS);
        for index in 0..HITS {
            matches.push((line.len(), line.len() + "needle".len()));
            line.push_str("needle");
            line.push_str(&"x".repeat(2600 - (index % 7)));
        }
        let line = line.trim_end_matches('x').to_owned();
        let needle = "needle";

        let naive_started = std::time::Instant::now();
        let mut naive_chars = 0_usize;
        for (start, end) in &matches {
            let start_chars = byte_to_char(&line, *start);
            let length = byte_to_char(&line, *end).saturating_sub(start_chars);
            let excerpt = reference(&line, start_chars, length);
            naive_chars = naive_chars.saturating_add(excerpt.text.chars().count());
        }
        let naive = naive_started.elapsed();

        let fast_started = std::time::Instant::now();
        let total_chars = line.chars().count();
        let boundaries = char_boundaries(&line, &matches);
        let mut fast_chars = 0_usize;
        for ((start, end), (start_chars, end_chars)) in matches.iter().zip(&boundaries) {
            let length = end_chars.saturating_sub(*start_chars);
            let excerpt =
                excerpt_around_bytes(&line, *start, *end, *start_chars, length, total_chars);
            fast_chars = fast_chars.saturating_add(excerpt.text.chars().count());
        }
        let single = fast_started.elapsed();

        // Compare outside the timed loops: calling the oracle per hit while the
        // clock runs would measure the implementation under test.
        for (index, ((start, end), (start_chars, end_chars))) in
            matches.iter().zip(&boundaries).enumerate()
        {
            if index % 37 != 0 {
                continue;
            }
            let length = end_chars.saturating_sub(*start_chars);
            let excerpt =
                excerpt_around_bytes(&line, *start, *end, *start_chars, length, total_chars);
            assert_eq!(
                excerpt.text,
                reference(&line, *start_chars, length).text,
                "the fast path must project the same excerpt"
            );
        }

        eprintln!(
            "long-line projection: per-hit-from-start={naive:?} bounded-walk={single:?} \
             line_bytes={} hits={HITS} needle={needle}",
            line.len()
        );
        assert_eq!(naive_chars, fast_chars, "both paths must project the same hits");
        assert!(
            single * 5 < naive,
            "the bounded walk must beat skipping from the start: naive={naive:?} single={single:?}"
        );
    }
}
