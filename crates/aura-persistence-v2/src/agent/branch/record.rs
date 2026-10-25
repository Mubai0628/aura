//! What a branch keeps of the conversation it was made from. Everything here is written once,
//! when the branch is made, and owned by the branch: deleting either conversation leaves the
//! other whole.
use aura_agent_domain::{AgentBranchOrigin, AgentMessageTextSource};
use aura_context_domain::{ContextReadEventView, HistoryAtomicGroupPreimageV1};
use aura_domain_common::*;
use aura_model_domain::{LogicalConversationItem, ProviderToolCallWireBinding};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact, record};
use aura_projection_contract::AgentTurnView;
use aura_tool_projection::AgentToolActivityEntry;
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

pub(super) const KIND: &str = "agent.session_branch";
pub(super) const USER_KIND: &str = "agent.imported_user_message";
const MEDIA: &str = "application/vnd.aura.branch+json";
const TEXT: &str = "text/plain; charset=utf-8";

/// The owner of a branch's facts: its record and the user messages it carried.
pub(super) fn owner(session: SessionId) -> String {
    format!("agent-branch:{session}")
}

/// The branch itself: where it came from, and where each carried turn is kept.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct BranchRecord {
    pub session_id: SessionId,
    pub command_invocation_id: CommandInvocationId,
    pub origin: AgentBranchOrigin,
    /// The carried turns, oldest first.
    pub turns: Vec<CarriedTurn>,
    /// The compactions made during the carried turns, copied as this branch's own.
    pub compactions: Vec<CarriedCompaction>,
    /// Where each carried text, result and tool call is kept.
    pub index_artifact_id: ArtifactId,
    /// Set while the older turns are still being carried after the branch
    /// opened with its newest ones. `turns` is then the newest part of the list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub copying: Option<BranchCopying>,
}

/// What a branch still has to carry, and how its history is put in order at the end.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct BranchCopying {
    /// The turns still to carry, oldest first. The next step carries the last of them.
    pub remaining: Vec<PendingTurn>,
    /// How many of all the turns come from the source's own carried turns (they come first).
    pub shared: usize,
    /// The source's own turns the branch carries, oldest first.
    pub own: Vec<TurnTaskId>,
    /// Why the copy stopped, when it could not finish.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct PendingTurn {
    pub turn_task_id: TurnTaskId,
    /// A turn the source itself carried: its artifacts are shared, not captured again.
    pub shared: bool,
}

impl BranchRecord {
    /// The place in the whole list of the first turn carried so far.
    pub(super) fn first(&self) -> usize {
        self.copying.as_ref().map_or(0, |copying| copying.remaining.len())
    }

    /// The carried turn at this place in the whole list, when it has been carried.
    pub(super) fn turn_at(&self, place: usize) -> Option<&CarriedTurn> {
        self.turns.get(place.checked_sub(self.first())?)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct CarriedTurn {
    pub turn_task_id: TurnTaskId,
    pub root_user_event_id: EventId,
    /// What the model was sent for this turn ([`CarriedModel`]).
    pub model_artifact_id: ArtifactId,
    /// What the conversation showed for this turn ([`CarriedDisplay`]).
    pub display_artifact_id: ArtifactId,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct CarriedCompaction {
    /// This branch's copy.
    pub compaction_id: ContextProjectionSnapshotId,
    /// The compaction it copies, as the model was told it in the summary.
    pub original_id: ContextProjectionSnapshotId,
    /// The carried turn during which it was made.
    pub turn: usize,
}

/// One carried turn as the model was sent it.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(super) struct CarriedModel {
    pub items: Vec<LogicalConversationItem>,
    /// Each item's place in the conversation, in the order the items were written.
    pub sequences: Vec<u64>,
    pub wire_bindings: Vec<ProviderToolCallWireBinding>,
    pub groups: Vec<HistoryAtomicGroupPreimageV1>,
    /// What `context.read_events` reads of this turn.
    pub rows: Vec<CarriedRow>,
    /// What `context.read_tool_result` reads of this turn's tool calls.
    pub tool_results: Vec<CarriedToolResult>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CarriedRow {
    pub turn: Option<TurnTaskId>,
    pub view: ContextReadEventView,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct CarriedToolResult {
    pub tool_call_id: ToolCallId,
    pub tool_name: String,
    pub input: Option<String>,
    pub result: String,
    pub is_error: bool,
}

/// One carried turn as the conversation showed it.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CarriedDisplay {
    /// Every message of the turn; pages are cut from it as the turn's own pages were.
    pub view: AgentTurnView,
    pub activity: Vec<AgentToolActivityEntry>,
}

/// Where the carried texts and results are, by what the conversation asks for.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct CarriedIndex {
    pub bodies: Vec<CarriedBody>,
    pub results: Vec<CarriedResult>,
    /// Each carried tool call and the turn it belongs to.
    pub calls: Vec<(ToolCallId, usize)>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct CarriedBody {
    pub turn: usize,
    pub source: AgentMessageTextSource,
    pub artifact_id: ArtifactId,
    pub total_bytes: u64,
    pub sealed_revision: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct CarriedResult {
    pub turn: usize,
    pub tool_call_id: ToolCallId,
    pub artifact_id: ArtifactId,
    pub total_bytes: u64,
}

/// A user message the branch carried, kept where the model egress check finds it by its event.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct CarriedUserMessage {
    pub event_id: EventId,
    pub text: String,
}

pub(super) fn load(db: &Connection, session: SessionId) -> StoreResult<Option<BranchRecord>> {
    let raw: Option<String> = db
        .query_row(
            "SELECT payload FROM events WHERE owner=?1 AND kind=?2 AND version=1 \
             ORDER BY sequence DESC LIMIT 1",
            params![owner(session), KIND],
            |row| row.get(0),
        )
        .optional()?;
    let Some(raw) = raw else { return Ok(None) };
    let value: BranchRecord = record::decode(&raw)?;
    if value.session_id != session {
        return Err(Error::Conflict);
    }
    Ok(Some(value))
}

/// Writes the branch's record; the newest one is the branch.
pub(super) fn save(
    tx: &Transaction<'_>,
    value: &BranchRecord,
    at: UtcTimestamp,
) -> StoreResult<()> {
    crate::agent::facts::append(tx, EventId::new(), &owner(value.session_id), KIND, value, at)
}

/// Writes a carried value as an artifact the branch owns.
pub(super) fn put_json<T: Serialize>(
    tx: &Transaction<'_>,
    session: SessionId,
    value: &T,
    at: UtcTimestamp,
) -> StoreResult<ArtifactId> {
    let id = ArtifactId::new();
    artifact::put(tx, id, &session.to_string(), MEDIA, &serde_json::to_vec(value)?, at)?;
    Ok(id)
}

pub(super) fn put_text(
    tx: &Transaction<'_>,
    session: SessionId,
    text: &str,
    at: UtcTimestamp,
) -> StoreResult<ArtifactId> {
    let id = ArtifactId::new();
    artifact::put(tx, id, &session.to_string(), TEXT, text.as_bytes(), at)?;
    Ok(id)
}

pub(super) fn read_json<T: DeserializeOwned>(
    db: &Connection,
    session: SessionId,
    id: ArtifactId,
) -> StoreResult<T> {
    Ok(serde_json::from_slice(&artifact::read(db, id, &session.to_string())?)?)
}

pub(super) fn read_text(
    db: &Connection,
    session: SessionId,
    id: ArtifactId,
) -> StoreResult<String> {
    String::from_utf8(artifact::read(db, id, &session.to_string())?).map_err(|_| Error::Conflict)
}

/// Makes `to` an owner of an artifact `from` owns, so a branch made from a branch keeps what it
/// carried after the other is deleted. The bytes are not copied.
pub(super) fn share(
    tx: &Transaction<'_>,
    id: ArtifactId,
    from: SessionId,
    to: SessionId,
) -> StoreResult<()> {
    let shared = tx.execute(
        "INSERT INTO artifact_owners SELECT artifact_id, ?3 FROM artifact_owners \
         WHERE artifact_id=?1 AND owner=?2 ON CONFLICT DO NOTHING",
        params![id.to_string(), from.to_string(), to.to_string()],
    )?;
    if shared == 0 {
        let owned: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM artifact_owners WHERE artifact_id=?1 AND owner=?2)",
            params![id.to_string(), to.to_string()],
            |row| row.get(0),
        )?;
        if !owned {
            return Err(Error::Conflict);
        }
    }
    Ok(())
}
