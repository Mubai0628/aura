//! A compaction answer is measured by its summary text and refused only well past its target.
use aura_context_domain::{CompactionAnchorKind, CompactionAnchorManifest, ContextSourceRef};
use aura_context_runtime::{
    CompactionSourceDraft, CompactionValidationError, build_anchor_manifest,
    compaction_output_too_large, compaction_summary_characters,
    parse_and_validate_compaction_output,
};
use aura_domain_common::{ContentHash, EventId};

const TARGET: u64 = 4_096;

fn manifest(anchors: usize) -> CompactionAnchorManifest {
    build_anchor_manifest(
        (0..anchors)
            .map(|index| CompactionSourceDraft {
                kind: CompactionAnchorKind::Event,
                source_ref: ContextSourceRef::Event { event_id: EventId::new() },
                source_hash: ContentHash::sha256(format!("event {index}").as_bytes()),
                preview: format!("event {index}"),
            })
            .collect(),
    )
    .expect("manifest")
}

/// An answer in the frozen shape whose summary sections hold `summary` characters in all.
fn answer(anchors: usize, summary: usize) -> String {
    let line = |chars: usize| "s".repeat(chars.saturating_sub(1));
    let index = (1..=anchors).map(|n| format!("- a{n:06}")).collect::<Vec<_>>().join("\n");
    let quarter = summary / 4;
    format!(
        "# Historical-Snapshot-Summary\n{}\n# Active-Turn-Progress-Summary\n{}\n# Structured-Compaction\n## Summary\n{}\n## Full-Turn-Session-Line\nline\n## Active-Turn-Segment-Line\n{}\n## Truth-Index\n{index}\n",
        line(quarter),
        line(quarter),
        line(quarter),
        line(summary - 3 * quarter - 5),
    )
}

#[test]
fn the_headings_and_the_truth_index_do_not_count() {
    let text = answer(62, 2_000);
    assert_eq!(compaction_summary_characters(&text), 2_000);
    assert!(text.chars().count() > 2_000 + 62 * 9, "the whole answer is longer");
}

#[test]
fn an_answer_is_refused_only_past_four_times_its_target() {
    assert!(!compaction_output_too_large(&answer(62, 16_384), TARGET));
    assert!(compaction_output_too_large(&answer(62, 16_385), TARGET));
    assert!(!compaction_output_too_large(&answer(0, 100), TARGET));
}

/// The four answers a real session refused (summary text 3,222–5,579 characters, Truth-Index up to
/// 730) are accepted, through the same validation a compaction runs.
#[test]
fn the_answers_a_real_session_refused_are_accepted() {
    for (anchors, summary) in [(62, 4_585), (62, 5_579), (25, 4_169), (73, 3_222)] {
        let text = answer(anchors, summary);
        let parsed = parse_and_validate_compaction_output(&text, TARGET, &manifest(anchors), true);
        assert!(parsed.is_ok(), "{anchors} anchors, {summary} characters: {parsed:?}");
    }
    let far_too_long = answer(10, 20_000);
    assert_eq!(
        parse_and_validate_compaction_output(&far_too_long, TARGET, &manifest(10), true)
            .map(|_| ()),
        Err(CompactionValidationError::OutputTooLarge)
    );
}
