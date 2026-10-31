//! What the model reads of the task's
//! control state.
//!
//! The instructions come before the conversation, so anything in them that changes from one step
//! to the next makes the provider process the whole conversation again on every step. Core's
//! per-step bookkeeping — the run's own record, the checkpoint revision and hash it verifies when
//! it commits the step, the registry reference and the step's trigger — stays with Core. The model
//! reads what it acts on (the turn and its revision, lineages, todo, goal, question, mode, guard,
//! permission mode), which changes only when the task's state changes.
//!
//! The to-do changes with each update the model makes, and every to-do result shows the whole
//! to-do. So the state carries it only while the conversation does not show it as it is - once a
//! compaction or a clearing took the result that did - and not while no update has written it.
use aura_agent_domain::TodoState;
use aura_context_domain::ContextSourceRef;
use aura_context_runtime::ContextPreparationInput;
use aura_domain_common::Revision;
use aura_model_domain::LogicalConversationItem;
use aura_tool_contract::{ModelVisibleToolResultOutcomeV1, ModelVisibleToolSuccessPayloadV1};

/// Core-only fields of the control state: they change on every step and no tool takes them.
const PER_STEP: [&str; 5] =
    ["run", "state_revision", "state_hash", "registry_snapshot_ref", "continuation"];

/// The source kind of the control state's fragment in the task state section.
pub(in crate::agent_application) const CONTROL_STATE: &str = "agent.control_state.v1";

/// The tools whose result is the whole to-do.
const TODO_TOOLS: [&str; 3] = ["todo.get", "todo.replace", "todo.update"];

/// The control state as the model reads it beside the conversation the request carries.
pub(in crate::agent_application) fn prompt_state(
    state: &serde_json::Value,
    conversation: &[LogicalConversationItem],
) -> serde_json::Value {
    let mut view = state.clone();
    if let Some(fields) = view.as_object_mut() {
        for field in PER_STEP {
            fields.remove(field);
        }
        let todo: Option<TodoState> =
            fields.get("todo").and_then(|todo| serde_json::from_value(todo.clone()).ok());
        if todo.is_some_and(|todo| unwritten(&todo) || shown(&todo, conversation)) {
            fields.remove("todo");
        }
    }
    view
}

/// Writes the control state the model reads into the request being prepared, as its conversation
/// now stands: a compaction or a clearing may have taken the result that showed the to-do.
pub(in crate::agent_application) fn write_control_state(
    input: &mut ContextPreparationInput,
    state: &serde_json::Value,
) -> anyhow::Result<()> {
    let content = serde_json::to_string(&prompt_state(state, &input.recent_history_items))?;
    for fragment in input.section_inputs.iter_mut().flat_map(|section| &mut section.fragments) {
        if matches!(&fragment.source_ref, ContextSourceRef::RuntimeState { state_kind, .. } if state_kind == CONTROL_STATE)
        {
            fragment.content.clone_from(&content);
        }
    }
    Ok(())
}

/// No update has written the to-do: a turn's to-do starts empty at its first revision.
fn unwritten(todo: &TodoState) -> bool {
    todo.revision == Revision::INITIAL && todo.items.is_empty()
}

/// The newest successful to-do result the conversation carries shows the to-do as it is.
fn shown(todo: &TodoState, conversation: &[LogicalConversationItem]) -> bool {
    conversation
        .iter()
        .rev()
        .find_map(|item| match item {
            LogicalConversationItem::ToolResult { tool_name, content, is_error: false, .. }
                if TODO_TOOLS.contains(&tool_name.as_str()) =>
            {
                result_todo(content)
            }
            _ => None,
        })
        .is_some_and(|shown| shown == *todo)
}

/// The to-do a result shows; `None` for one cleared or replaced by a Hook.
fn result_todo(content: &str) -> Option<TodoState> {
    let mut view: serde_json::Value = serde_json::from_str(content).ok()?;
    let outcome: ModelVisibleToolResultOutcomeV1 =
        serde_json::from_value(view.get_mut("outcome")?.take()).ok()?;
    let ModelVisibleToolResultOutcomeV1::Succeeded {
        payload: ModelVisibleToolSuccessPayloadV1::StructuredJson { value },
    } = outcome
    else {
        return None;
    };
    serde_json::from_value(value).ok()
}

#[cfg(test)]
#[path = "prompt_state_tests.rs"]
mod tests;
