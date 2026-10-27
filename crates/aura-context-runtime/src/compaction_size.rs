//! How long a compaction answer may be.
//!
//! The limit is a target for the summary text. The headings are fixed and the Truth-Index has a
//! line per anchor, so they grow with the history a pass covers, not with the summary, and do not
//! count. An answer is refused only well past the target: a summary a few times its target still
//! replaces a history far larger than itself, while refusing it ends the pass — one recorded
//! answer missed a 4,096-character limit by 21 characters, its Truth-Index alone being 730.

/// How far past its target a summary may run before the answer is refused.
pub const COMPACTION_OUTPUT_HARD_LIMIT_FACTOR: u64 = 4;

/// The characters of an answer that count against its limit: every line except the headings and
/// the Truth-Index's anchor lines.
#[must_use]
pub fn compaction_summary_characters(output: &str) -> u64 {
    output
        .lines()
        .filter(|line| !line.starts_with('#') && !is_anchor_line(line))
        .map(|line| line.chars().count() as u64 + 1)
        .sum()
}

/// Whether an answer runs past the hard limit for a target of `target` characters.
#[must_use]
pub fn compaction_output_too_large(output: &str, target: u64) -> bool {
    compaction_summary_characters(output)
        > target.saturating_mul(COMPACTION_OUTPUT_HARD_LIMIT_FACTOR)
}

fn is_anchor_line(line: &str) -> bool {
    line.trim_end()
        .strip_prefix("- a")
        .is_some_and(|digits| digits.len() == 6 && digits.bytes().all(|byte| byte.is_ascii_digit()))
}
