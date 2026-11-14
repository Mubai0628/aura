//! What a sub-agent reads in its next turn about who stopped its last one and why, when it was
//! not the user stopping it from the 多 Agent page. Each clause follows "This turn stopped here: "
//! in the model's English note on the cut-off turn.
use crate::StopNote;
use std::fmt::Display;

/// The main agent stopped it with agents.stop.
pub(super) fn by_main_agent() -> StopNote {
    note("the main agent stopped it with agents.stop", None)
}

/// The user stopped the main agent's turn, which stops the sub-agents it started.
pub(super) fn with_main_turn() -> StopNote {
    note("the user stopped the main agent's turn, and with it the sub-agents it had started", None)
}

/// The main agent discarded the sub-agent's worktree, which it cannot work without.
pub(super) fn worktree_discarded() -> StopNote {
    note("the main agent discarded this sub-agent's worktree with agents.discard", None)
}

/// Aura's watchdog stopped a turn that ran past the time a task may take.
pub(super) fn over_time(minutes: impl Display) -> StopNote {
    note(
        format!(
            "Aura's watchdog stopped it after more than {minutes} minutes of work without a final answer (time spent waiting for approval is not counted)"
        ),
        Some(
            "If the main agent asks you to go on, report what is done and what remains with agent.report before that much time passes again.",
        ),
    )
}

/// Aura's watchdog stopped a turn that made no progress for a while.
pub(super) fn no_progress(minutes: impl Display) -> StopNote {
    note(
        format!(
            "Aura's watchdog stopped it after {minutes} minutes with no progress (no model answer and no tool call running)"
        ),
        None,
    )
}

/// Aura's watchdog stopped a sub-agent whose answers used up its token budget.
pub(super) fn budget_used(used: impl Display, budget: impl Display) -> StopNote {
    note(
        format!(
            "Aura's watchdog stopped it because its token budget ran out (used {used} of {budget})"
        ),
        None,
    )
}

fn note(clause: impl Into<String>, next: Option<&str>) -> StopNote {
    StopNote { clause: clause.into(), next: next.map(str::to_owned) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_note_reads_after_stopped_here_as_one_clause() {
        for note in [
            by_main_agent(),
            with_main_turn(),
            worktree_discarded(),
            over_time(30),
            no_progress(15),
            budget_used(120_000, 100_000),
        ] {
            let first = note.clause.chars().next().unwrap_or_default();
            assert!(first.is_lowercase() || note.clause.starts_with("Aura's"), "{note:?}");
            assert!(!note.clause.ends_with('.') && !note.clause.contains('\n'), "{note:?}");
            assert!(note.next.as_deref().is_none_or(|next| next.ends_with('.')), "{note:?}");
        }
        assert!(over_time(30).clause.contains("30 minutes"));
        assert!(budget_used(7, 5).clause.contains("used 7 of 5"));
    }
}
