//! What the model reads of the task's
//! control state.
//!
//! The instructions come before the conversation, so anything in them that changes from one step
//! to the next makes the provider process the whole conversation again on every step. Core's
//! per-step bookkeeping — the run's own record, the checkpoint revision and hash it verifies when
//! it commits the step, the registry reference and the step's trigger — stays with Core. The model
//! reads what it acts on (the turn and its revision, lineages, todo, goal, question, mode, guard,
//! permission mode), which changes only when the task's state changes.

/// Core-only fields of the control state: they change on every step and no tool takes them.
const PER_STEP: [&str; 5] =
    ["run", "state_revision", "state_hash", "registry_snapshot_ref", "continuation"];

/// The control state as the model reads it.
pub(in crate::agent_application) fn prompt_state(state: &serde_json::Value) -> serde_json::Value {
    let mut view = state.clone();
    if let Some(fields) = view.as_object_mut() {
        for field in PER_STEP {
            fields.remove(field);
        }
    }
    view
}

#[cfg(test)]
mod tests {
    use super::prompt_state;

    #[test]
    fn the_model_reads_no_per_step_bookkeeping_and_core_keeps_it() {
        let state = serde_json::json!({
            "turn": {"revision": "3"}, "run": {"revision": "9"}, "state_revision": "12",
            "state_hash": "h", "registry_snapshot_ref": {}, "continuation": {"trigger": "tool_batch"},
            "todo": null, "permission_mode": "read_only"
        });
        assert_eq!(
            prompt_state(&state),
            serde_json::json!({"turn": {"revision": "3"}, "todo": null, "permission_mode": "read_only"})
        );
        assert_eq!(state["state_hash"], "h", "the commit check still has what it verifies");
    }
}
