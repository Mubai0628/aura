//! The read-only run-information query route.
//!
//! WHAT THIS ROUTE IS ALLOWED TO DO
//! Answer a question about what the real executors are running right now. Nothing else.
//! It takes no lock over a resource, starts and stops nothing, and cannot influence any
//! tool's fate. A failure here is a failed *query*: the tools keep running and A's common
//! file protection is untouched, because neither ever consults this route.
//!
//! WHY VISIBILITY IS RESOLVED HERE AND NOT FROM THE REQUEST
//! A request can name a session and a set of physical targets it claims to care about. It
//! cannot name its own workspace, and it cannot name another session's private body into
//! existence. The route resolves the caller's real workspace from the runtime's own
//! session projection, and the index applies the visibility filter before relevance and
//! before any budget - so a request can narrow what it sees, never widen it.
use crate::*;
use aura_activity_domain::{
    ACTIVITY_QUERY_CONTRACT_VERSION, ActivityQueryRequestV1, ActivityQueryResponseV1,
};
use aura_activity_runtime::{ActivityResourceKey, ActivityVisibility};
use aura_domain_common::SessionId;

pub(crate) async fn dispatch(
    services: &RuntimeServices,
    request: &JsonRpcRequest,
) -> Result<serde_json::Value> {
    let input: ActivityQueryRequestV1 = serde_json::from_value(request.params.clone())?;
    let session: SessionId = input.session_id;
    // R5: THE SESSION PROJECTION IS READ ON THE EXISTING BOUNDED OWNER, NOT ON THIS REACTOR.
    //
    // `workspace_session_projection()` is a synchronous read that takes the projection's own lock.
    // Calling it directly from this async entry - twice, as the previous shape did - is exactly the
    // shape that produced a reactor stall: a Tokio worker that also has to deliver the Core
    // heartbeat and answer another session was parked on a synchronous owner. The read itself is
    // unchanged (same projection, same coordinator, same statements); it now runs whole on the
    // process's one bounded synchronous boundary, which is the owner Core already uses for this
    // class of work, and it is read **once** for the whole request rather than once per use.
    let persistence = Arc::clone(&services.persistence);
    let projection = match services
        .sync
        .run(
            // The projection rebuild/read is the existing owner's own section: this route reads the
            // same projection that section maintains, so it uses that classification rather than
            // inventing a new one.
            aura_core_base::sync_boundary::SyncSection::Projection,
            aura_core_base::sync_boundary::SyncSubject::new()
                .request(session.to_string())
                .method("agent.activity.query"),
            move || persistence.workspace_session_projection(),
        )
        .await
    {
        aura_core_base::blocking_pool::BlockingAdmission::Completed(result) => result?,
        // The bounded owner refused or did not answer. This is a *query* failure and never a tool
        // outcome: nothing about any running tool changes, and A's protection is untouched. It is
        // reported as unavailable rather than as "nothing is running", which is the distinction the
        // coverage contract exists to preserve.
        aura_core_base::blocking_pool::BlockingAdmission::Refused { .. }
        | aura_core_base::blocking_pool::BlockingAdmission::Unknown { .. } => {
            anyhow::bail!("activity.query_unavailable")
        }
    };
    // The caller's real workspace, from the runtime's own projection. A session that the
    // runtime does not know is refused rather than being answered with an empty table,
    // because an empty table for an unknown session would read as "nothing is running".
    let workspace_id = projection
        .sessions
        .iter()
        .find(|entry| entry.session_id == session)
        .map(|entry| entry.workspace_id)
        .context("agent.session_unknown")?;

    let mut visibility = ActivityVisibility::for_session(
        services.activity_runtime_instance_id,
        session,
        workspace_id,
    );
    // The caller's own named targets, and only those. They are keys into the index's own
    // physical-identity space, so a caller cannot invent one for a target it has never
    // legitimately named: the key has to match a fact a real publisher recorded.
    //
    // R4: these are **lookup hints, not grants**, and they are bounded. They let the reader's own
    // justified targets participate in association; they cannot widen what it may see, because
    // `may_see` still decides, and an over-long or malformed hint is refused rather than accepted
    // as a key.
    anyhow::ensure!(
        input.relevant_targets.len() <= aura_activity_domain::MAX_ACTIVITY_RELEVANT_HINTS,
        "activity.request_hints_over_limit"
    );
    for target in &input.relevant_targets {
        anyhow::ensure!(target.is_within_request_bounds(), "activity.request_hint_over_limit");
    }
    visibility.relevant_resources = input.relevant_targets.clone();
    visibility.relevant_targets =
        input.relevant_targets.iter().map(ActivityResourceKey::of).collect();
    // R4: a caller may NOT widen its own visibility by asking a broad question. The query type is
    // not an authorisation, so this stays false and `may_see` admits another session's row only
    // through a target the caller really reached, or as the minimum permitted fact for an
    // unprovable write set.

    // One held snapshot, then one page of it. The snapshot is taken here and only here,
    // so the page, its coverage block and its revision all describe one instant.
    let result = aura_agent_execution::agent_activity::query::page(
        &services.activity_index,
        visibility,
        input.query.clone(),
        input.expected_revision,
        input.offset,
        input.effective_limit(),
        false,
    )?;
    Ok(serde_json::to_value(ActivityQueryResponseV1 {
        contract_version: ACTIVITY_QUERY_CONTRACT_VERSION,
        result,
    })?)
}
