//! What a compaction leaves in the context: its paragraph, and the rounds that were the latest
//! when it was written. The rounds are those of that moment, so the section changes only at the
//! next compaction and the provider's cache of what follows it holds; rounds begun since are in the
//! conversation as they were.
use super::*;

/// The latest rounds the section names.
pub(super) const RECENT_ROUNDS: usize = 6;

/// A stored summary's text: the paragraph, or an older summary's two parts.
pub(super) fn text(result: &ContextReadCompactionResult) -> String {
    [result.historical_snapshot.trim(), result.active_turn_summary.trim()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The section's content.
pub(super) fn section(result: &ContextReadCompactionResult, recent: &[HistoryTurnLine]) -> String {
    let mut section = format!(
        "Earlier in this conversation, summarized (find the original words with history.search and history.read):\n{}",
        text(result)
    );
    if !recent.is_empty() {
        section.push_str(
            "\n\nThe latest rounds when it was summarized (a round is one task, from the user's request until it was completed; T5.1 is round 5's first message):",
        );
        for round in recent {
            section.push_str(&format!(
                "\n{} · {} · {}",
                round.id,
                minute(round.sent_at),
                round.first_message
            ));
            if round.first_message.chars().count() as u64 != round.first_message_chars {
                section.push_str(&format!(" ({} characters)", round.first_message_chars));
            }
        }
    }
    section
}

/// `2026-10-27 05:07Z`.
fn minute(at: UtcTimestamp) -> String {
    let text = at.to_string();
    match (text.get(..10), text.get(11..16)) {
        (Some(day), Some(time)) => format!("{day} {time}Z"),
        _ => text,
    }
}

/// Older active runs keep their normal request prefix. Upgrade only the reviewed auxiliary
/// instruction that describes the retired single-paragraph format.
pub(super) fn instructions(
    frozen: &aura_prompt_runtime::ValidatedPromptBundle,
) -> anyhow::Result<String> {
    let mut content = Vec::new();
    for module in frozen.modules.iter().filter(|m| m.module_id.starts_with("compaction.")) {
        let major = module.module_version.as_str().split('.').next();
        if module.module_id == "compaction.system" && matches!(major, Some("1" | "2")) {
            let current = aura_prompt_runtime::ValidatedPromptBundle::embedded()?;
            let reviewed = current
                .modules
                .into_iter()
                .find(|m| m.module_id == module.module_id)
                .ok_or_else(|| anyhow::anyhow!("context.compaction_prompt_missing"))?;
            content.push(reviewed.normalized_content);
        } else {
            content.push(module.normalized_content.clone());
        }
    }
    anyhow::ensure!(!content.is_empty(), "context.compaction_prompt_missing");
    Ok(content.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(historical: &str, active: &str) -> ContextReadCompactionResult {
        ContextReadCompactionResult {
            source_context_snapshot_id: ContextProjectionSnapshotId::new(),
            sequence: 1,
            historical_snapshot: historical.into(),
            active_turn_summary: active.into(),
            structured_compaction: String::new(),
            source_span: CompactionSourceSpan {
                start_sequence: SessionSequence::new(1),
                end_sequence: SessionSequence::new(2),
                source_refs: vec![],
            },
            anchor_manifest: CompactionAnchorManifest {
                anchors: vec![],
                manifest_hash: ContentHash::sha256(b"[]"),
            },
        }
    }

    #[test]
    fn the_section_holds_the_paragraph_and_the_latest_rounds_of_its_time() {
        let line = HistoryTurnLine {
            id: "T5".into(),
            sent_at: "2026-10-27T05:07:31Z".parse().expect("time"),
            first_message: "做一个网页游戏…".into(),
            first_message_chars: 1_234,
        };
        let section = section(&result("用户要一个网页游戏。", ""), &[line]);
        assert!(section.contains("用户要一个网页游戏。"), "{section}");
        assert!(section.contains("T5 · 2026-10-27 05:07Z · 做一个网页游戏… (1234 characters)"));
        assert!(section.contains("history.search") && section.contains("history.read"));
        let older = section_of_old();
        assert!(older.contains("历史") && older.contains("进展") && !older.contains("Truth-Index"));
    }

    fn section_of_old() -> String {
        let mut old = result("历史", "进展");
        old.structured_compaction = "## Truth-Index\n- a000001".into();
        section(&old, &[])
    }
}

#[cfg(test)]
mod prompt_compatibility {
    #[test]
    fn compaction_of_an_old_active_run_uses_the_current_auxiliary_instruction_only() {
        let current =
            aura_prompt_runtime::ValidatedPromptBundle::embedded().expect("reviewed bundle");
        let mut legacy = current.clone();
        let old = legacy
            .modules
            .iter_mut()
            .find(|m| m.module_id == "compaction.system")
            .expect("summary module");
        old.module_version =
            aura_domain_common::CanonicalSemVer::new("2.2.0").expect("legacy version");
        old.normalized_content = "Write one paragraph".into();
        assert_eq!(
            super::instructions(&legacy).expect("upgraded suffix"),
            super::instructions(&current).expect("current suffix")
        );
        assert!(legacy.modules.iter().any(|m| m.normalized_content == "Write one paragraph"));
    }
}
