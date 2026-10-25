//! Approval and result helpers for package scenarios. Installs come from the
//! package service's own test support: import, approved Conformance, install.
use super::*;
pub(super) use crate::package_services::test_support::{Installed, install, set_enabled};

/// The backend a package call really runs on, as selection would name it.
fn backend(f: &Fixture, call: &CallContext) -> aura_execution_contract::BackendDescriptor {
    let engine = f.service().agent_execution_required();
    if crate::agent_execution::package_tools::program::is_program(&call.operation.content) {
        crate::agent_execution::package_tools::program::native_descriptor(engine.runtime)
    } else {
        broker::descriptor(engine.runtime)
    }
}

/// The question the policy asks for this call, or `None` when the mode covers it.
pub(super) fn question(
    f: &Fixture,
    call: &CallContext,
) -> Option<aura_security_contract::AuthorizationProposal> {
    let engine = f.service().agent_execution_required();
    let policy = call.policy(UtcTimestamp::now(), engine.runtime);
    match aura_permission_engine::policy::authorize(&policy, &backend(f, call), None) {
        PolicyAuthorization::Ask { proposal } => Some(proposal),
        _ => None,
    }
}

/// Answers the real question with `decision`, through the production resolver.
pub(super) fn answer(
    f: &Fixture,
    call: &CallContext,
    decision: ApprovalDecisionKind,
) -> AuthorizationGrant {
    let engine = f.service().agent_execution_required();
    let backend = backend(f, call);
    let proposal = question(f, call).expect("the call asks");
    let now = UtcTimestamp::now();
    let request = AuthorizationApproval {
        approval_id: ApprovalRequestId::new(),
        revision: Revision::INITIAL,
        scope: proposal.scope,
        allowed_decisions: proposal.allowed_decisions,
        state: ApprovalState::Pending,
        requested_at: now,
        expires_at: UtcTimestamp::from_value(now.value() + time::Duration::minutes(10)),
    };
    f.service()
        .broker
        .lock()
        .expect("approval journal")
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::security::insert_approval(tx, &request)?;
            aura_persistence_v2::security_approval::resolve(
                tx,
                request.approval_id,
                request.revision,
                &call.operation,
                decision,
                backend.class,
                call.snapshot.session_permission_revision,
                engine.runtime,
                now,
            )
        })
        .expect("persist the answer")
        .expect("an allow decision issues a grant")
}

/// The durable result of one call.
pub(super) fn result(f: &Fixture, id: ToolCallId) -> ToolResultV1 {
    let store = f.service().broker.lock().expect("journal");
    aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("call exists")
        .result
        .expect("terminal result")
}

/// What the session row shows for one call: its brief line and its expandable details.
pub(super) fn activity(f: &Fixture, id: ToolCallId) -> (String, Vec<String>) {
    let store = f.service().broker.lock().expect("activity");
    let page = aura_persistence_v2::tool::activity::page(
        store.database.connection(),
        &aura_tool_projection::AgentToolActivityRequest {
            session_id: f.turn.session_id,
            before_call_id: None,
            limit: 20,
            call_ids: Some(vec![id]),
        },
    )
    .expect("activity page");
    let call = page.calls.into_iter().next().expect("the call's row");
    (call.display.summary, call.display.details)
}

pub(super) fn attempted(f: &Fixture, id: ToolCallId) -> bool {
    let store = f.service().broker.lock().expect("journal");
    aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("call exists")
        .attempt_id
        .is_some()
}
