use aura_domain_common::CanonicalSemVer;
use aura_domain_common::ToolPackageId;
use aura_domain_common::jcs::CanonicalSha256HashV1;
use aura_domain_common::{
    AgentRunId, ContentHash, FileReferenceId, PermissionGrantId, UtcTimestamp,
};
use aura_permission_domain::{
    NativeSandboxMode, NetworkAccessRequirement, OperationSecurityPlan, PackageRuntimeScopeV1,
    PermissionDecision, PermissionGrant, ResolvedScope, TurnDenyRecord,
};
use aura_permission_engine::{
    DefaultPermissionEvaluator, PermissionEvaluation, PermissionEvaluator, matching_grant,
};
use aura_security_contract::TrustState;
use aura_security_contract::{
    ApprovalDecisionKind, InvocationActor, OperationRiskFlag, PermissionGrantLifetime,
    PermissionMode, PermissionSnapshot, ResolvedCapability,
};
use aura_tool_contract::PackageRuntimeClosureHashV1;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PermissionMatrixFixture {
    schema_version: u32,
    workspace_file_write: Vec<WorkspaceWriteFixture>,
    full_access_high_risk: Vec<HighRiskFixture>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceWriteFixture {
    mode: PermissionMode,
    expected_decision: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HighRiskFixture {
    risk: OperationRiskFlag,
    expected_decision: String,
}

mod support;
use support::fixture;

fn evaluate(
    plan: &OperationSecurityPlan,
    snapshot: &PermissionSnapshot,
    grants: &[PermissionGrant],
    denies: &[TurnDenyRecord],
) -> PermissionDecision {
    DefaultPermissionEvaluator.evaluate(&PermissionEvaluation {
        plan,
        snapshot: snapshot.clone(),
        source_enabled: true,
        trust_state: &TrustState::SystemTrusted,
        grants,
        turn_denies: denies,
        available_sandbox_modes: &[
            NativeSandboxMode::CompatibilityAppContainer,
            NativeSandboxMode::StrictAppContainer,
            NativeSandboxMode::ObservedNative,
            NativeSandboxMode::ObservedNativeNetworked,
            NativeSandboxMode::ElevatedBrokered,
        ],
        network_control_available: true,
        allow_unrestricted_inline_scripts_in_full_access: false,
        now: UtcTimestamp::now(),
    })
}

fn decision_name(decision: &PermissionDecision) -> &'static str {
    match decision {
        PermissionDecision::Allow { .. } => "allow",
        PermissionDecision::Ask { .. } => "ask",
        PermissionDecision::Deny { .. } => "deny",
    }
}

fn permission_matrix_fixture() -> PermissionMatrixFixture {
    serde_json::from_str(include_str!("../../../fixtures/security/permission-matrix.json"))
        .expect("permission matrix fixture must stay compatible with the canonical contract")
}

#[test]
fn workspace_write_matrix_matches_three_modes() {
    let fixture_cases = permission_matrix_fixture();
    assert_eq!(fixture_cases.schema_version, 2);
    for case in fixture_cases.workspace_file_write {
        let (plan, snapshot) = fixture(case.mode);
        assert_eq!(decision_name(&evaluate(&plan, &snapshot, &[], &[])), case.expected_decision);
    }
}

#[test]
fn full_access_high_risk_fixture_never_bypasses_approval() {
    for case in permission_matrix_fixture().full_access_high_risk {
        let (mut plan, snapshot) = fixture(PermissionMode::FullAccess);
        plan.risk_flags = vec![case.risk];
        assert_eq!(decision_name(&evaluate(&plan, &snapshot, &[], &[])), case.expected_decision);
    }
}

#[test]
fn full_access_cannot_persistently_override_high_risk() {
    let (mut plan, snapshot) = fixture(PermissionMode::FullAccess);
    plan.risk_flags = vec![OperationRiskFlag::Elevation];
    plan.capabilities = vec![ResolvedCapability::ElevatedOperation];
    let PermissionDecision::Ask { request } = evaluate(&plan, &snapshot, &[], &[]) else {
        panic!("elevation must ask");
    };
    assert_eq!(
        request.allowed_decisions,
        vec![ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny]
    );
}

#[test]
fn exact_once_grant_allows_the_approved_high_risk_operation_only() {
    let (mut plan, mut snapshot) = fixture(PermissionMode::FullAccess);
    plan.risk_flags = vec![OperationRiskFlag::Elevation];
    plan.capabilities = vec![ResolvedCapability::ElevatedOperation];
    let once = PermissionGrant {
        id: PermissionGrantId::new(),
        lifetime: PermissionGrantLifetime::Once,
        workspace_id: snapshot.workspace_id,
        session_id: Some(snapshot.session_id),
        turn_task_id: plan.identity.turn_task_id,
        source: plan.source.clone(),
        source_fingerprint: None,
        capabilities: plan.capabilities.clone(),
        scopes: plan.scopes.clone(),
        approved_risk_flags: plan.risk_flags.clone(),
        operation_fingerprint: Some(plan.operation_fingerprint.clone()),
        created_at: UtcTimestamp::now(),
        expires_at: None,
        revoked_at: None,
        consumed_at: None,
    };
    snapshot.active_grant_ids.push(once.id);
    assert!(matches!(evaluate(&plan, &snapshot, &[once], &[]), PermissionDecision::Allow { .. }));

    let session = PermissionGrant {
        id: PermissionGrantId::new(),
        lifetime: PermissionGrantLifetime::Session,
        workspace_id: snapshot.workspace_id,
        session_id: Some(snapshot.session_id),
        turn_task_id: None,
        source: plan.source.clone(),
        source_fingerprint: None,
        capabilities: plan.capabilities.clone(),
        scopes: plan.scopes.clone(),
        approved_risk_flags: plan.risk_flags.clone(),
        operation_fingerprint: None,
        created_at: UtcTimestamp::now(),
        expires_at: None,
        revoked_at: None,
        consumed_at: None,
    };
    snapshot.active_grant_ids = vec![session.id];
    assert!(matches!(evaluate(&plan, &snapshot, &[session], &[]), PermissionDecision::Ask { .. }));
}

#[test]
fn package_runtime_grant_requires_exact_package_version_and_closure() {
    let (mut plan, mut snapshot) = fixture(PermissionMode::FullAccess);
    plan.capabilities = vec![ResolvedCapability::ProcessExecute];
    plan.scopes = vec![ResolvedScope::PackageRuntime {
        package: PackageRuntimeScopeV1 {
            tool_package_id: ToolPackageId::new(),
            package_version: CanonicalSemVer::new("1.2.3").expect("version"),
            runtime_closure_hash: PackageRuntimeClosureHashV1(CanonicalSha256HashV1::sha256(
                b"closure one",
            )),
        },
    }];
    let grant = PermissionGrant {
        id: PermissionGrantId::new(),
        lifetime: PermissionGrantLifetime::Session,
        workspace_id: snapshot.workspace_id,
        session_id: Some(snapshot.session_id),
        turn_task_id: None,
        source: plan.source.clone(),
        source_fingerprint: plan.source_fingerprint.clone(),
        capabilities: plan.capabilities.clone(),
        scopes: plan.scopes.clone(),
        approved_risk_flags: plan.risk_flags.clone(),
        operation_fingerprint: None,
        created_at: UtcTimestamp::now(),
        expires_at: None,
        revoked_at: None,
        consumed_at: None,
    };
    snapshot.active_grant_ids.push(grant.id);
    let grants = [grant];
    let input = PermissionEvaluation {
        plan: &plan,
        snapshot: snapshot.clone(),
        source_enabled: true,
        trust_state: &TrustState::SystemTrusted,
        grants: &grants,
        turn_denies: &[],
        available_sandbox_modes: &[],
        network_control_available: true,
        allow_unrestricted_inline_scripts_in_full_access: false,
        now: UtcTimestamp::now(),
    };
    assert!(matching_grant(&input).is_some());

    let mut changed = plan.clone();
    let ResolvedScope::PackageRuntime { package } = &mut changed.scopes[0] else {
        panic!("Package Runtime scope");
    };
    package.runtime_closure_hash =
        PackageRuntimeClosureHashV1(CanonicalSha256HashV1::sha256(b"closure two"));
    let changed_input = PermissionEvaluation {
        plan: &changed,
        snapshot,
        source_enabled: true,
        trust_state: &TrustState::SystemTrusted,
        grants: &grants,
        turn_denies: &[],
        available_sandbox_modes: &[],
        network_control_available: true,
        allow_unrestricted_inline_scripts_in_full_access: false,
        now: UtcTimestamp::now(),
    };
    assert!(matching_grant(&changed_input).is_none());
}

/// A precise approval is what lets a ReadOnly session write; it is still exact.
///
/// The migrated half is the first assertion. It used to require `Deny { ModeDenied }` for a
/// ReadOnly plan carrying an exact once-grant, which encoded the mode as an unbreakable
/// ceiling and made the confirmed "a ReadOnly session may approve writing a specified
/// target" rule unreachable. The second half is unchanged and is the reason this test still
/// matters: the grant authorizes **only** the operation it was issued for, so "approvable"
/// never became "approvable for anything".
#[test]
fn exact_once_grant_authorizes_only_its_own_operation_in_a_lower_mode() {
    let (plan, mut snapshot) = fixture(PermissionMode::ReadOnly);
    let grant = PermissionGrant {
        id: PermissionGrantId::new(),
        lifetime: PermissionGrantLifetime::Once,
        workspace_id: snapshot.workspace_id,
        session_id: Some(snapshot.session_id),
        turn_task_id: plan.identity.turn_task_id,
        source: plan.source.clone(),
        source_fingerprint: None,
        capabilities: plan.capabilities.clone(),
        scopes: plan.scopes.clone(),
        approved_risk_flags: plan.risk_flags.clone(),
        operation_fingerprint: Some(plan.operation_fingerprint.clone()),
        created_at: UtcTimestamp::now(),
        expires_at: None,
        revoked_at: None,
        consumed_at: None,
    };
    snapshot.active_grant_ids.push(grant.id);
    let grants = [grant.clone()];
    let input = PermissionEvaluation {
        plan: &plan,
        snapshot: snapshot.clone(),
        source_enabled: true,
        trust_state: &TrustState::SystemTrusted,
        grants: &grants,
        turn_denies: &[],
        available_sandbox_modes: &[],
        network_control_available: true,
        allow_unrestricted_inline_scripts_in_full_access: false,
        now: UtcTimestamp::now(),
    };
    assert_eq!(matching_grant(&input), Some(grant.id));
    assert!(
        matches!(DefaultPermissionEvaluator.evaluate(&input), PermissionDecision::Allow { .. }),
        "the approved operation runs in ReadOnly; the mode is a default, not a ceiling"
    );

    let mut changed = plan.clone();
    changed.operation_fingerprint = ContentHash::blake3(b"changed");
    let changed_input = PermissionEvaluation { plan: &changed, ..input };
    assert_eq!(matching_grant(&changed_input), None);
    assert!(
        !matches!(
            DefaultPermissionEvaluator.evaluate(&changed_input),
            PermissionDecision::Allow { .. }
        ),
        "the same grant cannot authorize a different operation"
    );
}

#[test]
fn matching_grant_does_not_bypass_unavailable_explicit_sandbox_route() {
    let (mut plan, mut snapshot) = fixture(PermissionMode::FullAccess);
    plan.requested_sandbox = Some(NativeSandboxMode::StrictAppContainer);
    let grant = PermissionGrant {
        id: PermissionGrantId::new(),
        lifetime: PermissionGrantLifetime::Session,
        workspace_id: snapshot.workspace_id,
        session_id: Some(snapshot.session_id),
        turn_task_id: None,
        source: plan.source.clone(),
        source_fingerprint: plan.source_fingerprint.clone(),
        capabilities: plan.capabilities.clone(),
        scopes: plan.scopes.clone(),
        approved_risk_flags: plan.risk_flags.clone(),
        operation_fingerprint: None,
        created_at: UtcTimestamp::now(),
        expires_at: None,
        revoked_at: None,
        consumed_at: None,
    };
    snapshot.active_grant_ids.push(grant.id);
    let grants = [grant];
    let input = PermissionEvaluation {
        plan: &plan,
        snapshot,
        source_enabled: true,
        trust_state: &TrustState::SystemTrusted,
        grants: &grants,
        turn_denies: &[],
        available_sandbox_modes: &[],
        network_control_available: true,
        allow_unrestricted_inline_scripts_in_full_access: false,
        now: UtcTimestamp::now(),
    };

    assert!(matching_grant(&input).is_some());
    assert!(matches!(
        DefaultPermissionEvaluator.evaluate_with_authorization_basis(&input).decision,
        PermissionDecision::Deny {
            reason: aura_permission_domain::PermissionDenyReason::SandboxUnavailable
        }
    ));
}

#[test]
fn immutable_snapshot_does_not_see_a_grant_created_after_it() {
    let (plan, snapshot) = fixture(PermissionMode::ReadOnly);
    let grant = PermissionGrant {
        id: PermissionGrantId::new(),
        lifetime: PermissionGrantLifetime::Once,
        workspace_id: snapshot.workspace_id,
        session_id: Some(snapshot.session_id),
        turn_task_id: plan.identity.turn_task_id,
        source: plan.source.clone(),
        source_fingerprint: None,
        capabilities: plan.capabilities.clone(),
        scopes: plan.scopes.clone(),
        approved_risk_flags: plan.risk_flags.clone(),
        operation_fingerprint: Some(plan.operation_fingerprint.clone()),
        created_at: UtcTimestamp::now(),
        expires_at: None,
        revoked_at: None,
        consumed_at: None,
    };
    assert_eq!(
        matching_grant(&PermissionEvaluation {
            plan: &plan,
            snapshot,
            source_enabled: true,
            trust_state: &TrustState::SystemTrusted,
            grants: &[grant],
            turn_denies: &[],
            available_sandbox_modes: &[],
            network_control_available: true,
            allow_unrestricted_inline_scripts_in_full_access: false,
            now: UtcTimestamp::now(),
        }),
        None
    );
}

#[test]
fn session_authorized_external_file_read_is_allowed_in_read_only_mode() {
    let (mut plan, snapshot) = fixture(PermissionMode::ReadOnly);
    plan.capabilities = vec![ResolvedCapability::ExternalFileRead];
    plan.scopes =
        vec![ResolvedScope::SessionAuthorizedFile { file_reference_id: FileReferenceId::new() }];
    assert!(matches!(evaluate(&plan, &snapshot, &[], &[]), PermissionDecision::Allow { .. }));
}

#[test]
fn turn_deny_prevents_reprompt() {
    let (plan, snapshot) = fixture(PermissionMode::WorkspaceReadWrite);
    let deny = TurnDenyRecord {
        turn_task_id: plan.identity.turn_task_id.expect("turn"),
        source: plan.source.clone(),
        operation_fingerprint: plan.operation_fingerprint.clone(),
        denied_at: UtcTimestamp::now(),
    };
    assert!(matches!(
        evaluate(&plan, &snapshot, &[], &[deny]),
        PermissionDecision::Deny {
            reason: aura_permission_domain::PermissionDenyReason::DeniedForCurrentTurn
        }
    ));
}

#[test]
fn child_identity_and_model_purpose_cannot_be_forged_away() {
    let (mut plan, snapshot) = fixture(PermissionMode::FullAccess);
    plan.identity.actor = InvocationActor::ChildAgent;
    plan.identity.parent_agent_run_id = None;
    assert!(matches!(
        evaluate(&plan, &snapshot, &[], &[]),
        PermissionDecision::Deny {
            reason: aura_permission_domain::PermissionDenyReason::IdentityInvalid
        }
    ));

    plan.identity.parent_agent_run_id = Some(AgentRunId::new());
    plan.purpose = Some("  ".to_owned());
    assert!(matches!(
        evaluate(&plan, &snapshot, &[], &[]),
        PermissionDecision::Deny {
            reason: aura_permission_domain::PermissionDenyReason::PurposeRequired
        }
    ));
}

/// Adding risk never increases permission, and lowering the mode never grants one.
///
/// The invariant this test exists for is the *monotonicity*, not any particular decision,
/// so it is expressed against the risk and mode change rather than a fixed answer:
///
/// * adding a one-shot risk flag turns an allow into an approval request, and
/// * lowering the mode can never turn an approval request into an allow, and never turns a
///   refusal into an approval request.
///
/// The migrated half is the ReadOnly case. It used to require `Deny { ModeDenied }`, which
/// encoded the mode as an unbreakable ceiling. Under the confirmed semantics the correct
/// statement is the weaker and more useful one — ReadOnly does not *allow* the write on its
/// own — so the assertion is that the outcome is not `Allow`, which is what actually has to
/// hold whichever of Ask the flow produces.
#[test]
fn adding_risk_and_lowering_mode_never_increases_permission() {
    let (base, full) = fixture(PermissionMode::FullAccess);
    assert!(matches!(evaluate(&base, &full, &[], &[]), PermissionDecision::Allow { .. }));

    let mut risky = base.clone();
    risky.risk_flags.push(OperationRiskFlag::Elevation);
    assert!(matches!(evaluate(&risky, &full, &[], &[]), PermissionDecision::Ask { .. }));

    let mut read_only = full.clone();
    read_only.mode = PermissionMode::ReadOnly;
    let lowered = evaluate(&base, &read_only, &[], &[]);
    assert!(
        !matches!(lowered, PermissionDecision::Allow { .. }),
        "ReadOnly does not authorize a workspace write on its own; it asks for one"
    );
    assert!(
        matches!(lowered, PermissionDecision::Ask { .. }),
        "and what it produces is a request the user can answer, not a refusal"
    );
}

#[test]
fn network_capability_requires_an_explicit_required_declaration() {
    let (mut plan, snapshot) = fixture(PermissionMode::WorkspaceReadWrite);
    plan.capabilities = vec![ResolvedCapability::PublicNetworkRead];
    assert!(matches!(
        evaluate(&plan, &snapshot, &[], &[]),
        PermissionDecision::Deny {
            reason: aura_permission_domain::PermissionDenyReason::NetworkAccessDeclarationRequired
        }
    ));
}

#[test]
fn ordinary_networked_cli_work_follows_the_three_permission_modes() {
    for (mode, expected) in [
        (PermissionMode::ReadOnly, "deny"),
        (PermissionMode::WorkspaceReadWrite, "deny"),
        (PermissionMode::FullAccess, "allow"),
    ] {
        let (mut plan, snapshot) = fixture(mode);
        plan.capabilities = vec![
            ResolvedCapability::ProcessExecute,
            ResolvedCapability::PublicNetworkRead,
            ResolvedCapability::DownloadArtifact,
            ResolvedCapability::GitRemoteRead,
        ];
        plan.requested_sandbox = Some(NativeSandboxMode::ObservedNativeNetworked);
        plan.network_access = NetworkAccessRequirement::Required;
        assert_eq!(decision_name(&evaluate(&plan, &snapshot, &[], &[])), expected);
    }
}

#[test]
fn networked_cli_high_risk_operations_still_require_approval() {
    for (capability, risk) in [
        (ResolvedCapability::DownloadWindowsExecutable, OperationRiskFlag::ExecutableArtifact),
        (ResolvedCapability::DataEgress, OperationRiskFlag::DataLeavesDevice),
        (ResolvedCapability::WindowsSoftwareInstall, OperationRiskFlag::SoftwareInstallation),
        (ResolvedCapability::ElevatedOperation, OperationRiskFlag::Elevation),
        (ResolvedCapability::SystemFileWrite, OperationRiskFlag::SystemConfigurationMutation),
    ] {
        for mode in [PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess] {
            let (mut plan, snapshot) = fixture(mode);
            plan.capabilities = vec![
                ResolvedCapability::ProcessExecute,
                ResolvedCapability::PublicNetworkRead,
                capability,
            ];
            plan.risk_flags = vec![risk];
            plan.requested_sandbox = Some(NativeSandboxMode::ObservedNativeNetworked);
            plan.network_access = NetworkAccessRequirement::Required;
            let expected = if mode == PermissionMode::WorkspaceReadWrite {
                "deny"
            } else if capability == ResolvedCapability::DataEgress {
                "allow"
            } else {
                "ask"
            };
            assert_eq!(decision_name(&evaluate(&plan, &snapshot, &[], &[])), expected);
        }
    }
}
