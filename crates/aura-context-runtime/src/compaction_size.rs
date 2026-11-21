//! Counting the narrative in a legacy structured background for diagnostics.

/// The characters in the legacy narrative: every line except the headings and
/// the Truth-Index's anchor lines.
#[must_use]
pub fn compaction_summary_characters(output: &str) -> u64 {
    output
        .lines()
        .filter(|line| !line.starts_with('#') && !is_anchor_line(line))
        .map(|line| line.chars().count() as u64 + 1)
        .sum()
}

fn is_anchor_line(line: &str) -> bool {
    line.trim_end()
        .strip_prefix("- a")
        .is_some_and(|digits| digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_digit()))
}
