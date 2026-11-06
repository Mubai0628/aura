//! What another conversation was about, for a message that refers to it: lines of its latest
//! summary or its last rounds, within about a thousand tokens. The newest lines are kept first; a
//! line that does not fit whole keeps its start and ends with "…".

/// About how many tokens a conversation's recap takes at most.
pub const RECAP_TOKENS: u64 = 1_000;

/// `lines` joined one per line, oldest first, the newest kept within `within` tokens as `tokens`
/// counts them.
pub fn recap(lines: &[String], within: u64, tokens: impl Fn(&str) -> u64) -> String {
    let mut kept: Vec<String> = Vec::new();
    let mut used = 0_u64;
    for line in lines.iter().rev() {
        // Each line after the first takes a line break too.
        let cost = tokens(line).saturating_add(u64::from(!kept.is_empty()));
        if used.saturating_add(cost) <= within {
            kept.push(line.clone());
            used = used.saturating_add(cost);
            continue;
        }
        let room = within.saturating_sub(used).saturating_sub(u64::from(!kept.is_empty()));
        let start = fitting(line, room.saturating_sub(tokens("…")), &tokens);
        if !start.is_empty() {
            kept.push(format!("{start}…"));
        }
        break;
    }
    kept.reverse();
    kept.join("\n")
}

/// The longest start of `line` within `room` tokens.
fn fitting<'a>(line: &'a str, room: u64, tokens: &impl Fn(&str) -> u64) -> &'a str {
    let ends: Vec<usize> =
        line.char_indices().map(|(at, _)| at).skip(1).chain([line.len()]).collect();
    let (mut low, mut high) = (0_usize, ends.len());
    while low < high {
        let middle = (low + high).div_ceil(2);
        if tokens(&line[..ends[middle - 1]]) <= room {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    if low == 0 { "" } else { &line[..ends[low - 1]] }
}
