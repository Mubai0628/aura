//! Core-owned V2 dispatch reservation. R05/R06 execute the returned dispatch;
//! this control plane never starts a runner or migrates the legacy database.
use aura_execution_contract::{
    AuthorizedDispatch, BackendDescriptor, CancellationState, DispatchRequest,
};
use aura_permission_engine::{dispatch_guard, policy::PolicyInput};
use aura_persistence_core::{
    PersistenceCoreError as Error, StoreResult,
    operation::{self, BeginOperation, OperationDraft},
};
use aura_persistence_v2::{V2Store, execution, security};
use aura_security_contract::AuthorizationDecision;

/// All facts in `current` and `cancellation` come from Core domain resolvers;
/// neither this entry point nor its authorization token is an IPC DTO. The
/// transaction reloads authority and refuses a duplicate even with a new ordinal.
// Each argument is a distinct live authority input; keep the store and mode
// authority explicit while the Orchestrator owns their eventual assembly.
#[allow(clippy::too_many_arguments)]
pub fn reserve_dispatch(
    permission_authority: &aura_persistence::PersistenceCoordinator,
    store: &mut V2Store,
    request: DispatchRequest,
    authority: &AuthorizationDecision,
    current: &PolicyInput<'_>,
    backend: &BackendDescriptor,
    cancellation: &CancellationState,
    request_key: &str,
) -> StoreResult<AuthorizedDispatch> {
    reserve(
        permission_authority,
        store,
        request,
        authority,
        current,
        backend,
        cancellation,
        request_key,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn reserve_tool_dispatch(
    permission_authority: &aura_persistence::PersistenceCoordinator,
    store: &mut V2Store,
    request: DispatchRequest,
    authority: &AuthorizationDecision,
    current: &PolicyInput<'_>,
    backend: &BackendDescriptor,
    cancellation: &CancellationState,
    call: &aura_persistence_v2::tool::execution::CallReservation,
) -> StoreResult<AuthorizedDispatch> {
    reserve(
        permission_authority,
        store,
        request,
        authority,
        current,
        backend,
        cancellation,
        &format!("tool:{}", call.call_id),
        Some(call),
    )
}

#[allow(clippy::too_many_arguments)]
fn reserve(
    permission_authority: &aura_persistence::PersistenceCoordinator,
    store: &mut V2Store,
    request: DispatchRequest,
    authority: &AuthorizationDecision,
    current: &PolicyInput<'_>,
    backend: &BackendDescriptor,
    cancellation: &CancellationState,
    request_key: &str,
    call: Option<&aura_persistence_v2::tool::execution::CallReservation>,
) -> StoreResult<AuthorizedDispatch> {
    permission_authority
        .with_session_permission_authority(current.snapshot.session_id, |mode| {
            if mode.mode != current.snapshot.mode
                || mode.revision != current.snapshot.session_permission_revision
            {
                let c = &current.operation.content;
                let explicit = matches!(
                    authority.basis,
                    aura_security_contract::AuthorizationBasis::Grant { .. }
                ) && authority.scope.consent.is_none();
                if !explicit
                    && !aura_permission_engine::mode_ceiling::permits(
                        c.identity.actor,
                        mode.mode,
                        &c.capabilities,
                        c.network != aura_execution_contract::OperationNetwork::None,
                        c.action_kind.starts_with("web."),
                    )
                {
                    return Err(Error::Conflict);
                }
            }
            let live_permission_revision = mode.revision;
            store.database.unit_of_work(|tx| {
                reserve_in_transaction(
                    tx,
                    request,
                    authority,
                    current,
                    backend,
                    cancellation,
                    request_key,
                    call,
                    live_permission_revision,
                )
            })
        })
        .map_err(|_| Error::Conflict)?
}

/// Caller holds the same session mode reader barrier for this entire owning
/// transaction. Atomic Agent control groups share reservation with normal
/// dispatch; they do not implement a second authorization/Attempt protocol.
#[allow(clippy::too_many_arguments)]
pub(crate) fn reserve_in_transaction(
    tx: &aura_persistence_core::Transaction<'_>,
    request: DispatchRequest,
    authority: &AuthorizationDecision,
    current: &PolicyInput<'_>,
    backend: &BackendDescriptor,
    cancellation: &CancellationState,
    request_key: &str,
    call: Option<&aura_persistence_v2::tool::execution::CallReservation>,
    live_permission_revision: u64,
) -> StoreResult<AuthorizedDispatch> {
    // The owning mode barrier already checked fresh default/explicit authority.
    // A newer mode snapshot must not discard an otherwise permitted prepared call.
    security::validate_recorded_snapshot(tx, current.snapshot)?;
    // Exact-operation Grants plus, for this session only, any reusable Native
    // session consent. A guard that runs without the second set would refuse a
    // call the user already approved; a guard that runs with it for another
    // session would borrow an approval nobody gave.
    let grants = security::grants_for_call(
        tx,
        &current.operation.fingerprint,
        current.operation.content.identity.session_id,
    )?;
    let mut live = current.clone();
    live.grants = &grants;
    if let Some(call) = call {
        aura_persistence_v2::agent::checkpoint::verify_live_call(tx, &call.batch, call.call_id)?;
    }
    let checked = dispatch_guard::revalidate(
        request,
        authority,
        current.operation,
        &live,
        backend,
        cancellation,
    )
    .map_err(|_| Error::Conflict)?;
    let operation = &checked.request().operation;
    let payload = serde_json::to_value(operation)?;
    if operation::begin(
        tx,
        &OperationDraft {
            id: operation.operation_id,
            domain: "execution",
            kind: &operation.content.action_kind,
            request_key,
            version: 1,
            payload: &payload,
            at: current.now,
        },
    )? != BeginOperation::Created
    {
        return Err(Error::Conflict);
    }
    execution::prepare(tx, &checked, authority, live_permission_revision, current.now)?;
    if let Some(call) = call {
        aura_persistence_v2::tool::execution::bind_attempt(tx, call, &checked)?;
    }
    Ok(checked)
}
