//! Opt-in, read-only diagnostic. Prints boundaries, never message or credential data.
use aura_agent_domain::ModelStepState;
use aura_domain_common::*;
use aura_model_domain::*;
use aura_persistence_v2::{
    agent::{history, model_context, turns},
    context, model, tool,
};

#[test]
#[ignore = "requires an explicitly selected local read-only database and run"]
fn inspect_history_boundaries() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::var("AURA_DIAGNOSTIC_DB")?;
    let run_id: AgentRunId = std::env::var("AURA_DIAGNOSTIC_RUN")?.parse()?;
    let db =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let run = turns::load_run(&db, run_id)?;
    let turn = turns::load(&db, run.turn_task_id)?;
    println!(
        "bundle: {:?}",
        model_context::bundle(&db, run.prompt_bundle_snapshot_id, turn.session_id).map(|_| ())
    );
    println!("groups: {:?}", context::history(&db, turn.session_id).map(|g| g.len()));
    println!(
        "lineage view (resolution={}): {:?}",
        turn.lineage_resolution_required,
        aura_persistence_v2::agent::inspector::lineage_view(&db, turn.lineage_id).map(|_| ())
    );
    println!(
        "previous context: {:?}",
        aura_persistence_v2::agent::inspector::previous_context(&db, turn.session_id).map(|_| ())
    );
    let mut q = db.prepare("SELECT id FROM turn_tasks WHERE session_id=?1 ORDER BY rowid")?;
    let ids = q
        .query_map([turn.session_id.to_string()], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for id in ids {
        let previous = turns::load(&db, id.parse()?)?;
        for step in model_context::steps(&db, previous.agent_run_id)? {
            if let ModelStepState::Completed { tool_batch_id, .. } = step.state {
                let output = history::output(&db, step.model_step_id)?;
                let snapshot = model::step_snapshot(&db, step.model_step_id)?;
                let evidence =
                    model::request_evidence(&db, snapshot.id)?.ok_or("missing evidence")?;
                println!(
                    "step {} normalize {:?}; bindings {:?}",
                    step.model_step_id,
                    derive_p07_normalized_tool_calls_v1(&snapshot, &evidence, &output).map(|_| ()),
                    derive_provider_tool_call_protocol_bindings(&snapshot, &evidence, &output)
                        .map(|_| ())
                );
                if let Some(batch) = tool_batch_id {
                    println!("batch: {:?}", tool::execution::batch(&db, batch).map(|_| ()));
                }
            }
        }
    }
    let result = model_context::history_for_model(&db, run_id, true);
    println!("history: {:?}", result.as_ref().map(|h| h.items.len()));
    result?;
    Ok(())
}
