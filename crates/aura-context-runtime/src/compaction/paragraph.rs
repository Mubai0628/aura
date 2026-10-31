//! The summary a compaction asks for: one paragraph of plain prose between `<summary>` tags, which
//! takes the place of the history it covers and of the summary before it. Core keeps which sources
//! it replaced, so the model writes no headings and lists no anchors; headings or line breaks it
//! writes all the same are folded into the one paragraph. The tags tell the summary from what a
//! model writes around it; an answer without them is the summary as a whole.
use super::{CompactionValidationError, ParsedCompactionOutput};

const OPEN: &str = "<summary>";
const CLOSE: &str = "</summary>";
/// Words a first line uses that only announces the summary ("Here is the summary:").
const ANNOUNCING: [&str; 3] = ["summary", "摘要", "总结"];

/// Whether the answer holds a summary between both its tags.
pub(super) fn has_complete_summary(output: &str) -> bool {
    output.rfind(CLOSE).is_some_and(|end| output[..end].contains(OPEN))
}

/// What the last pair of tags holds, or after the last opening tag when it was never closed;
/// without tags, the answer less a first line that only announces the summary.
fn summary_text(output: &str) -> &str {
    if let Some(end) = output.rfind(CLOSE)
        && let Some(start) = output[..end].rfind(OPEN)
    {
        return &output[start + OPEN.len()..end];
    }
    if let Some(start) = output.rfind(OPEN) {
        return &output[start + OPEN.len()..];
    }
    let answer = output.trim_start();
    match answer.split_once('\n') {
        Some((first, rest))
            if !rest.trim().is_empty()
                && first.trim_end().ends_with([':', '：'])
                && ANNOUNCING.iter().any(|word| first.to_lowercase().contains(word)) =>
        {
            rest
        }
        _ => answer,
    }
}

/// The paragraph in the answer, or why the answer cannot stand for the history.
pub fn parse_paragraph_output(
    output: &str,
    character_target: u64,
) -> Result<ParsedCompactionOutput, CompactionValidationError> {
    let normalized = output.replace("\r\n", "\n").replace('\r', "\n");
    let paragraph = summary_text(&normalized)
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

    #[test]
    fn the_summary_is_what_its_last_pair_of_tags_holds() {
        let summary = |answer: &str| {
            parse_paragraph_output(answer, 100).expect("a paragraph").historical_snapshot
        };
        assert_eq!(
            summary(
                "Here it is.\n<summary>\n用户要做网页游戏。\n下一步：计分。\n</summary>\nDone."
            ),
            "用户要做网页游戏。 下一步：计分。"
        );
        assert_eq!(
            summary("<summary>first</summary> and <summary>second</summary> after"),
            "second"
        );
        assert_eq!(summary("<summary>whole</summary> then <summary>cut"), "whole");
        assert_eq!(summary("I will write it: <summary>never closed"), "never closed");
        assert!(has_complete_summary("a <summary>b</summary>"));
        assert!(!has_complete_summary("a <summary>b") && !has_complete_summary("b</summary>"));
        assert_eq!(
            parse_paragraph_output("<summary> \n </summary>", 100),
            Err(CompactionValidationError::OutputEmpty)
        );
    }

    #[test]
    fn without_tags_a_first_line_that_only_announces_the_summary_is_left_out() {
        let summary = |answer: &str| {
            parse_paragraph_output(answer, 100).expect("a paragraph").historical_snapshot
        };
        assert_eq!(
            summary("Here is the summary:\nThe user asked for a game."),
            "The user asked for a game."
        );
        assert_eq!(summary("以下是摘要：\n用户要做网页游戏。"), "用户要做网页游戏。");
        assert_eq!(
            summary("The user asked for three things:\na game and a score."),
            "The user asked for three things: a game and a score.",
            "a line of the summary itself"
        );
        assert_eq!(
            summary("Summary: the user asked for a game."),
            "Summary: the user asked for a game."
        );
    }
}
