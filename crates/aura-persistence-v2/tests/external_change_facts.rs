//! What an external mutation did, as a durable fact rather than a guess.
//!
//! These are the regressions for two independent defects in how those facts were read and
//! written:
//!
//! * a call's fact was looked for in the **last 32 rows of the whole table**, so a recorded
//!   publication disappeared from view as soon as enough unrelated calls were recorded. A
//!   fact that exists must never be reported as "no fact": the read-back path turns a missing
//!   fact into an unconfirmed side effect, so losing one makes the model's answer wrong in
//!   the expensive direction;
//! * writing a stage was an unconditional append, so a published change could be rewritten
//!   as `not_performed` afterwards — the object really had the new content and the record
//!   said nothing happened.
//!
//! A call now has exactly one terminal outcome. The same fact written twice is still accepted
//! (a retried bookkeeping write must not fail the call it describes), and an *unconfirmed*
//! outcome may be settled by a check of the real object through the separate reconciliation
//! path — never by a plain overwrite in either direction.
#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::AgentTaskMode;
use aura_domain_common::*;
use aura_persistence_v2::{
    V2Store,
    tool::external_change::{
        self, ExternalChangeAuthority, ExternalChangeFact, ExternalChangeStage as Stage,
    },
};

fn setup() -> (tempfile::TempDir, V2Store, support::Fixture) {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    (root, store, fixture)
}

fn draft(
    call: ToolCallId,
    stage: Stage,
    at: UtcTimestamp,
    operation_id: Option<SecurityOperationId>,
) -> ExternalChangeFact {
    ExternalChangeFact {
        call_id: call,
        operation_id,
        stage,
        display_path: "C:/synthetic-review/file.txt".into(),
        previous_hash: None,
        resulting_hash: None,
        identity: None,
        observed_previous_hash: None,
        observed_identity: None,
        authority: ExternalChangeAuthority::Policy,
        reason: None,
        at,
    }
}

/// The same draft, naming the object it is about, so reconciliation binding can be exercised.
fn draft_for(
    call: ToolCallId,
    stage: Stage,
    at: UtcTimestamp,
    operation: SecurityOperationId,
    display_path: &str,
) -> ExternalChangeFact {
    ExternalChangeFact {
        display_path: display_path.to_owned(),
        ..draft(call, stage, at, Some(operation))
    }
}

/// The exact shape the review reproduced: one batch of twenty calls, each writing a
/// prepared and a published fact, and the first call's own fact must still be readable.
#[test]
fn a_persistent_fact_is_not_lost_behind_unrelated_calls() {
    let (_root, mut store, fixture) = setup();
    let batch = ToolBatchId::new();
    let original = ToolCallId::new();
    for n in 0..20 {
        let call = if n == 0 { original } else { ToolCallId::new() };
        for stage in [Stage::Prepared, Stage::Published] {
            store
                .database
                .unit_of_work(|tx| {
                    external_change::record(tx, batch, &draft(call, stage, fixture.at, None))
                })
                .expect("record stage");
        }
    }
    let one = external_change::fact(store.database.connection(), batch, original)
        .expect("query")
        .expect("the recorded fact is readable");
    assert_eq!(
        one.stage,
        Stage::Published,
        "a persistent fact cannot disappear behind unrelated events"
    );
    let all = external_change::load(store.database.connection(), batch).expect("batch facts");
    assert_eq!(all.len(), 20, "every call of the batch keeps its latest stage");
    assert_eq!(all.get(&original).map(|fact| fact.stage), Some(Stage::Published));
}

/// Facts in *other* batches are other executions, so they must not answer for this call, and
/// they must not push this call's fact out of a window either.
#[test]
fn facts_are_found_by_call_identity_across_any_amount_of_history() {
    let (_root, mut store, fixture) = setup();
    let ours = ToolBatchId::new();
    let call = ToolCallId::new();
    // Two hundred unrelated events first, so any fixed-size recent window is exhausted.
    for _ in 0..100 {
        let other = ToolBatchId::new();
        let other_call = ToolCallId::new();
        for stage in [Stage::Prepared, Stage::Published] {
            store
                .database
                .unit_of_work(|tx| {
                    external_change::record(tx, other, &draft(other_call, stage, fixture.at, None))
                })
                .expect("unrelated fact");
        }
    }
    store
        .database
        .unit_of_work(|tx| {
            external_change::record(tx, ours, &draft(call, Stage::Prepared, fixture.at, None))
        })
        .expect("prepared");
    store
        .database
        .unit_of_work(|tx| {
            external_change::record(tx, ours, &draft(call, Stage::Published, fixture.at, None))
        })
        .expect("published");

    let found = external_change::fact(store.database.connection(), ours, call)
        .expect("query")
        .expect("the fact is readable");
    assert_eq!(found.stage, Stage::Published);
    assert_eq!(found.call_id, call);

    // A call with no fact is still reported as having none — the fix must not invent one.
    let absent =
        external_change::fact(store.database.connection(), ours, ToolCallId::new()).expect("query");
    assert!(absent.is_none());
    let wrong_batch = external_change::fact(store.database.connection(), ToolBatchId::new(), call)
        .expect("query");
    assert!(wrong_batch.is_none(), "another batch's rows are not this call's fact");
}

/// The read-back path (the broker's side-effect evidence) knows the call but not its batch,
/// so it looks the fact up by identity instead of scanning a recent window.
#[test]
fn the_read_back_path_finds_the_fact_without_being_told_the_batch() {
    let (_root, mut store, fixture) = setup();
    let batch = ToolBatchId::new();
    let call = ToolCallId::new();
    for stage in [Stage::Prepared, Stage::Published] {
        store
            .database
            .unit_of_work(|tx| {
                external_change::record(tx, batch, &draft(call, stage, fixture.at, None))
            })
            .expect("record stage");
    }
    for _ in 0..40 {
        let other = ToolBatchId::new();
        store
            .database
            .unit_of_work(|tx| {
                external_change::record(
                    tx,
                    other,
                    &draft(ToolCallId::new(), Stage::Published, fixture.at, None),
                )
            })
            .expect("later unrelated fact");
    }
    let found = external_change::latest_for_call(store.database.connection(), call)
        .expect("query")
        .expect("the fact is readable by call identity alone");
    assert_eq!(found.stage, Stage::Published);
    assert_eq!(found.call_id, call);
}

/// A published change must not become `not_performed`. The second terminal write is refused
/// and the stored outcome is left exactly as it was.
#[test]
fn a_second_different_terminal_result_is_refused() {
    let (_root, mut store, fixture) = setup();
    let batch = ToolBatchId::new();
    let call = ToolCallId::new();
    for stage in [Stage::Prepared, Stage::Published] {
        store
            .database
            .unit_of_work(|tx| {
                external_change::record(tx, batch, &draft(call, stage, fixture.at, None))
            })
            .expect("record stage");
    }
    let second = store.database.unit_of_work(|tx| {
        external_change::record(tx, batch, &draft(call, Stage::NotPerformed, fixture.at, None))
    });
    assert!(second.is_err(), "a published external change must not become not_performed");
    let latest = external_change::fact(store.database.connection(), batch, call)
        .expect("query")
        .expect("the original fact survives");
    assert_eq!(latest.stage, Stage::Published);

    // The same is true in the other direction: no terminal answer is rewritten by another.
    let call2 = ToolCallId::new();
    for stage in [Stage::Prepared, Stage::NotPerformed] {
        store
            .database
            .unit_of_work(|tx| {
                external_change::record(tx, batch, &draft(call2, stage, fixture.at, None))
            })
            .expect("record stage");
    }
    let overwrite = store.database.unit_of_work(|tx| {
        external_change::record(tx, batch, &draft(call2, Stage::Published, fixture.at, None))
    });
    assert!(overwrite.is_err(), "a refusal cannot be turned into a publication after the fact");
}

/// Writing the *same* terminal fact twice is bookkeeping, not a new claim: a retried write
/// must not fail the call it describes.
#[test]
fn repeating_the_same_terminal_fact_is_idempotent() {
    let (_root, mut store, fixture) = setup();
    let batch = ToolBatchId::new();
    let call = ToolCallId::new();
    for stage in [Stage::Prepared, Stage::Published, Stage::Published] {
        store
            .database
            .unit_of_work(|tx| {
                external_change::record(tx, batch, &draft(call, stage, fixture.at, None))
            })
            .expect("idempotent stage");
    }
    assert_eq!(
        external_change::fact(store.database.connection(), batch, call)
            .expect("query")
            .map(|fact| fact.stage),
        Some(Stage::Published)
    );
}

/// An outcome that was never confirmed may be settled by checking the real object — and only
/// towards an answer. A call whose outcome is already known is not revisited, and an unknown
/// outcome cannot be produced from a known one.
#[test]
fn a_reconciliation_settles_only_an_unconfirmed_outcome() {
    let (_root, mut store, fixture) = setup();
    let batch = ToolBatchId::new();
    let call = ToolCallId::new();
    let operation = SecurityOperationId::new();
    for stage in [Stage::Prepared, Stage::Unknown] {
        store
            .database
            .unit_of_work(|tx| {
                external_change::record(tx, batch, &draft(call, stage, fixture.at, Some(operation)))
            })
            .expect("record stage");
    }
    store
        .database
        .unit_of_work(|tx| {
            external_change::reconcile(
                tx,
                batch,
                &draft(call, Stage::Published, fixture.at, Some(operation)),
            )
        })
        .expect("an unknown outcome may be confirmed as published");
    let downward = store.database.unit_of_work(|tx| {
        external_change::reconcile(
            tx,
            batch,
            &draft(call, Stage::NotPerformed, fixture.at, Some(operation)),
        )
    });
    assert!(downward.is_err(), "a confirmed outcome is not reconsidered");

    let fresh = ToolCallId::new();
    store
        .database
        .unit_of_work(|tx| {
            external_change::record(
                tx,
                batch,
                &draft(fresh, Stage::NotPerformed, fixture.at, Some(operation)),
            )
        })
        .expect("refusal");
    let upgrade = store.database.unit_of_work(|tx| {
        external_change::reconcile(
            tx,
            batch,
            &draft(fresh, Stage::Published, fixture.at, Some(operation)),
        )
    });
    assert!(upgrade.is_err(), "a refusal is an answer, so it is not reconciled into a publication");
}

/// A stored answer cannot be walked back to "unconfirmed" and then replaced.
///
/// `prepared` and `unknown` are not terminal, so a rule that only compared two terminal stages
/// let a published change be recorded as `prepared` — an unconfirmed state — and then as
/// `not_performed`. The record would end up saying the object was never touched, while the new
/// content had really been published. This covers the whole matrix rather than that one path:
/// no terminal stage has any legal successor, and `record` never settles an unconfirmed one.
#[test]
fn a_settled_outcome_has_no_way_back() {
    for settled in [Stage::Published, Stage::NotPerformed, Stage::Compensated] {
        for next in [
            Stage::Prepared,
            Stage::Unknown,
            Stage::Published,
            Stage::NotPerformed,
            Stage::Compensated,
        ] {
            if next == settled {
                continue;
            }
            let (_root, mut store, fixture) = setup();
            let batch = ToolBatchId::new();
            let call = ToolCallId::new();
            let operation = SecurityOperationId::new();
            store
                .database
                .unit_of_work(|tx| {
                    external_change::record(
                        tx,
                        batch,
                        &draft(call, Stage::Prepared, fixture.at, Some(operation)),
                    )
                })
                .expect("prepared");
            store
                .database
                .unit_of_work(|tx| {
                    external_change::record(
                        tx,
                        batch,
                        &draft(call, settled, fixture.at, Some(operation)),
                    )
                })
                .expect("the settled outcome");
            let written = store.database.unit_of_work(|tx| {
                external_change::record(tx, batch, &draft(call, next, fixture.at, Some(operation)))
            });
            assert!(
                written.is_err(),
                "{settled:?} must not be replaced by {next:?} through record"
            );
            assert_eq!(
                external_change::fact(store.database.connection(), batch, call)
                    .expect("query")
                    .map(|fact| fact.stage),
                Some(settled),
                "the stored answer survives the refused write"
            );
        }
    }
}

/// `prepared` is an intention, not a state to return to: it is recorded once and the call then
/// moves to an outcome, so a settled call cannot re-declare that it is about to do something.
#[test]
fn prepared_is_recorded_once_before_any_outcome() {
    let (_root, mut store, fixture) = setup();
    let batch = ToolBatchId::new();
    let call = ToolCallId::new();
    let operation = SecurityOperationId::new();
    store
        .database
        .unit_of_work(|tx| {
            external_change::record(
                tx,
                batch,
                &draft(call, Stage::Prepared, fixture.at, Some(operation)),
            )
        })
        .expect("prepared");
    // Repeating it before anything else is a duplicated intent, which is not a new claim.
    store
        .database
        .unit_of_work(|tx| {
            external_change::record(
                tx,
                batch,
                &draft(call, Stage::Prepared, fixture.at, Some(operation)),
            )
        })
        .expect("idempotent prepared");
    store
        .database
        .unit_of_work(|tx| {
            external_change::record(
                tx,
                batch,
                &draft(call, Stage::Unknown, fixture.at, Some(operation)),
            )
        })
        .expect("an unconfirmed outcome may replace the intention");
    let back = store.database.unit_of_work(|tx| {
        external_change::record(
            tx,
            batch,
            &draft(call, Stage::Prepared, fixture.at, Some(operation)),
        )
    });
    assert!(back.is_err(), "an unconfirmed outcome does not return to the intention");
}

/// A call's facts belong to one execution. Another batch cannot create a competing answer for
/// the same call identity, because the only way to do so is to have lost which execution the
/// call belonged to.
#[test]
fn a_second_batch_cannot_invent_an_answer_for_another_executions_call() {
    let (_root, mut store, fixture) = setup();
    let owner = ToolBatchId::new();
    let call = ToolCallId::new();
    let operation = SecurityOperationId::new();
    store
        .database
        .unit_of_work(|tx| {
            external_change::record(
                tx,
                owner,
                &draft(call, Stage::Prepared, fixture.at, Some(operation)),
            )
        })
        .expect("prepared");
    store
        .database
        .unit_of_work(|tx| {
            external_change::record(
                tx,
                owner,
                &draft(call, Stage::Published, fixture.at, Some(operation)),
            )
        })
        .expect("published");

    // A different batch naming the same call, with a different operation, is a different
    // execution claiming the same identity: it must not be accepted as this call's answer.
    let intruder = ToolBatchId::new();
    let other_operation = SecurityOperationId::new();
    let forged = store.database.unit_of_work(|tx| {
        external_change::reconcile(
            tx,
            intruder,
            &draft(call, Stage::NotPerformed, fixture.at, Some(other_operation)),
        )
    });
    assert!(forged.is_err(), "another batch cannot settle this call's outcome");

    // And the original answer is untouched and still readable through both paths.
    assert_eq!(
        external_change::fact(store.database.connection(), owner, call)
            .expect("query")
            .map(|fact| fact.stage),
        Some(Stage::Published)
    );
    assert_eq!(
        external_change::latest_for_call(store.database.connection(), call)
            .expect("query")
            .map(|fact| fact.stage),
        Some(Stage::Published)
    );
}

/// A reconciliation is bound to the call's own execution and its own object.
///
/// Omitting the operation used to be read as "matches", so a reconciliation could keep an
/// operation identity it never proved — or clear an existing one — and it could name a
/// different file and still be accepted. Both let a check of one thing be recorded as
/// evidence about another.
#[test]
fn a_reconciliation_must_name_the_operation_and_the_object_it_checked() {
    let (_root, mut store, fixture) = setup();
    let batch = ToolBatchId::new();
    let call = ToolCallId::new();
    let operation = SecurityOperationId::new();
    let path = "C:/synthetic-review/target.txt";
    store
        .database
        .unit_of_work(|tx| {
            external_change::record(
                tx,
                batch,
                &draft_for(call, Stage::Prepared, fixture.at, operation, path),
            )
        })
        .expect("prepared");
    store
        .database
        .unit_of_work(|tx| {
            external_change::record(
                tx,
                batch,
                &draft_for(call, Stage::Unknown, fixture.at, operation, path),
            )
        })
        .expect("unknown");

    // A different object, with the operation identity preserved.
    let elsewhere = store.database.unit_of_work(|tx| {
        external_change::reconcile(
            tx,
            batch,
            &draft_for(
                call,
                Stage::Published,
                fixture.at,
                operation,
                "C:/synthetic-review/other.txt",
            ),
        )
    });
    assert!(elsewhere.is_err(), "a reconciliation against another object is not evidence");

    // Omitting the operation is not the same as matching it.
    let unbound = store.database.unit_of_work(|tx| {
        external_change::reconcile(tx, batch, &draft(call, Stage::Published, fixture.at, None))
    });
    assert!(unbound.is_err(), "a reconciliation must name the operation it belongs to");

    // A different operation does not inherit this call's history.
    let impostor = store.database.unit_of_work(|tx| {
        external_change::reconcile(
            tx,
            batch,
            &draft_for(call, Stage::Published, fixture.at, SecurityOperationId::new(), path),
        )
    });
    assert!(impostor.is_err(), "another operation cannot settle this call");

    // The bound reconciliation is the one that works.
    store
        .database
        .unit_of_work(|tx| {
            external_change::reconcile(
                tx,
                batch,
                &draft_for(call, Stage::Published, fixture.at, operation, path),
            )
        })
        .expect("the object and operation this call really had");
    assert_eq!(
        external_change::fact(store.database.connection(), batch, call)
            .expect("query")
            .map(|fact| fact.stage),
        Some(Stage::Published)
    );
}

/// A fact that already names an operation is never allowed to lose it, and a fact that names a
/// different object is refused rather than appended.
#[test]
fn a_recorded_binding_is_not_erased_or_moved_by_a_later_fact() {
    let (_root, mut store, fixture) = setup();
    let batch = ToolBatchId::new();
    let call = ToolCallId::new();
    let operation = SecurityOperationId::new();
    store
        .database
        .unit_of_work(|tx| {
            external_change::record(
                tx,
                batch,
                &draft(call, Stage::Prepared, fixture.at, Some(operation)),
            )
        })
        .expect("prepared");

    let cleared = store.database.unit_of_work(|tx| {
        external_change::record(tx, batch, &draft(call, Stage::Unknown, fixture.at, None))
    });
    assert!(cleared.is_err(), "an existing operation binding is not dropped");

    let moved = store.database.unit_of_work(|tx| {
        external_change::record(
            tx,
            batch,
            &draft_for(
                call,
                Stage::Unknown,
                fixture.at,
                operation,
                "C:/synthetic-review/elsewhere.txt",
            ),
        )
    });
    assert!(moved.is_err(), "a call does not change the object it is about");
}

/// The fact carries the operation it belongs to and the observations the file layer really
/// made, and a row written before those fields existed loads with them absent rather than
/// with a value invented from the call's own request.
#[test]
fn a_fact_binds_its_operation_and_keeps_the_observations_it_made() {
    let (_root, mut store, fixture) = setup();
    let batch = ToolBatchId::new();
    let call = ToolCallId::new();
    let operation = SecurityOperationId::new();
    let mut written = draft(call, Stage::Published, fixture.at, Some(operation));
    written.observed_previous_hash = Some("blake3:before".into());
    written.observed_identity = Some("{\"volume_serial_number\":\"7\",\"file_id\":\"32\"}".into());
    store.database.unit_of_work(|tx| external_change::record(tx, batch, &written)).expect("record");

    let loaded = external_change::fact(store.database.connection(), batch, call)
        .expect("query")
        .expect("the fact");
    assert_eq!(loaded.operation_id, Some(operation));
    assert_eq!(loaded.observed_previous_hash.as_deref(), Some("blake3:before"));
    assert_eq!(
        loaded.observed_identity.as_deref(),
        Some("{\"volume_serial_number\":\"7\",\"file_id\":\"32\"}")
    );

    // A row from before the observations existed still loads: their absence is the fact, not
    // an error and not a zero value.
    let legacy = serde_json::json!({
        "call_id": call,
        "stage": "published",
        "display_path": "C:/synthetic-review/file.txt",
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
    assert_eq!(decoded.stage, Stage::Published);
}
