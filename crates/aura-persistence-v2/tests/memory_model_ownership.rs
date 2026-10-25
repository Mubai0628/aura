#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::StoreResult;
use aura_persistence_v2::{
    V2Store,
    agent::{turns, work},
    model::{self, memory_maintenance as owner},
};
#[test]
fn auxiliary_model_is_bound_to_completed_source_does_not_charge_goal_and_never_replays() {
    let temp = tempfile::tempdir().expect("model ownership fixture");
    let mut store = V2Store::create(&temp.path().join("v2")).expect("model ownership fixture");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Goal);
    let mut snapshot = aura_contract_schema::canonical_fixture()
        .expect("model ownership fixture")
        .models
        .execution_snapshot
        .expect("model ownership fixture");
    snapshot.id = ModelExecutionSnapshotId::new();
    snapshot.model_step_id = ModelStepId::new();
    snapshot.prompt_bundle_snapshot_id =
        turns::load_run(store.database.connection(), f.turn.agent_run_id)
            .expect("model ownership fixture")
            .prompt_bundle_snapshot_id;
    let runtime = RuntimeInstanceId::new();
    assert!(
        store
            .database
            .unit_of_work(|tx| owner::open(tx, f.turn.agent_run_id, runtime, &snapshot, f.at))
            .is_err(),
        "active turn cannot become auxiliary owner"
    );
    let input = NormalCompleteInput {
        lineage_id: f.turn.lineage_id,
        expected_lineage_revision: Revision::INITIAL,
        final_report: "Verified".into(),
        disclosed_limitations: vec![],
    };
    let batch = f.admit(
        &mut store,
        vec![("normal.complete", serde_json::to_value(&input).expect("model ownership fixture"))],
    );
    store
        .database
        .unit_of_work(|tx| -> StoreResult<()> {
            f.success(tx, &batch, 0)?;
            aura_persistence_v2::agent::history::complete_batch(
                tx,
                batch.batch.tool_batch_id,
                f.at,
            )?;
            aura_persistence_v2::agent::completion::normal(
                tx,
                f.step.model_step_id,
                batch.batch.tool_batch_id,
                &input,
                false,
                f.at,
            )?;
            owner::open(tx, f.turn.agent_run_id, runtime, &snapshot, f.at)
        })
        .expect("model ownership fixture");
    let before = serde_json::to_value(
        work::goal(store.database.connection(), f.turn.turn_task_id)
            .expect("model ownership fixture"),
    )
    .expect("model ownership fixture");
    assert!(
        owner::authorize_model(store.database.connection(), snapshot.model_step_id)
            .expect("model ownership fixture")
    );
    let mut output = f.template.clone();
    output.model_step_id = snapshot.model_step_id;
    output.tool_calls.clear();
    output.usage.input_tokens = 500;
    output.usage.output_tokens = 100;
    store
        .database
        .unit_of_work(|tx| owner::record_output(tx, &snapshot, &output, f.at))
        .expect("model ownership fixture");
    let after = serde_json::to_value(
        work::goal(store.database.connection(), f.turn.turn_task_id)
            .expect("model ownership fixture"),
    )
    .expect("model ownership fixture");
    assert_eq!(before, after, "maintenance usage never debits the Goal");
    let changed = snapshot.clone();
    store
        .database
        .connection()
        .execute(
            "UPDATE model_execution_snapshots SET session_id=?2 WHERE id=?1",
            [changed.id.to_string(), SessionId::new().to_string()],
        )
        .unwrap_err(); // FK protects nonexistent sessions.
    assert!(
        store
            .database
            .unit_of_work(|tx| owner::close(
                tx,
                snapshot.model_step_id,
                RuntimeInstanceId::new(),
                "foreign runtime",
                f.at
            ))
            .is_err()
    );
    store
        .database
        .unit_of_work(|tx| owner::interrupt_previous_process(tx, RuntimeInstanceId::new(), f.at))
        .expect("model ownership fixture");
    assert!(
        !owner::authorize_model(store.database.connection(), snapshot.model_step_id)
            .expect("model ownership fixture")
    );
    assert!(
        model::agent_transport::owning_step(store.database.connection(), snapshot.model_step_id)
            .is_err()
    );
    assert!(matches!(
        turns::load(store.database.connection(), f.turn.turn_task_id)
            .expect("model ownership fixture")
            .state,
        TurnTaskState::Completed { .. }
    ));
    let mut mutated = snapshot.clone();
    mutated.model_profile_id = ModelProfileId::new();
    assert!(
        store
            .database
            .unit_of_work(|tx| owner::open(tx, f.turn.agent_run_id, runtime, &mutated, f.at))
            .is_err()
    );
}
