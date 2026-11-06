//! Where a turn that was stopped before it finished stopped. The model is sent such a turn as it
//! was - its messages and its finished steps - and then one note, written from the turn's stop
//! fact: why it stopped, and what of its last response had not finished. A later message, 继续 or
//! anything else, reads where the turn stopped there instead of searching the history for it.
//!
//! A response whose tool calls had not all finished has no sealed group, so its calls cannot be
//! sent as calls; the note names them with what is known of each. Their results stay out of it:
//! a result is workspace data and is sent only as a tool result.
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::*;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use aura_tool_contract::ToolResultOutcomeV1;
use rusqlite::{Connection, OptionalExtension};
use std::fmt::Write as _;

/// The most of a call's arguments the note repeats.
const ARGUMENT_CHARS: usize = 400;

/// The last response of a stopped turn whose tool calls had not all finished.
pub(super) struct Unfinished {
    pub text: String,
    pub calls: Vec<UnfinishedCall>,
}

pub(super) struct UnfinishedCall {
    pub id: ToolCallId,
    pub name: String,
    pub arguments: String,
}

/// The note a stopped turn ends with, sourced from its stop fact; none for a turn that finished
/// or is still running.
pub(super) fn note(
    db: &Connection,
    turn: &TurnTask,
    unfinished: Option<Unfinished>,
) -> StoreResult<Option<LogicalConversationItem>> {
    if !matches!(turn.state, TurnTaskState::Cancelled | TurnTaskState::RecoveryRequired { .. }) {
        return Ok(None);
    }
    let Some((event, payload)) = db
        .query_row(
            "SELECT event_id, payload FROM events WHERE owner=?1 AND kind='agent.stopped' ORDER BY sequence LIMIT 1",
            [turn.agent_run_id.to_string()],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let (reason, cancelled): (String, bool) = record::decode(&payload)?;
    let mut content = String::new();
    if let Some(unfinished) = &unfinished
        && !unfinished.text.is_empty()
    {
        content.push_str(&unfinished.text);
        content.push_str("\n\n");
    }
    content.push('[');
    content.push_str(&why(&reason, cancelled));
    match unfinished.filter(|u| !u.calls.is_empty()) {
        None => content.push_str(
            " Its messages and finished steps are above as they were; nothing else ran, so the history holds nothing more about it.",
        ),
        Some(unfinished) => {
            content.push_str(" The tool calls of its last response had not all finished:");
            for call in &unfinished.calls {
                let _ = write!(
                    content,
                    "\n- {} {} (tool_call_id {}): {}",
                    call.name,
                    shortened(&call.arguments),
                    call.id,
                    state(db, call.id)
                );
            }
            content.push_str("\nEverything else this turn did is above as it was.");
        }
    }
    content.push(']');
    Ok(Some(LogicalConversationItem::Assistant {
        provider_history: None,
        source_event_id: event.parse().map_err(|_| Error::Conflict)?,
        content_hash: Some(ContentHash::sha256(content.as_bytes())),
        content: Some(content),
        tool_calls: Vec::new(),
    }))
}

fn why(reason: &str, cancelled: bool) -> String {
    match reason {
        "agent.core_restarted" if !cancelled => {
            "This turn was cut off here: Aura restarted while it was running.".into()
        }
        "agent.cancelled" | "agent.core_restarted" => "The user stopped this turn here.".into(),
        "agent.mode_paused" => "This turn stopped here: the user paused its mode.".into(),
        "agent.mode_terminated" => "This turn stopped here: the user ended its mode.".into(),
        other => format!("This turn stopped here ({other})."),
    }
}

/// What is known of a call of the unfinished response. A call with no record was never admitted.
fn state(db: &Connection, id: ToolCallId) -> &'static str {
    match crate::tool::execution::call(db, id) {
        Ok(call) => match call.result.map(|result| result.outcome) {
            Some(ToolResultOutcomeV1::Succeeded) => {
                "it finished; context.read_tool_result reads its result."
            }
            Some(ToolResultOutcomeV1::NotExecuted { .. }) => "it did not run.",
            Some(ToolResultOutcomeV1::Cancelled { .. }) => {
                "it was cancelled before it finished; context.read_tool_result reads what it did."
            }
            Some(_) => "it finished with an error; context.read_tool_result reads it.",
            None if call.attempt_id.is_none() => "it did not run.",
            None => {
                "it started, but its result was lost: it may or may not have taken effect, so check before doing it again."
            }
        },
        Err(Error::Sqlite(rusqlite::Error::QueryReturnedNoRows)) => "it did not run.",
        Err(_) => "what became of it is not known: check before doing it again.",
    }
}

fn shortened(arguments: &str) -> String {
    match arguments.char_indices().nth(ARGUMENT_CHARS) {
        Some((end, _)) => format!("{}…", &arguments[..end]),
        None => arguments.to_owned(),
    }
}
