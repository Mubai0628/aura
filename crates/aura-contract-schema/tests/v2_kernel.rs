// Fixture JSON is built with `serde_json::to_value` on values whose serialization
// cannot fail; an `expect` message would only restate that.
#![allow(clippy::unwrap_used)]

use aura_contract_schema::{V2KernelFixture, v2_kernel_fixture};
use aura_domain_common::{Revision, RuntimeInstanceId};
use aura_execution_contract::*;
use aura_security_contract::*;

fn authorize(f: &V2KernelFixture) -> Result<AuthorizedDispatch, ExecutionContractError> {
    AuthorizedDispatch::revalidate(
        f.dispatch.clone(),
        &f.authority,
        &f.dispatch.plan.prepared.backend,
        &f.cancellation,
        f.authority.decided_at,
    )
}

#[test]
fn generated_fixture_round_trips_and_checks_the_complete_dispatch() {
    let f = v2_kernel_fixture().expect("fixture");
    let decoded: V2KernelFixture =
        serde_json::from_value(serde_json::to_value(&f).expect("wire")).expect("round trip");
    assert_eq!(decoded, f);
    assert!(authorize(&f).is_ok());
    f.result.validate().expect("succeeded with explicit unknown side effect");
}

#[test]
fn changed_content_including_credentials_cannot_reuse_an_operation_fingerprint() {
    let f = v2_kernel_fixture().expect("fixture");
    for (pointer, replacement) in [
        ("/content/action/path", serde_json::json!("different.txt")),
        ("/content/action_revision", serde_json::json!("2")),
        ("/content/action_kind", serde_json::json!("fixture.write")),
        ("/content/credentials/0/credential_revision", serde_json::json!("2")),
        ("/content/credentials/0/use_fingerprint/hex", serde_json::json!("a".repeat(64))),
        ("/content/source/source_kind", serde_json::json!("system_component")),
        ("/content/purpose", serde_json::json!("another purpose")),
        (
            "/content/resources/0/workspace_id",
            serde_json::json!(aura_domain_common::WorkspaceId::new()),
        ),
        ("/content/capabilities/0", serde_json::json!("process_execute")),
    ] {
        let mut value = serde_json::to_value(&f.dispatch.operation).expect("operation wire");
        *value.pointer_mut(pointer).expect(pointer) = replacement;
        assert!(serde_json::from_value::<SecuredOperation>(value).is_err(), "{pointer}");
    }
}

#[test]
fn expired_revoked_consumed_or_changed_authority_and_cancellation_are_rejected() {
    let f = v2_kernel_fixture().expect("fixture");
    assert!(
        AuthorizedDispatch::revalidate(
            f.dispatch.clone(),
            &f.authority,
            &f.dispatch.plan.prepared.backend,
            &f.cancellation,
            f.authority.expires_at
        )
        .is_err()
    );
    for revoke in [true, false] {
        let mut grant = f.grant.clone();
        if revoke {
            grant.revoked_at = Some(f.authority.decided_at);
        } else {
            grant.consumed_at = Some(f.authority.decided_at);
        }
        assert!(
            AuthorizationDecision::from_grant(
                &grant,
                f.authority.decided_at,
                f.authority.expires_at
            )
            .is_none()
        );
    }
    let mut changed = f.clone();
    changed.grant.revision = Revision::new(2);
    changed.authority = AuthorizationDecision::from_grant(
        &changed.grant,
        f.authority.decided_at,
        f.authority.expires_at,
    )
    .expect("new grant revision");
    assert!(authorize(&changed).is_err());
    let mut changed = f.clone();
    changed.cancellation.requested = true;
    assert!(authorize(&changed).is_err());
    changed.cancellation.requested = false;
    changed.cancellation.revision = Revision::new(2);
    assert!(authorize(&changed).is_err());
    let mut changed = f.clone();
    changed.dispatch.binding.attempt.operation_id = aura_domain_common::SecurityOperationId::new();
    assert!(authorize(&changed).is_err());
}

#[test]
fn equivalent_backend_rebind_preserves_content_and_requires_a_new_exact_plan_binding() {
    let mut f = v2_kernel_fixture().expect("fixture");
    let fingerprint = f.dispatch.operation.fingerprint.clone();
    let mut backend = f.dispatch.plan.prepared.backend.clone();
    backend.id = BackendId("fixture.offline".into());
    backend.runtime_instance_id = RuntimeInstanceId::new();
    backend.class = BackendClass::RestrictedUserOffline;
    backend.guarantees.insert(EnforcementGuarantee::OfflineNetwork);
    assert!(
        AuthorizedDispatch::revalidate(
            f.dispatch.clone(),
            &f.authority,
            &backend,
            &f.cancellation,
            f.authority.decided_at
        )
        .is_err()
    );
    let mut prepared = f.dispatch.plan.prepared.clone();
    prepared.backend = backend;
    f.dispatch.plan = ExecutionPlan::new(prepared).expect("reprepare");
    assert!(authorize(&f).is_err());
    f.dispatch.binding.plan_fingerprint = f.dispatch.plan.fingerprint.clone();
    assert!(authorize(&f).is_ok());
    assert_eq!(fingerprint, f.dispatch.operation.fingerprint);
    f.dispatch.plan.prepared.backend.guarantees.remove(&EnforcementGuarantee::AuthorizedWritesOnly);
    f.dispatch.plan = ExecutionPlan::new(f.dispatch.plan.prepared.clone()).expect("weaker plan");
    f.dispatch.binding.plan_fingerprint = f.dispatch.plan.fingerprint.clone();
    assert!(authorize(&f).is_err());
}

#[test]
fn native_is_never_authorized_by_a_mode_or_workspace_lifetime() {
    let mut f = v2_kernel_fixture().expect("fixture");
    f.dispatch.plan.prepared.backend.class = BackendClass::Native;
    f.dispatch.plan = ExecutionPlan::new(f.dispatch.plan.prepared.clone()).expect("native plan");
    f.dispatch.binding.plan_fingerprint = f.dispatch.plan.fingerprint.clone();
    assert!(authorize(&f).is_err());
    f.authority.scope.backend_requirement.allowed_classes.insert(BackendClass::Native);
    f.dispatch.binding.authorization_fingerprint = f.authority.fingerprint().expect("authority");
    assert!(authorize(&f).is_ok());
    f.authority.scope.lifetime = AuthorizationLifetime::Workspace {
        workspace_id: f.dispatch.operation.content.identity.workspace_id.expect("workspace"),
    };
    f.dispatch.binding.authorization_fingerprint = f.authority.fingerprint().expect("authority");
    assert!(authorize(&f).is_err());
    f.authority.scope.lifetime =
        AuthorizationLifetime::Once { operation_id: f.dispatch.operation.operation_id };
    f.authority.basis = AuthorizationBasis::PermissionMode {
        session_permission_revision: 1,
        snapshot_id: aura_domain_common::PermissionSnapshotId::new(),
        mode: PermissionMode::FullAccess,
    };
    f.dispatch.binding.authorization_fingerprint = f.authority.fingerprint().expect("authority");
    assert!(authorize(&f).is_err());
}

#[test]
fn attempt_has_one_terminal_result_and_unknown_does_not_become_confirmed_cancellation() {
    let f = v2_kernel_fixture().expect("fixture");
    let mut attempt = ExecutionAttempt {
        binding: f.dispatch.binding.clone(),
        prepared_at: f.authority.decided_at,
        dispatched_at: None,
        state: AttemptState::Prepared,
    };
    assert!(attempt.complete(f.result.clone()).is_err());
    attempt.mark_dispatched(f.authority.decided_at).expect("dispatch");
    let mut invalid = f.result.clone();
    invalid.warnings.clear();
    assert!(attempt.complete(invalid).is_err());
    let mut invalid = f.result.clone();
    invalid.outcome = ExecutionOutcome::Cancelled;
    assert!(attempt.complete(invalid).is_err());
    let mut invalid = f.result.clone();
    invalid.receipt.attempt.ordinal = std::num::NonZeroU32::new(2).expect("ordinal");
    assert!(attempt.complete(invalid).is_err());
    attempt.complete(f.result.clone()).expect("unknown side effect retained");
    let terminal = attempt.clone();
    assert!(attempt.complete(f.result).is_err());
    assert!(attempt.mark_dispatched(f.authority.decided_at).is_err());
    assert_eq!(attempt, terminal);
}

#[test]
fn protected_path_identity_keeps_legacy_fingerprints_and_binds_a_verified_object() {
    use aura_domain_common::jcs::canonical_sha256;
    let f = v2_kernel_fixture().expect("fixture");
    let path = "C:/synthetic-review/old.txt";
    let canonical_path = serde_json::json!({
        "display": path,
        "utf16": path.encode_utf16().collect::<Vec<u16>>(),
    });
    let identity = aura_domain_common::FileIdentity {
        volume_serial_number: "volume".into(),
        file_id: "object".into(),
    };
    let parent = aura_domain_common::FileIdentity {
        volume_serial_number: "volume".into(),
        file_id: "parent".into(),
    };
    for kind in ["system_path", "sensitive_user_path"] {
        let mut legacy = serde_json::to_value(&f.dispatch.operation).expect("operation wire");
        legacy["content"]["resources"] = serde_json::json!([{
            "scope_kind": kind,
            "canonical_path": canonical_path,
        }]);
        let fingerprint = OperationFingerprint(canonical_sha256(&legacy["content"]).expect("hash"));
        legacy["fingerprint"] = serde_json::to_value(&fingerprint).expect("fingerprint");
        let decoded = serde_json::from_value::<SecuredOperation>(legacy.clone()).expect(kind);
        let wire = serde_json::to_value(&decoded.content.resources[0]).expect("resource");
        assert!(wire.get("identity").is_none(), "{kind} omits unbound identity");
        assert!(wire.get("parent_identity").is_none(), "{kind} omits unbound parent");

        let mut intermediate = legacy.clone();
        intermediate["content"]["resources"][0]["identity"] = serde_json::Value::Null;
        let null_fingerprint =
            OperationFingerprint(canonical_sha256(&intermediate["content"]).expect("null hash"));
        intermediate["fingerprint"] = serde_json::to_value(&null_fingerprint).expect("fingerprint");
        serde_json::from_value::<SecuredOperation>(intermediate).expect("explicit null identity");

        let mut bound = legacy.clone();
        bound["content"]["resources"][0]["identity"] = serde_json::to_value(&identity).unwrap();
        bound["content"]["resources"][0]["parent_identity"] =
            serde_json::to_value(&parent).unwrap();
        let bound_fingerprint =
            OperationFingerprint(canonical_sha256(&bound["content"]).expect("bound hash"));
        assert_ne!(bound_fingerprint, fingerprint);
        bound["fingerprint"] = serde_json::to_value(&bound_fingerprint).expect("fingerprint");
        let decoded = serde_json::from_value::<SecuredOperation>(bound).expect("bound identity");
        match &decoded.content.resources[0] {
            ResourceDescriptor::SystemPath {
                identity: Some(found),
                parent_identity: Some(found_parent),
                ..
            }
            | ResourceDescriptor::SensitiveUserPath {
                identity: Some(found),
                parent_identity: Some(found_parent),
                ..
            } => {
                assert_eq!(found, &identity);
                assert_eq!(found_parent, &parent);
            }
            other => panic!("verified identity was not kept: {other:?}"),
        }

        let mut wrong_fingerprint = legacy.clone();
        wrong_fingerprint["fingerprint"] =
            serde_json::to_value(&bound_fingerprint).expect("fingerprint");
        assert!(
            serde_json::from_value::<SecuredOperation>(wrong_fingerprint).is_err(),
            "{kind} rejects a fingerprint that belongs to a different identity"
        );

        let mut wrong_identity = legacy.clone();
        wrong_identity["content"]["resources"][0]["identity"] =
            serde_json::to_value(&identity).unwrap();
        assert!(
            serde_json::from_value::<SecuredOperation>(wrong_identity).is_err(),
            "{kind} rejects a body that no longer matches its fingerprint"
        );
    }
}
