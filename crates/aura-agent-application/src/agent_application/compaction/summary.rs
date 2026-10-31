//! What a compaction leaves in the context: its paragraph, and the rounds that were the latest
//! when it was written. The rounds are those of that moment, so the section changes only at the
//! next compaction and the provider's cache of what follows it holds; rounds begun since are in the
//! conversation as they were.
use super::*;

/// The summary's length the model is asked for, in characters.
pub(super) const SUMMARY_CHARS: u64 = 3_000;
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

/// Core's own summary when the model gave none: the earlier summary, and where the rest went.
pub(super) fn fallback(earlier: Option<&str>) -> String {
    let moved = "Earlier messages of this conversation were moved out of the context without a new summary; find them with history.list_turns, history.search and history.read.";
    match earlier.map(str::trim).filter(|earlier| !earlier.is_empty()) {
        Some(earlier) => format!("{earlier} {moved}"),
        None => moved.to_owned(),
    }
}

impl AgentApplication {
    /// Completes a compaction whose model attempts failed with Core's own summary.
    pub(super) async fn complete_by_core(
        self: &Arc<Self>,
        record: &store::Compaction,
        text: String,
    ) -> anyhow::Result<()> {
        let result = ContextReadCompactionResult {
            source_context_snapshot_id: record.id,
            sequence: record.sequence,
            historical_snapshot: text,
            active_turn_summary: String::new(),
            structured_compaction: String::new(),
            source_span: record.source_span.clone(),
            anchor_manifest: record.anchors.clone(),
        };
        let journal = Arc::clone(&self.execution.journal);
        let id = record.id;
        // Boundary: Core's summary is one bounded write.
        self.store
            .run_critical(
                StoreStep::run(record.agent_run_id, "agent.compaction.complete_by_core"),
                StoreAccess::Write,
                move || {
                    Ok(journal.lock()?.database.unit_of_work(|tx| {
                        store::complete_by_core(tx, id, result, UtcTimestamp::now())
                    })?)
                },
            )
            .await
    }
}

/// `2026-10-27 05:07Z`.
fn minute(at: UtcTimestamp) -> String {
    let text = at.to_string();
    match (text.get(..10), text.get(11..16)) {
        (Some(day), Some(time)) => format!("{day} {time}Z"),
        _ => text,
    }
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
        assert!(fallback(Some("此前的总结。")).starts_with("此前的总结。 Earlier messages"));
        assert!(fallback(None).contains("history.search"));
    }

    fn section_of_old() -> String {
        let mut old = result("历史", "进展");
        old.structured_compaction = "## Truth-Index\n- a000001".into();
        section(&old, &[])
    }
}
