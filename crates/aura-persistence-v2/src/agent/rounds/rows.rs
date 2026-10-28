//! A round's messages and responses, in order: the user's messages, and each model response with
//! its thinking, its reply and its tool calls with their results.
use super::turns::Round;
use crate::agent::branch::{CarriedRow, CarriedToolResult};
use aura_domain_common::{EventId, SessionId, ToolCallId, UtcTimestamp};
use aura_model_domain::{AssistantContentBlock, NormalizedModelOutput, NormalizedToolCall};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, record};
use rusqlite::{Connection, params};
use std::collections::BTreeMap;

/// A place in the conversation: a branch's carried history (0) comes before its own (1), each in
/// the order of its sequence.
pub(super) type Position = (u8, u64);

pub(super) struct Row {
    pub at: UtcTimestamp,
    pub body: Body,
}

pub(super) enum Body {
    User(String),
    Model { thinking: String, reply: String, calls: Vec<Call> },
}

pub(super) struct Call {
    pub tool: String,
    pub arguments: String,
    /// What the model was given back; `None` while the call has no result.
    pub result: Option<String>,
}

/// Reads rounds' rows; what a branch carried is read once.
pub(super) struct Reader<'a> {
    db: &'a Connection,
    session: SessionId,
    carried: Option<Carried>,
}

struct Carried {
    rows: Vec<CarriedRow>,
    results: BTreeMap<ToolCallId, CarriedToolResult>,
    times: BTreeMap<EventId, UtcTimestamp>,
}

impl<'a> Reader<'a> {
    pub(super) fn new(db: &'a Connection, session: SessionId) -> Self {
        Self { db, session, carried: None }
    }

    pub(super) fn rows(&mut self, round: &Round) -> StoreResult<Vec<Row>> {
        let mut rows = Vec::new();
        if round.start.0 == 0 {
            let end = round.end.filter(|end| end.0 == 0).map(|end| end.1);
            rows.extend(self.carried_rows(round.start.1, end)?);
        }
        if round.end.is_none_or(|end| end.0 == 1) {
            let start = if round.start.0 == 1 { round.start.1 } else { 0 };
            rows.extend(self.own_rows(start, round.end.map(|end| end.1))?);
        }
        Ok(rows)
    }

    fn carried_rows(&mut self, start: u64, end: Option<u64>) -> StoreResult<Vec<Row>> {
        if self.carried.is_none() {
            self.carried = Some(Carried {
                rows: crate::agent::branch::carried_rows(self.db, self.session)?,
                results: crate::agent::branch::carried_tool_results(self.db, self.session)?,
                times: crate::agent::branch::carried_user_times(self.db, self.session)?,
            });
        }
        let carried = self.carried.as_ref().ok_or(Error::Conflict)?;
        let mut rows = Vec::new();
        let mut last_at = None;
        for row in &carried.rows {
            let sequence = row.view.session_sequence.value();
            if sequence < start || end.is_some_and(|end| sequence >= end) {
                continue;
            }
            if row.view.actor == "user" {
                let at = carried.times.get(&row.view.event_id).copied().ok_or(Error::Conflict)?;
                last_at = Some(at);
                rows.push(Row { at, body: Body::User(row.view.content.clone()) });
                continue;
            }
            // A carried response keeps no time of its own: it is placed at the message before it.
            let at = last_at.ok_or(Error::Conflict)?;
            let content: CarriedResponse = serde_json::from_str(&row.view.content)?;
            let calls = content
                .tool_calls
                .iter()
                .map(|call| match carried.results.get(&call.tool_call_id) {
                    Some(result) => Call {
                        tool: result.tool_name.clone(),
                        arguments: result
                            .input
                            .clone()
                            .unwrap_or_else(|| call.arguments_json.clone()),
                        result: Some(result.result.clone()),
                    },
                    None => Call {
                        tool: call.tool_name.clone(),
                        arguments: call.arguments_json.clone(),
                        result: None,
                    },
                })
                .collect();
            rows.push(Row {
                at,
                body: Body::Model {
                    thinking: String::new(),
                    reply: reply(
                        &content.assistant_blocks,
                        content.refusal.as_ref().map(|refusal| refusal.message.as_str()),
                    ),
                    calls,
                },
            });
        }
        Ok(rows)
    }

    fn own_rows(&self, start: u64, end: Option<u64>) -> StoreResult<Vec<Row>> {
        let end = end.map_or(i64::MAX, |end| i64::try_from(end).unwrap_or(i64::MAX));
        let start = i64::try_from(start).map_err(|_| Error::Conflict)?;
        // The user's messages are one owner's events and the responses those of the session's
        // top-level runs, both read through the owner index.
        let mut query = self.db.prepare(
            "SELECT * FROM ( \
             SELECT e.sequence,e.event_id,e.kind,e.payload,e.created_at FROM events e \
             WHERE e.owner=?2 AND e.kind='agent.user_message' AND e.version=1 \
             AND e.sequence>=?3 AND e.sequence<?4 \
             UNION ALL \
             SELECT e.sequence,e.event_id,e.kind,e.payload,e.created_at FROM turn_tasks t \
             CROSS JOIN agent_runs ar CROSS JOIN events e \
             WHERE t.session_id=?1 AND ar.turn_task_id=t.id AND ar.parent_id IS NULL \
             AND e.owner=ar.id AND e.kind='agent.assistant_response' AND e.version=1 \
             AND e.sequence>=?3 AND e.sequence<?4) ORDER BY 1",
        )?;
        let raw = query
            .query_map(
                params![
                    self.session.to_string(),
                    format!("agent-user:{}", self.session),
                    start,
                    end
                ],
                |r| {
                    Ok((
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )?
            .collect::<Result<Vec<_>, _>>()?;
        let mut rows = Vec::new();
        for (event_id, kind, payload, created_at) in raw {
            let event_id: EventId = event_id.parse().map_err(|_| Error::Conflict)?;
            if kind == "agent.user_message" {
                let message = crate::agent::session::user_message(self.db, self.session, event_id)?;
                rows.push(Row { at: message.at, body: Body::User(message.text) });
                continue;
            }
            let at: UtcTimestamp = created_at.parse().map_err(|_| Error::Conflict)?;
            let output: NormalizedModelOutput = record::decode(&payload)?;
            let thinking = output
                .reasoning_blocks
                .iter()
                .map(|block| block.text.as_str())
                .filter(|text| !text.trim().is_empty())
                .collect::<Vec<_>>()
                .join("\n\n");
            let calls = output
                .tool_calls
                .iter()
                .map(|call| self.call(call))
                .collect::<StoreResult<Vec<_>>>()?;
            rows.push(Row {
                at,
                body: Body::Model {
                    thinking,
                    reply: reply(
                        &output.assistant_blocks,
                        output.refusal.as_ref().map(|refusal| refusal.message.as_str()),
                    ),
                    calls,
                },
            });
        }
        Ok(rows)
    }

    /// A call of the session's own, with what it gave back once it has a result.
    fn call(&self, call: &NormalizedToolCall) -> StoreResult<Call> {
        let recorded: Option<String> = self
            .db
            .query_row(
                "SELECT c.id FROM tool_calls c JOIN tool_batches b ON b.id=c.batch_id \
                 WHERE c.id=?1 AND b.session_id=?2",
                params![call.tool_call_id.to_string(), self.session.to_string()],
                |r| r.get(0),
            )
            .ok();
        let finished = recorded.is_some()
            && crate::tool::execution::call(self.db, call.tool_call_id)?.result.is_some();
        if !finished {
            return Ok(Call {
                tool: call.tool_name.clone(),
                arguments: call.arguments_json.clone(),
                result: None,
            });
        }
        let whole =
            crate::context::read::whole_tool_result(self.db, self.session, call.tool_call_id)?;
        Ok(Call {
            tool: whole.tool_name,
            arguments: whole.input.unwrap_or_else(|| call.arguments_json.clone()),
            result: Some(whole.result),
        })
    }
}

/// The response as a branch carried it.
#[derive(serde::Deserialize)]
struct CarriedResponse {
    #[serde(default)]
    assistant_blocks: Vec<AssistantContentBlock>,
    #[serde(default)]
    tool_calls: Vec<NormalizedToolCall>,
    #[serde(default)]
    refusal: Option<aura_model_domain::ModelRefusal>,
}

fn reply(blocks: &[AssistantContentBlock], refusal: Option<&str>) -> String {
    let mut text = blocks
        .iter()
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_owned();
    if let Some(refusal) = refusal {
        if !text.is_empty() {
            text.push_str(
                "

",
            );
        }
        text.push_str(refusal);
    }
    text
}
