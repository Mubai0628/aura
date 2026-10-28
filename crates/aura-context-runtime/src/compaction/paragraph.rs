//! The summary a compaction asks for: one paragraph of plain prose, which takes the place of the
//! history it covers and of the summary before it. Core keeps which sources it replaced, so the
//! model writes no headings and lists no anchors; headings or line breaks it writes all the same
//! are folded into the one paragraph.
use super::{CompactionValidationError, ParsedCompactionOutput};

/// The paragraph in the answer, or why the answer cannot stand for the history.
pub fn parse_paragraph_output(
    output: &str,
    character_target: u64,
) -> Result<ParsedCompactionOutput, CompactionValidationError> {
    let normalized = output.replace("\r\n", "\n").replace('\r', "\n");
    let paragraph = normalized
        .lines()
        .map(|line| line.trim().trim_start_matches('#').trim())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if paragraph.is_empty() {
        return Err(CompactionValidationError::OutputEmpty);
    }
    if paragraph.chars().count() as u64
        > character_target.saturating_mul(crate::COMPACTION_OUTPUT_HARD_LIMIT_FACTOR)
    {
        return Err(CompactionValidationError::OutputTooLarge);
    }
    Ok(ParsedCompactionOutput {
        historical_snapshot: paragraph,
        active_turn_summary: String::new(),
        structured_compaction: String::new(),
        referenced_anchors: Vec::new(),
        full_markdown: normalized,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_becomes_one_paragraph_and_only_an_empty_or_far_too_long_one_is_refused() {
        let parsed = parse_paragraph_output(
            "# Summary\n\n用户要做一个网页游戏。\n- 已写好 index.html\r\n下一步：加计分。\n",
            100,
        )
        .expect("a paragraph");
        assert_eq!(
            parsed.historical_snapshot,
            "Summary 用户要做一个网页游戏。 - 已写好 index.html 下一步：加计分。"
        );
        assert!(parsed.active_turn_summary.is_empty() && parsed.referenced_anchors.is_empty());
        assert_eq!(
            parse_paragraph_output(" \n#\n", 100),
            Err(CompactionValidationError::OutputEmpty)
        );
        let long = "字".repeat(401);
        assert_eq!(
            parse_paragraph_output(&long, 100),
            Err(CompactionValidationError::OutputTooLarge)
        );
        assert!(parse_paragraph_output(&"字".repeat(400), 100).is_ok(), "four times the target");
    }
}
