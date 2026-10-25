//! Store re-checks the original sealed decision, even if its grant changes after the guard.
use super::*;
use aura_execution_contract::AuthorizedDispatch;

fn checked(
    f: &Fixture,
    op: &SecuredOperation,
    authority: &AuthorizationDecision,
) -> AuthorizedDispatch {
    let backend = backend(f);
    let plan = ExecutionPlan::new(PreparedExecution {
        operation_fingerprint: op.fingerprint.clone(),
        backend: backend.clone(),
        preparation: serde_json::json!({"fixture":"network-store-boundary"}),
    })
    .expect("valid prepared plan");
    let request = DispatchRequest {
        operation: op.clone(),
        binding: DispatchBinding {
            attempt: AttemptRef {
                operation_id: op.operation_id,
                ordinal: std::num::NonZeroU32::MIN,
            },
            operation_fingerprint: op.fingerprint.clone(),
            authorization_fingerprint: authority.fingerprint().expect("authority fingerprint"),
            plan_fingerprint: plan.fingerprint.clone(),
            cancellation_revision: Revision::INITIAL,
        },
        plan,
    };
    let snapshot = security::snapshot(
        f.store.database.connection(),
        op.content.identity.permission_snapshot_id.expect("bound snapshot"),
    )
    .expect("read snapshot")
    .expect("stored snapshot");
    let grants =
        security::grants_for_call(f.store.database.connection(), &op.fingerprint, Some(f.session))
            .expect("grant candidates");
    aura_permission_engine::dispatch_guard::revalidate(
        request,
        authority,
        op,
        &PolicyInput {
            network_caller: NetworkCallerKind::AuraTool,
            operation: op,
            snapshot: &snapshot,
            profile: SecurityProfile::FullAccess,
            source_enabled: true,
            trust: &TrustState::SystemTrusted,
            source_fingerprint: None,
            hard_boundaries_satisfied: true,
            denied_for_turn: false,
            grants: &grants,
            verified_external_capabilities: &[],
            verified_external_grants: &[],
            now: f.at,
            expires_at: authority.expires_at,
            core_runtime_instance_id: Some(f.runtime),
        },
        &backend,
        &CancellationState { revision: Revision::INITIAL, requested: false },
    )
    .expect("guard approves the original authority")
}

#[test]
fn s07_n7_store_rechecks_network_grant_after_the_dispatch_guard() {
    for change in
        ["unchanged", "principal", "grant_revision", "expired", "revoked", "deleted_session"]
    {
        let mut f = Fixture::new(PermissionMode::ReadOnly);
        let op = operation(&mut f);
        let mut grant = approve(&mut f, &op, ApprovalDecisionKind::AllowSession);
        let PolicyAuthorization::Allow { authorization } = decision(&f, &op) else {
            panic!("original consent authorizes the request")
        };
        let dispatch = checked(&f, &op, &authorization);
        match change {
            "principal" => {
                grant.scope.network_consent.as_mut().expect("consent").principal_id =
                    PrincipalId::new()
            }
            "grant_revision" => {
                grant.scope.network_consent.as_mut().expect("consent").grant_revision =
                    Revision::new(99)
            }
            "expired" => grant.expires_at = Some(f.at),
            "revoked" => grant.revoked_at = Some(f.at),
            _ => {}
        }
        let result = f.store.database.unit_of_work(|tx| {
            aura_persistence_core::operation::begin(
                tx,
                &aura_persistence_core::operation::OperationDraft {
                    id: op.operation_id,
                    domain: "execution",
                    kind: "http.request",
                    request_key: &op.operation_id.to_string(),
                    version: 1,
                    payload: &serde_json::to_value(&op)?,
                    at: f.at,
                },
            )?;
            tx.execute(
                "UPDATE authorization_grants SET payload=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&grant)?, grant.grant_id.to_string()],
            )?;
            if change == "deleted_session" {
                tx.execute("DELETE FROM sessions WHERE id=?1", [f.session.to_string()])?;
            }
            aura_persistence_v2::execution::prepare(
                tx,
                &dispatch,
                &authorization,
                f.snapshot.session_permission_revision,
                f.at,
            )
        });
        if change == "unchanged" {
            assert!(result.is_ok(), "the direct Store path must also accept valid authority");
        } else {
            assert!(result.is_err(), "{change} cannot pass the Store despite a prior valid guard");
        }
        assert_eq!(
            f.store
                .database
                .connection()
                .query_row("SELECT count(*) FROM execution_attempts", [], |row| row
                    .get::<_, i64>(0))
                .expect("reserved attempt count"),
            i64::from(change == "unchanged")
        );
    }
}
