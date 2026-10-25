//! What an external mutation did, as a fact on disk instead of a guess from a reply.
//!
//! An external object is not in the workspace index, so the workspace's own evidence — "the
//! journal is empty and the inspection before and after are equal, therefore nothing
//! happened" — says nothing about it. Reading that as *no effect* told the model the
//! opposite of the truth whenever the write really happened, and reading it as *unknown*
//! for every failure threw away facts the caller does have.
//!
//! The stages are recorded as they really occur: `prepared` before the publication is
//! attempted, then exactly one outcome — `published`, `compensated` (the platform reported a
//! rollback, so the previous content is back), `not_performed` (the call was refused before
//! anything could be published) or `unknown` (the publication was attempted and could not be
//! confirmed: a failed rollback, or a reply that never made it back into the record).
//!
//! Loading returns the latest stage per call, so a crash between the stages leaves
//! `prepared` — an unconfirmed mutation, never "no effect". Nothing here replays anything:
//! an `unknown` fact is what stops an automatic retry from acting twice.
//!
//! Three properties are deliberately enforced here rather than left to each caller:
//!
//! * a call's fact is found by **that call's identity**, not by scanning a recent window of
//!   unrelated events. A fact that exists is never reported as "no fact", because "no fact on
//!   a call that really ran" is read as an unconfirmed side effect — the expensive answer;
//! * a call has **exactly one terminal outcome**. A second, different terminal result is
//!   refused instead of rewriting history, so a published change cannot become
//!   `not_performed` after the fact;
//! * the observations a fact carries are **what was really seen**, and a row written before
//!   those observations existed loads with them absent rather than with a value invented from
//!   the call's own request.
use aura_domain_common::{
    EventId, ExternalGrantId, SecurityOperationId, ToolBatchId, ToolCallId, UtcTimestamp,
};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, event};
use rusqlite::{Connection, Transaction, params};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const KIND: &str = "tool.external_change";
const VERSION: u32 = 1;

/// How far an external mutation got.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExternalChangeStage {
    /// The publication is about to be attempted. A record left at this stage means the
    /// outcome is not known.
    Prepared,
    /// The change is on disk and was re-verified.
    Published,
    /// The publication did not commit and the platform restored the previous content.
    Compensated,
    /// The call was refused before anything could be published.
    NotPerformed,
    /// The publication was attempted and cannot be confirmed.
    Unknown,
}

impl ExternalChangeStage {
    /// Whether this stage is an outcome a call may not have a second of.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        !matches!(self, Self::Prepared)
    }

    /// Whether this stage leaves the destination in doubt, so a later check may settle it.
    ///
    /// `prepared` means the outcome never came back and `unknown` means it could not be
    /// confirmed; both are open questions about the same object. `not_performed` and
    /// `compensated` are answers, and `published` is the answer this fact exists to protect.
    #[must_use]
    pub const fn is_unconfirmed(self) -> bool {
        matches!(self, Self::Prepared | Self::Unknown)
    }
}

/// The authorization an external mutation acted under.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum ExternalChangeAuthority {
    /// A session grant covered the target.
    Grant { grant_id: ExternalGrantId },
    /// The session policy allowed the path directly.
    Policy,
}

/// One stage of one external mutation.
///
/// The observations are separate fields because they are separate facts and they do not
/// always agree: the caller's `expected` state is what the call *required*, while
/// `observed_*` is what the file layer really saw when it re-checked the target under the
/// publication lock. A grant-path call may not know any identity at all, so both identity
/// fields stay `None` there rather than being filled in from the path spelling.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalChangeFact {
    pub call_id: ToolCallId,
    /// The operation this call belongs to, so the fact can be tied to the same execution
    /// identity the attempt, the result and the audit trail use. Absent only on rows written
    /// before this field existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<SecurityOperationId>,
    pub stage: ExternalChangeStage,
    /// The absolute path as the user sees it.
    pub display_path: String,
    /// What the target held before, when that was known.
    pub previous_hash: Option<String>,
    /// What the target holds now, for a published change.
    pub resulting_hash: Option<String>,
    /// The platform identity of the object the change acted on, as canonical JSON.
    pub identity: Option<String>,
    /// The hash the file layer really observed on the target before publishing, when it
    /// re-checked it. This is an observation, not the caller's expected value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_previous_hash: Option<String>,
    /// The identity the file layer really observed on the destination after publishing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_identity: Option<String>,
    pub authority: ExternalChangeAuthority,
    pub reason: Option<String>,
    pub at: UtcTimestamp,
}

/// Whether two facts make the same claim about what happened to the object.
///
/// The moment of the write and the observations the writer made are deliberately left out:
/// a retried bookkeeping write happens at a different moment and sees the object *after* the
/// outcome it is describing, so neither can decide whether the claim itself is the same one.
/// What it does compare is every field that would contradict the stored answer — the stage,
/// the object, the hashes, the authority and the reason.
fn same_outcome(left: &ExternalChangeFact, right: &ExternalChangeFact) -> bool {
    left.call_id == right.call_id
        && left.operation_id == right.operation_id
        && left.stage == right.stage
        && left.display_path == right.display_path
        && left.previous_hash == right.previous_hash
        && left.resulting_hash == right.resulting_hash
        && left.identity == right.identity
        && left.authority == right.authority
        && left.reason == right.reason
}

/// Whether the object a new fact names is the same object the stored fact named.
///
/// A call's destination never changes. Comparing only the operation would let a fact keep its
/// operation identity and still describe a different file, which is exactly how a
/// reconciliation could be made to "confirm" an outcome for an object it never checked.
fn same_object(left: &ExternalChangeFact, right: &ExternalChangeFact) -> bool {
    left.call_id == right.call_id
        && left.display_path == right.display_path
        && left.identity == right.identity
}

/// Whether one stage may follow another for the same call by a plain `record`.
///
/// The whole matrix lives here rather than in a chain of special cases, so every legal
/// progression is stated once and everything else is refused by default:
///
/// * `prepared` is the record of an intention, written **before** the attempt. From it the
///   first outcome may be recorded — `published`, `not_performed`, `compensated` or `unknown`,
///   depending on what the publication really reached — and `prepared` may be repeated,
///   because a duplicated intention is not a new claim;
/// * nothing follows a recorded outcome. `unknown` is an outcome too: it says this could not be
///   confirmed, and only [`reconcile`] may settle it. No answer moves on its own.
///
/// Without the second rule a published change could be walked back to an unconfirmed state and
/// then to `not_performed` — the defect was `published → prepared → not_performed` — and the
/// record would end up saying the object was never touched.
fn is_legal_successor(previous: ExternalChangeStage, next: ExternalChangeStage) -> bool {
    use ExternalChangeStage as Stage;
    match previous {
        Stage::Prepared => matches!(
            next,
            Stage::Prepared
                | Stage::Published
                | Stage::NotPerformed
                | Stage::Compensated
                | Stage::Unknown
        ),
        Stage::Unknown | Stage::Published | Stage::Compensated | Stage::NotPerformed => false,
    }
}

/// Records one stage of an external mutation.
///
/// Stages are appended, never rewritten: the sequence of facts is what shows that a
/// publication was attempted even when its outcome never came back.
///
/// The successor has to be legal for the state the call is already in, and a repeated
/// identical fact is idempotent so a retried bookkeeping write cannot fail the call it
/// describes. Everything else is a conflict: the only way to produce it is to have lost track
/// of what really happened to the object. Settling an unconfirmed outcome is not this
/// function's job — that is [`reconcile`], and it enforces its own, narrower rules.
pub fn record(
    tx: &Transaction<'_>,
    batch: ToolBatchId,
    fact: &ExternalChangeFact,
) -> StoreResult<()> {
    if let Some(previous) = latest(tx, batch, fact.call_id)? {
        if same_outcome(&previous, fact) {
            // The same claim again: bookkeeping, not a new one.
            return append(tx, batch, fact);
        }
        if !same_object(&previous, fact) || previous.authority != fact.authority {
            return Err(Error::Conflict);
        }
        // A stored binding is not erased by a later fact that omits it. Missing fields are
        // load compatibility for old rows, never a way for new rows to drop what is known.
        if previous.operation_id.is_some() && previous.operation_id != fact.operation_id {
            return Err(Error::Conflict);
        }
        if !is_legal_successor(previous.stage, fact.stage) {
            return Err(Error::Conflict);
        }
    }
    append(tx, batch, fact)
}

/// Settles one call's unconfirmed outcome with what a check of the real object found.
///
/// This is deliberately narrower than [`record`]. It is the one path that may move a fact out
/// of `unknown`/`prepared`, and it only accepts a **confirming** outcome: a checked object
/// either holds the new content (`published`) or does not (`not_performed` or `compensated`).
/// It can never be used the other way round — a published change stays published — and a call
/// whose outcome is already an answer is refused rather than revisited.
///
/// The binding is enforced here, in the same transaction that reads the stored fact, because
/// a caller cannot be trusted to have checked it:
///
/// * the reconciliation must name the **operation** the original call belonged to, and that
///   operation must be the one already recorded. A fact that omits it is refused rather than
///   treated as matching, so an existing binding cannot be dropped;
/// * it must name the same **object**. A reconciliation against a different path is refused
///   even when the operation identity is preserved.
///
/// Nothing here decides *what* the check found, and nothing here replays anything: a
/// reconciliation is evidence about an object, not permission to write to it again.
pub fn reconcile(
    tx: &Transaction<'_>,
    batch: ToolBatchId,
    fact: &ExternalChangeFact,
) -> StoreResult<()> {
    let previous = latest(tx, batch, fact.call_id)?.ok_or(Error::Conflict)?;
    // Only an unconfirmed outcome is settled, and only into an answer. `unknown` is an
    // unconfirmed outcome even though `record` treats it as the end of its own chain, which is
    // exactly why it is spelled out here rather than derived from the transition matrix.
    if !previous.stage.is_unconfirmed() {
        return Err(Error::Conflict);
    }
    if !matches!(
        fact.stage,
        ExternalChangeStage::Published
            | ExternalChangeStage::NotPerformed
            | ExternalChangeStage::Compensated
    ) {
        return Err(Error::Conflict);
    }
    // The operation is required on both sides. A stored fact always has one; a reconciliation
    // that does not name it is not bound to this call's execution.
    match (previous.operation_id, fact.operation_id) {
        (Some(stored), Some(named)) if stored == named => {}
        _ => return Err(Error::Conflict),
    }
    if !same_object(&previous, fact) || previous.authority != fact.authority {
        return Err(Error::Conflict);
    }
    append(tx, batch, fact)
}

fn append(tx: &Transaction<'_>, batch: ToolBatchId, fact: &ExternalChangeFact) -> StoreResult<()> {
    event::append(
        tx,
        &event::EventDraft {
            id: EventId::new(),
            owner: &batch.to_string(),
            kind: KIND,
            version: VERSION,
            payload: &serde_json::to_value(fact)?,
            at: fact.at,
        },
        "tool",
    )?;
    Ok(())
}

/// The latest stage recorded for every call of one batch.
pub fn load(
    db: &Connection,
    batch: ToolBatchId,
) -> StoreResult<BTreeMap<ToolCallId, ExternalChangeFact>> {
    let mut statement = db.prepare(
        "SELECT payload FROM events WHERE owner=?1 AND kind=?2 AND version=?3 ORDER BY sequence",
    )?;
    let rows = statement
        .query_map([batch.to_string(), KIND.to_owned(), VERSION.to_string()], |row| {
            row.get::<_, String>(0)
        })?;
    let mut facts = BTreeMap::new();
    for row in rows {
        let fact: ExternalChangeFact = serde_json::from_str(&row?).map_err(|_| Error::Conflict)?;
        facts.insert(fact.call_id, fact);
    }
    Ok(facts)
}

/// The latest stage recorded for one call of one batch.
///
/// The lookup names the call, so it is answered by that call's own rows. The batch is part
/// of the key because it is what the rows are owned by: a match under another batch is a
/// different execution, not this call's fact.
pub fn fact(
    db: &Connection,
    batch: ToolBatchId,
    call: ToolCallId,
) -> StoreResult<Option<ExternalChangeFact>> {
    let mut statement = db.prepare(
        "SELECT payload FROM events WHERE owner=?1 AND kind=?2 AND version=?3 \
         AND json_extract(payload,'$.call_id')=?4 ORDER BY sequence DESC LIMIT 1",
    )?;
    let mut rows = statement.query(params![
        batch.to_string(),
        KIND.to_owned(),
        VERSION.to_string(),
        call.to_string()
    ])?;
    let Some(row) = rows.next()? else { return Ok(None) };
    let raw: String = row.get(0)?;
    serde_json::from_str(&raw).map(Some).map_err(|_| Error::Conflict)
}

/// The latest stage recorded for one call, whichever batch recorded it.
///
/// Only the read-back path uses this: the broker knows the call it is reporting on but not
/// the batch it belonged to, and asking per batch would mean guessing one. The call identity
/// is a ULID, so the answer is this call's fact and nothing else's. Every writer uses
/// [`record`], which is scoped to its own batch.
pub fn latest_for_call(
    db: &Connection,
    call: ToolCallId,
) -> StoreResult<Option<ExternalChangeFact>> {
    let mut statement = db.prepare(
        "SELECT payload FROM events WHERE kind=?1 AND version=?2 \
         AND json_extract(payload,'$.call_id')=?3 ORDER BY sequence DESC LIMIT 1",
    )?;
    let mut rows =
        statement.query(params![KIND.to_owned(), VERSION.to_string(), call.to_string()])?;
    let Some(row) = rows.next()? else { return Ok(None) };
    let raw: String = row.get(0)?;
    serde_json::from_str(&raw).map(Some).map_err(|_| Error::Conflict)
}

fn latest(
    tx: &Transaction<'_>,
    batch: ToolBatchId,
    call: ToolCallId,
) -> StoreResult<Option<ExternalChangeFact>> {
    let mut statement = tx.prepare(
        "SELECT payload FROM events WHERE owner=?1 AND kind=?2 AND version=?3 \
         AND json_extract(payload,'$.call_id')=?4 ORDER BY sequence DESC LIMIT 1",
    )?;
    let mut rows = statement.query(params![
        batch.to_string(),
        KIND.to_owned(),
        VERSION.to_string(),
        call.to_string()
    ])?;
    let Some(row) = rows.next()? else { return Ok(None) };
    let raw: String = row.get(0)?;
    serde_json::from_str(&raw).map(Some).map_err(|_| Error::Conflict)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::V2Store;

    fn fixture() -> (tempfile::TempDir, V2Store) {
        let root = tempfile::tempdir().expect("store root");
        let store = V2Store::create(&root.path().join("v2")).expect("store");
        (root, store)
    }

    fn draft(call: ToolCallId, stage: ExternalChangeStage) -> ExternalChangeFact {
        ExternalChangeFact {
            call_id: call,
            operation_id: None,
            stage,
            display_path: "C:/synthetic/external-change.txt".into(),
            previous_hash: None,
            resulting_hash: None,
            identity: None,
            observed_previous_hash: None,
            observed_identity: None,
            authority: ExternalChangeAuthority::Policy,
            reason: None,
            at: UtcTimestamp::now(),
        }
    }

    /// A fact is found through its own call identity, so unrelated traffic in other batches
    /// cannot push it out of view. The window this replaces was the last 32 rows of the
    /// whole table, which turned a recorded fact into "no fact at all".
    #[test]
    fn a_fact_is_found_behind_an_arbitrary_amount_of_unrelated_history() {
        let (_root, mut store) = fixture();
        let batch = ToolBatchId::new();
        let original = ToolCallId::new();
        for _ in 0..20 {
            let other = ToolBatchId::new();
            let other_call = ToolCallId::new();
            for stage in [ExternalChangeStage::Prepared, ExternalChangeStage::Published] {
                store
                    .database
                    .unit_of_work(|tx| record(tx, other, &draft(other_call, stage)))
                    .expect("unrelated fact");
            }
        }
        store
            .database
            .unit_of_work(|tx| record(tx, batch, &draft(original, ExternalChangeStage::Prepared)))
            .expect("prepared");
        store
            .database
            .unit_of_work(|tx| record(tx, batch, &draft(original, ExternalChangeStage::Published)))
            .expect("published");

        let found = fact(store.database.connection(), batch, original)
            .expect("query")
            .expect("the recorded fact is readable");
        assert_eq!(found.stage, ExternalChangeStage::Published);
        assert_eq!(found.call_id, original);
        assert_eq!(
            latest_for_call(store.database.connection(), original)
                .expect("query")
                .map(|value| value.stage),
            Some(ExternalChangeStage::Published),
            "the read-back path finds the same fact without being told the batch"
        );
    }

    /// A call has one outcome. The second terminal stage is refused rather than appended, so
    /// the stored answer stays the one that really happened.
    #[test]
    fn a_terminal_outcome_cannot_be_replaced_by_a_different_one() {
        let (_root, mut store) = fixture();
        let batch = ToolBatchId::new();
        let call = ToolCallId::new();
        for stage in [ExternalChangeStage::Prepared, ExternalChangeStage::Published] {
            store
                .database
                .unit_of_work(|tx| record(tx, batch, &draft(call, stage)))
                .expect("stage");
        }
        let second = store
            .database
            .unit_of_work(|tx| record(tx, batch, &draft(call, ExternalChangeStage::NotPerformed)));
        assert!(second.is_err(), "a published change must not become not_performed");
        assert_eq!(
            fact(store.database.connection(), batch, call).expect("query").map(|value| value.stage),
            Some(ExternalChangeStage::Published),
            "the refused write left the recorded outcome alone"
        );
    }

    /// Repeating the same terminal fact is bookkeeping, not a new claim, so a retried write
    /// is accepted instead of failing the call it describes.
    #[test]
    fn the_same_terminal_fact_may_be_recorded_twice() {
        let (_root, mut store) = fixture();
        let batch = ToolBatchId::new();
        let call = ToolCallId::new();
        for stage in [
            ExternalChangeStage::Prepared,
            ExternalChangeStage::Published,
            ExternalChangeStage::Published,
        ] {
            store
                .database
                .unit_of_work(|tx| record(tx, batch, &draft(call, stage)))
                .expect("idempotent");
        }
        assert_eq!(
            fact(store.database.connection(), batch, call).expect("query").map(|value| value.stage),
            Some(ExternalChangeStage::Published)
        );
    }

    /// An unconfirmed mutation may be settled by a real check, and only in the confirming
    /// direction: the reconciliation path cannot rewrite an answer that exists, and it must
    /// name the operation the stored fact belongs to.
    #[test]
    fn only_an_unconfirmed_mutation_may_be_reconciled_and_only_towards_an_answer() {
        let (_root, mut store) = fixture();
        let batch = ToolBatchId::new();
        let call = ToolCallId::new();
        let operation = SecurityOperationId::new();
        let bound = |stage, operation_id| ExternalChangeFact { operation_id, ..draft(call, stage) };
        for stage in [ExternalChangeStage::Prepared, ExternalChangeStage::Unknown] {
            store
                .database
                .unit_of_work(|tx| record(tx, batch, &bound(stage, Some(operation))))
                .expect("stage");
        }
        // A reconciliation that does not name the operation is not bound to this call.
        let unbound = store
            .database
            .unit_of_work(|tx| reconcile(tx, batch, &bound(ExternalChangeStage::Published, None)));
        assert!(unbound.is_err(), "a reconciliation must name the operation it checked");
        store
            .database
            .unit_of_work(|tx| {
                reconcile(tx, batch, &bound(ExternalChangeStage::Published, Some(operation)))
            })
            .expect("an unknown outcome may be confirmed as published");
        let downward = store.database.unit_of_work(|tx| {
            reconcile(tx, batch, &bound(ExternalChangeStage::NotPerformed, Some(operation)))
        });
        assert!(downward.is_err(), "a confirmed outcome is not reconsidered");
        let unknown_again = store.database.unit_of_work(|tx| {
            record(tx, batch, &bound(ExternalChangeStage::Unknown, Some(operation)))
        });
        assert!(unknown_again.is_err(), "a settled call does not go back to unknown");
    }

    /// A fact carries the observations the file layer really made, and a row written before
    /// those fields existed loads with them absent instead of with an invented value.
    #[test]
    fn observations_are_stored_as_made_and_older_rows_load_without_them() {
        let (_root, mut store) = fixture();
        let batch = ToolBatchId::new();
        let call = ToolCallId::new();
        let mut written = draft(call, ExternalChangeStage::Published);
        written.observed_previous_hash = Some("blake3:before".into());
        written.observed_identity = Some("{\"volume\":7,\"index\":32}".into());
        store.database.unit_of_work(|tx| record(tx, batch, &written)).expect("record");

        let loaded =
            fact(store.database.connection(), batch, call).expect("query").expect("the fact");
        assert_eq!(loaded.observed_previous_hash.as_deref(), Some("blake3:before"));
        assert_eq!(loaded.observed_identity.as_deref(), Some("{\"volume\":7,\"index\":32}"));

        let legacy = serde_json::json!({
            "call_id": call,
            "stage": "published",
            "display_path": "C:/synthetic/external-change.txt",
            "previous_hash": null,
            "resulting_hash": null,
            "identity": null,
            "authority": "policy",
            "reason": null,
            "at": "2026-01-01T00:00:00Z",
        });
        let decoded: ExternalChangeFact = serde_json::from_value(legacy).expect("legacy row");
        assert_eq!(decoded.observed_previous_hash, None);
        assert_eq!(decoded.observed_identity, None);
        assert_eq!(decoded.operation_id, None);
    }
}
