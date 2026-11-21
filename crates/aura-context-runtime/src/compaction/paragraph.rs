//! Extract the background while preserving its Markdown layout and printable controls.
use super::{CompactionValidationError, ParsedCompactionOutput};

#[cfg(test)]
use aura_context_domain::has_complete_summary;

/// The paragraph in the answer, or why the answer cannot stand for the history.
pub fn parse_paragraph_output(
    output: &str,
    _suggested_length: u64,
) -> Result<ParsedCompactionOutput, CompactionValidationError> {
    let normalized = output.replace("\r\n", "\n").replace('\r', "\n");
    let paragraph = crate::section::printable_context_text(
        aura_context_domain::background_text(&normalized).trim(),
    );
    if paragraph.is_empty() {
        return Err(CompactionValidationError::OutputEmpty);
    }
    Ok(ParsedCompactionOutput {
        historical_snapshot: paragraph.into_owned(),
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
    fn a_long_background_retains_its_sections_beyond_the_suggested_length() {
        let body =
            format!("## Confirmed\n\n{}\n\n## Pending\n\nRead the receipt.", "fact ".repeat(500));
        let parsed = parse_paragraph_output(&body, 100).expect("a soft target");
        assert_eq!(parsed.historical_snapshot, body);
    }

    #[test]
    fn an_accepted_summary_has_printable_text_for_the_next_context() {
        let raw = "<summary>kept\u{1b}[32m \0 details</summary>";
        let parsed = parse_paragraph_output(raw, 100).expect("usable summary");
        assert_eq!(parsed.historical_snapshot, r"kept\u001b[32m \u0000 details");
    }

    #[test]
    fn empty_background_is_refused() {
        assert_eq!(parse_paragraph_output("  ", 100), Err(CompactionValidationError::OutputEmpty));
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
            "用户要做网页游戏。\n下一步：计分。"
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
            "The user asked for three things:\na game and a score.",
            "a line of the summary itself"
        );
        assert_eq!(
            summary("Summary: the user asked for a game."),
            "Summary: the user asked for a game."
        );
    }
}
