#[path = "v2_policy/l03_git.rs"]
mod l03_git;
#[path = "v2_policy/l04_package.rs"]
mod l04_package;
#[path = "v2_policy/l05_memory.rs"]
mod l05_memory;
#[path = "v2_policy/l06_commands.rs"]
mod l06_commands;
#[path = "v2_policy/l06_init.rs"]
mod l06_init;
#[path = "v2_policy/l08_mcp.rs"]
mod l08_mcp;
#[path = "v2_policy/l13_4_deletion.rs"]
mod l13_4_deletion;
#[path = "v2_policy/l13_browser.rs"]
mod l13_browser;
#[path = "v2_policy/native.rs"]
mod native;
#[path = "v2_policy/network.rs"]
mod network;
#[path = "v2_policy/s08_shell.rs"]
mod s08_shell;
use aura_domain_common::{ContentHash, PermissionSnapshotId, Revision};
use aura_execution_contract::*;
use aura_permission_engine::{dispatch_guard, policy::*, selector};
use aura_security_contract::*;

#[path = "v2_policy/support.rs"]
mod support;
use support::*;
mod s06_policy;

#[test]
fn accepted_dns_limit_does_not_reuse_strong_offline_authority() {
    let c = Case::new(true);
    let mut old = c.allow();
    old.scope.backend_requirement.guarantees.remove(&EnforcementGuarantee::DirectNetworkBlockedV1);
    old.scope.backend_requirement.guarantees.insert(EnforcementGuarantee::OfflineNetwork);
    assert!(matches!(
        authorize(&c.input(), &c.backend, Some(&old)),
        PolicyAuthorization::Ask { .. }
    ));
    assert!(
        dispatch_guard::revalidate(c.request(&old), &old, &c.op, &c.input(), &c.backend, &cancel())
            .is_err()
    );
    let mut hidden_limit = c.backend.clone();
    hidden_limit.known_limitations.clear();
    assert!(selector::select(&c.input(), &[hidden_limit], None, None, false).is_err());
    let mut overclaimed = c.backend.clone();
    overclaimed.guarantees.insert(EnforcementGuarantee::OfflineNetwork);
    assert!(selector::select(&c.input(), &[overclaimed], None, None, false).is_err());
}

#[test]
fn agent_ceiling_rejects_old_grants_and_native_fallback_but_not_authenticated_ui() {
    let mut c = Case::new(false);
    c.op.content.capabilities = vec![ResolvedCapability::WorkspaceFileWrite];
    c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
    c.refresh();
    let allowed = c.allow();
    c.grants.push(AuthorizationGrant {
        grant_id: aura_domain_common::PermissionGrantId::new(),
        revision: Revision::INITIAL,
        scope: allowed.scope,
        issued_at: allowed.decided_at,
        expires_at: Some(allowed.expires_at),
        revoked_at: None,
        consumed_at: None,
    });
    // The mode is a default, not a ceiling: a ReadOnly session writing a workspace file is
    // asked, not refused. This is the migrated counter-example — the old expectation was
    // `Deny { ModeDenied }`, which made the confirmed "ReadOnly may approve a specified
    // write" rule unreachable.
    //
    // The grant above was minted for this exact operation while the session was in the
    // working mode, so it is removed first: the question being asked here is what the mode
    // does on its own, and an exact-operation grant would otherwise answer it.
    let grants = std::mem::take(&mut c.grants);
    c.snapshot.mode = PermissionMode::ReadOnly;
    assert!(
        matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Ask { .. }),
        "ReadOnly writing a workspace file asks for a precise approval"
    );
    // And a precise approval is what makes it proceed: the same operation, now covered by a
    // one-shot grant for exactly it, is allowed in ReadOnly.
    c.grants = grants;
    c.grants[0].scope.lifetime = AuthorizationLifetime::Once { operation_id: c.op.operation_id };
    assert!(
        matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Allow { .. }),
        "a precise ReadOnly approval authorizes the specified write"
    );
    // A child agent outside the mode's default has no authority of its own to fall back on,
    // so it stays refused rather than being offered a prompt its parent never established.
    c.op.content.identity.actor = InvocationActor::ChildAgent;
    c.op.content.identity.parent_agent_run_id = Some(aura_domain_common::AgentRunId::new());
    c.refresh();
    assert_eq!(assess(&c.input()).err(), Some(PolicyDenyReason::ModeDenied));
    c.op.content.identity.actor = InvocationActor::User;
    c.op.content.identity.agent_run_id = None;
    c.op.content.identity.turn_task_id = None;
    c.op.content.identity.parent_agent_run_id = None;
    c.op.content.source = SecuritySourceRef::UserInterface;
    c.refresh();
    assert!(assess(&c.input()).is_ok(), "UI mutation has separate authority");
    c.op.content.capabilities = vec![ResolvedCapability::ProcessExecute];
    c.refresh();
    assert!(
        !assess(&c.input())
            .expect("UI process has independent authorization")
            .requirement
            .guarantees
            .contains(&EnforcementGuarantee::ReadOnlyUserFiles)
    );

    let mut command = Case::new(true);
    command.op.content.network = OperationNetwork::BroadProcess;
    command.op.content.capabilities.push(ResolvedCapability::PublicNetworkRead);
    command.refresh();
    for mode in [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite] {
        command.snapshot.mode = mode;
        for class in [BackendClass::RestrictedUserOnline, BackendClass::Native] {
            command.backend.class = class;
            assert!(matches!(
                authorize(&command.input(), &command.backend, None),
                PolicyAuthorization::Deny { reason: PolicyDenyReason::ModeDenied }
            ));
        }
    }
}

#[test]
fn full_access_network_and_upload_do_not_ask_again() {
    let mut c = Case::new(true);
    c.snapshot.mode = PermissionMode::FullAccess;
    c.backend.class = BackendClass::RestrictedUserOnline;
    c.backend.guarantees.remove(&EnforcementGuarantee::DirectNetworkBlockedV1);
    c.backend.known_limitations.clear();
    c.op.content.network = OperationNetwork::BroadProcess;
    c.op.content
        .capabilities
        .extend([ResolvedCapability::PublicNetworkRead, ResolvedCapability::DataEgress]);
    c.op.content.risks =
        vec![OperationRiskFlag::DataLeavesDevice, OperationRiskFlag::PrivateOrLoopbackNetwork];
    c.refresh();
    c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
    assert!(matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Allow { .. }));
    c.op.content.capabilities.push(ResolvedCapability::ElevatedOperation);
    c.refresh();
    c.backend.capabilities.insert(ResolvedCapability::ElevatedOperation);
    assert!(matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Ask { .. }));
}

#[test]
fn policy_has_no_sandbox_or_readiness_dependency_and_mode_is_not_profile() {
    let mut c = Case::new(true);
    assert!(!assess(&c.input()).expect("policy without backend").asks);
    c.backend.readiness = BackendReadiness::Unavailable { reason: "not installed".into() };
    assert!(!assess(&c.input()).expect("same policy").asks);
    assert!(selector::select(&c.input(), &[c.backend.clone()], None, None, false).is_err());
    assert_eq!(
        SecurityProfile::for_permission_mode(PermissionMode::ReadOnly),
        SecurityProfile::Balanced
    );
    c.snapshot.mode = PermissionMode::ReadOnly;
    assert!(assess(&c.input()).expect("read only shell approval").asks);
    assert!(matches!(
        authorize(&c.input(), &c.backend, None),
        PolicyAuthorization::Deny { reason: PolicyDenyReason::BackendUnavailable }
    ));
    c.profile = SecurityProfile::Guided;
    c.snapshot.mode = PermissionMode::WorkspaceReadWrite;
    assert!(assess(&c.input()).expect("guided").asks);
}

#[test]
fn eligibility_uses_each_guarantee_compatibility_and_profile() {
    let mut c = Case::new(true);
    c.op.content.compatibility.insert(ExecutionCompatibility::NativeWindowsToolchain);
    c.refresh();
    assert!(selector::select(&c.input(), &[c.backend.clone()], None, None, false).is_err());
    c.backend.compatibility = c.op.content.compatibility.clone();
    let mut strict = c.backend.clone();
    strict.id = BackendId("strict".into());
    strict.class = BackendClass::AppContainer;
    strict.guarantees.remove(&EnforcementGuarantee::DirectNetworkBlockedV1);
    strict.guarantees.insert(EnforcementGuarantee::OfflineNetwork);
    strict.known_limitations.clear();
    strict.guarantees.insert(EnforcementGuarantee::AuthorizedReadsOnly);
    let candidates =
        selector::select(&c.input(), &[c.backend.clone(), strict.clone()], None, None, false)
            .expect("two candidates");
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].backend.id, strict.id);
    for guarantee in [
        EnforcementGuarantee::AuthorizedWritesOnly,
        EnforcementGuarantee::DirectNetworkBlockedV1,
        EnforcementGuarantee::ChildProcessControl,
        EnforcementGuarantee::BoundedOutput,
        EnforcementGuarantee::IdentityIsolation,
    ] {
        let mut missing = c.backend.clone();
        missing.guarantees.remove(&guarantee);
        assert!(
            selector::select(&c.input(), &[missing], None, None, false).is_err(),
            "{guarantee:?}"
        );
    }
    c.profile = SecurityProfile::Strict;
    let only = selector::select(&c.input(), &[c.backend.clone(), strict], None, None, false)
        .expect("strict");
    assert_eq!(only.len(), 1);
}

#[test]
fn exact_http_and_broad_process_network_are_not_interchangeable() {
    let mut c = Case::new(true);
    c.snapshot.mode = PermissionMode::FullAccess;
    c.op.content.network = OperationNetwork::BroadProcess;
    c.op.content.capabilities.push(ResolvedCapability::PublicNetworkRead);
    c.refresh();
    c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
    assert!(selector::select(&c.input(), &[c.backend.clone()], None, None, false).is_err());
    c.backend.class = BackendClass::RestrictedUserOnline;
    c.backend.guarantees.remove(&EnforcementGuarantee::DirectNetworkBlockedV1);
    c.backend.known_limitations.clear();
    assert!(matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Allow { .. }));
    c.op.content.network = OperationNetwork::ExactOrigins;
    c.refresh();
    assert!(assess(&c.input()).is_err());
    c.op.content.resources.push(ResourceDescriptor::NetworkOrigin {
        scheme: "https".into(),
        host: "example.test".into(),
        port: 443,
    });
    c.refresh();
    assert!(matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Deny { .. }));
    c.backend.guarantees.insert(EnforcementGuarantee::ExactNetworkOrigins);
    assert!(matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Allow { .. }));
    c.profile = SecurityProfile::Strict;
    assert_eq!(assess(&c.input()).err(), Some(PolicyDenyReason::ProfileProhibited));
}

#[test]
fn equivalent_backend_requires_new_plan_binding_and_weaker_requires_new_approval() {
    let mut c = Case::new(true);
    c.backend.guarantees.insert(EnforcementGuarantee::AuthorizedReadsOnly);
    let authority = c.allow();
    let old = c.request(&authority);
    c.backend.id = BackendId("equivalent".into());
    c.backend.revision = Revision::new(2);
    assert!(
        dispatch_guard::revalidate(old, &authority, &c.op, &c.input(), &c.backend, &cancel())
            .is_err()
    );
    let fresh = c.request(&authority);
    dispatch_guard::revalidate(fresh, &authority, &c.op, &c.input(), &c.backend, &cancel())
        .expect("same op new binding");
    c.backend.guarantees.remove(&EnforcementGuarantee::AuthorizedReadsOnly);
    assert!(matches!(
        authorize(&c.input(), &c.backend, Some(&authority)),
        PolicyAuthorization::Ask { .. }
    ));
    assert!(
        selector::select(&c.input(), &[c.backend.clone()], None, Some(&authority), true).is_err()
    );
}

#[test]
fn hard_boundaries_denial_source_and_credentials_cannot_be_overridden() {
    let mut c = Case::new(false);
    c.profile = SecurityProfile::FullAccess;
    let mut input = c.input();
    input.hard_boundaries_satisfied = false;
    let deny = authorize(&input, &c.backend, None);
    assert!(matches!(deny, PolicyAuthorization::Deny { reason: PolicyDenyReason::HardBoundary }));
    assert!(!serde_json::to_string(&deny).expect("serialize").contains("credential-canary"));
    input.hard_boundaries_satisfied = true;
    input.denied_for_turn = true;
    assert!(assess(&input).is_err());
    input.denied_for_turn = false;
    input.source_enabled = false;
    assert!(assess(&input).is_err());
    c.op.content.source = SecuritySourceRef::Plugin { plugin_id: "plugin".into() };
    c.op.content.source_revision_fingerprint = Some(ContentHash::blake3(b"source-revision-1"));
    c.refresh();
    c.trust = TrustState::Revoked;
    assert!(assess(&c.input()).is_err());
    c.trust = TrustState::SystemTrusted;
    let mut input = c.input();
    let changed = ContentHash::blake3(b"source-revision-2");
    input.source_fingerprint = Some(&changed);
    assert!(assess(&input).is_err());
}

#[test]
fn dispatch_rejects_current_fact_changes_even_when_old_request_hashes_match() {
    let mut c = Case::new(false);
    let hash = aura_domain_common::jcs::CanonicalSha256HashV1::sha256;
    // A path outside the workspace that is not an ordinary external file: a system path
    // still asks, which is what this case needs in order to be approved before dispatch
    // is exercised. An ordinary external file follows the capability baseline instead.
    c.op.content.resources.push(ResourceDescriptor::SystemPath {
        canonical_path: CanonicalPath {
            display: "C:/synthetic/file".into(),
            utf16: "C:/synthetic/file".encode_utf16().collect(),
        },
        identity: None,
        parent_identity: None,
        file_target: None,
    });
    c.op.content.resources.push(ResourceDescriptor::ToolRegistrySnapshot {
        resource_fingerprint: hash(b"registry-1"),
    });
    c.op.content.credentials = aura_contract_schema::v2_kernel_fixture()
        .expect("credential reference")
        .dispatch
        .operation
        .content
        .credentials;
    c.backend.guarantees.insert(EnforcementGuarantee::CredentialIsolation);
    c.refresh();
    c.approve(AuthorizationLifetime::Once { operation_id: c.op.operation_id });
    let authority = c.allow();
    let request = c.request(&authority);
    let mut snapshot = c.snapshot.clone();
    // Permission changes create a new immutable Snapshot; Core's Store rejects
    // an in-place payload rewrite under an existing Snapshot ID.
    snapshot.id = PermissionSnapshotId::new();
    snapshot.session_permission_revision += 1;
    let mut input = c.input();
    input.snapshot = &snapshot;
    assert!(
        dispatch_guard::revalidate(
            request.clone(),
            &authority,
            &c.op,
            &input,
            &c.backend,
            &cancel()
        )
        .is_err()
    );
    for changed in 0..4 {
        let mut content = c.op.content.clone();
        match changed {
            0 => {
                let ResourceDescriptor::SystemPath { canonical_path, .. } =
                    &mut content.resources[1]
                else {
                    panic!("system path")
                };
                canonical_path.display = "C:/synthetic/other".into();
                canonical_path.utf16 = canonical_path.display.encode_utf16().collect();
            }
            1 => {
                content.resources[2] = ResourceDescriptor::ToolRegistrySnapshot {
                    resource_fingerprint: hash(b"registry-2"),
                }
            }
            2 => {
                content.credentials[0].credential_revision =
                    content.credentials[0].credential_revision.next().expect("rotated credential")
            }
            _ => content.action_revision = Revision::new(2),
        }
        let current = SecuredOperation::new(c.op.operation_id, content).expect("new current facts");
        let mut input = c.input();
        input.operation = &current;
        assert!(
            dispatch_guard::revalidate(
                request.clone(),
                &authority,
                &current,
                &input,
                &c.backend,
                &cancel()
            )
            .is_err()
        );
    }
    let mut cancelled = cancel();
    cancelled.revision = Revision::new(2);
    assert!(
        dispatch_guard::revalidate(
            request.clone(),
            &authority,
            &c.op,
            &c.input(),
            &c.backend,
            &cancelled
        )
        .is_err()
    );
    let mut expired = c.input();
    expired.now = authority.expires_at;
    assert!(
        dispatch_guard::revalidate(request, &authority, &c.op, &expired, &c.backend, &cancel())
            .is_err()
    );
}

/// An ordinary delete is decided by its **scope**, not by where the object sits; a grant is
/// still exact and live.
///
/// Three claims, each a counter-example against a different wrong implementation:
///
/// * the `OutsideWorkspaceDelete` flag does **not** by itself produce an approval request. It
///   records where the object is, not what the operation does to it; treating it as a reason
///   to ask is what made an ordinary granted-scope delete ask a second time.
/// * a `Session` grant really authorizes it, so "reusable at the term the user chose" is not
///   quietly reduced to a single use.
/// * the grant is still exact and live: revocation, consumption, a revision move and expiry
///   each refuse it, so "reusable" never became "reusable after it died".
#[test]
fn grants_are_exact_live_and_high_risk_once() {
    let mut c = Case::new(false);
    c.op.content.risks.push(OperationRiskFlag::OutsideWorkspaceDelete);
    c.refresh();
    // The flag describes the object's location. In a writable scope that is an ordinary write,
    // so it must not add its own question; FullAccess reaches an ordinary external delete by
    // default. What decides the operation is the scope, checked below through the grant.
    assert!(
        matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Allow { .. }),
        "the outside-workspace flag alone must not add a second approval"
    );
    // The same operation under an explicit grant: the grant is the authority, and a session
    // term is a term the user may choose. The grant is recorded with a session lifetime so the
    // assertion is about the grant being accepted, not about the mode's default.
    c.grants.push(AuthorizationGrant {
        grant_id: aura_domain_common::PermissionGrantId::new(),
        revision: Revision::INITIAL,
        scope: {
            let mut scope = c.allow().scope;
            scope.lifetime = AuthorizationLifetime::Session { session_id: c.snapshot.session_id };
            scope
        },
        issued_at: c.input().now,
        expires_at: Some(c.input().expires_at),
        revoked_at: None,
        consumed_at: None,
    });
    assert!(
        matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Allow { .. }),
        "a session grant authorizes an ordinary external delete in its scope"
    );
    // A truly non-reusable operation still forces a single use: elevation is not something a
    // session grant may stand in for.
    let mut once_only = Case::new(false);
    once_only.op.content.risks.push(OperationRiskFlag::Elevation);
    once_only.refresh();
    once_only.approve(AuthorizationLifetime::Session { session_id: once_only.snapshot.session_id });
    assert!(
        matches!(
            authorize(&once_only.input(), &once_only.backend, None),
            PolicyAuthorization::Ask { .. }
        ),
        "an elevation stays a one-shot approval"
    );
    c.grants[0].scope.lifetime = AuthorizationLifetime::Once { operation_id: c.op.operation_id };
    let authority = c.allow();
    let request = c.request(&authority);
    for kind in 0..4 {
        let mut grants = c.grants.clone();
        match kind {
            0 => grants[0].revoked_at = Some(c.input().now),
            1 => grants[0].consumed_at = Some(c.input().now),
            2 => grants[0].revision = Revision::new(2),
            _ => grants[0].expires_at = Some(c.input().now),
        }
        let mut input = c.input();
        input.grants = &grants;
        assert!(
            dispatch_guard::revalidate(
                request.clone(),
                &authority,
                &c.op,
                &input,
                &c.backend,
                &cancel()
            )
            .is_err()
        );
    }
}

#[test]
fn missing_core_snapshot_or_mutable_source_revision_cannot_be_authorized() {
    let mut c = Case::new(false);
    c.op.content.identity.permission_snapshot_id = None;
    c.refresh();
    assert_eq!(assess(&c.input()).err(), Some(PolicyDenyReason::IdentityMismatch));
    c.op.content.identity.permission_snapshot_id = Some(c.snapshot.id);
    for source in [
        SecuritySourceRef::Hook { hook_id: "hook".into() },
        SecuritySourceRef::McpServer { server_id: "mcp".into() },
        SecuritySourceRef::Skill { skill_id: "skill".into() },
        SecuritySourceRef::Plugin { plugin_id: "plugin".into() },
    ] {
        c.op.content.source = source;
        c.refresh();
        assert_eq!(assess(&c.input()).err(), Some(PolicyDenyReason::SourceUntrusted));
    }
}

/// What an authorization decision must be for one external capability in one mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expected {
    Allow,
    Ask,
    Denied,
}

/// An ordinary external path is decided by the capability the target really needs.
///
/// This drives the real assessment and the real authorization for each mode, so the
/// decision here is the approval state the user would see, not an internal flag. The
/// workspace capability a tool definition declares must never stand in for it: that
/// substitution is what would let a ReadOnly session read an external file, or a
/// WorkspaceReadWrite session write one, without any approval at all. The Core side of
/// that substitution is covered where the operation is classified.
///
/// The mode is a **default**, so `Ask` and `Denied` are different facts and both matter:
/// `Ask` means the user can grant exactly this capability for exactly this target, and
/// `Denied` means the mode does not reach this kind of authority at all. Only ReadOnly's
/// external *write* moved from the second to the first, which is the confirmed rule that a
/// ReadOnly session may approve writing a specified target without changing its mode.
#[test]
fn an_ordinary_external_path_is_decided_by_its_own_capability() {
    use ResolvedCapability::*;
    let cases = [
        (PermissionMode::ReadOnly, ExternalFileRead, Expected::Ask),
        (PermissionMode::ReadOnly, ExternalFileWrite, Expected::Ask),
        // An external object is never covered by a workspace default. WorkspaceReadWrite
        // authorizes reading *in the workspace*; treating a declared read capability as
        // authority over an arbitrary reachable object is the substitution that would let a
        // workspace-mode session read outside its boundary with no approval. It asks.
        (PermissionMode::WorkspaceReadWrite, ExternalFileRead, Expected::Ask),
        (PermissionMode::WorkspaceReadWrite, ExternalFileWrite, Expected::Ask),
        (PermissionMode::FullAccess, ExternalFileRead, Expected::Allow),
        (PermissionMode::FullAccess, ExternalFileWrite, Expected::Allow),
        (PermissionMode::FullAccess, ExternalFileCreate, Expected::Allow),
        // Deleting an ordinary external object follows the granted write scope rather than
        // forcing a fresh one-shot approval for every delete.
        (PermissionMode::FullAccess, ExternalFileDelete, Expected::Allow),
    ];
    for (mode, capability, expected) in cases {
        let mut c = Case::new(false);
        c.snapshot.mode = mode;
        c.op.content.risks.clear();
        c.op.content.network = OperationNetwork::None;
        c.op.content.resources = vec![ResourceDescriptor::ExternalPath {
            canonical_path: CanonicalPath {
                display: "C:\\review-synthetic\\file.txt".into(),
                utf16: "C:\\review-synthetic\\file.txt".encode_utf16().collect(),
            },
            identity: None,
            parent_identity: None,
            directory: false,
            recursive: false,
            file_target: None,
        }];
        c.op.content.capabilities = vec![capability];
        // A real plan is prepared for the operation it was chosen for, so the backend
        // declares the same capability the operation asks for.
        c.backend.capabilities = [capability].into_iter().collect();
        c.refresh();
        let decision = authorize(&c.input(), &c.backend, None);
        let actual = match decision {
            PolicyAuthorization::Allow { .. } => Expected::Allow,
            PolicyAuthorization::Ask { .. } => Expected::Ask,
            PolicyAuthorization::Deny { .. } => Expected::Denied,
        };
        assert_eq!(actual, expected, "{capability:?} on an external path in {mode:?}");
    }
}

/// A precise lower-mode approval is real authority, and the newest fact still governs it.
///
/// This is the end-to-end shape of the confirmed rule — "a ReadOnly session may approve
/// writing a specified target, without changing the whole session mode" — and it is
/// deliberately expressed as a **full** cycle rather than a single decision: ask, approve,
/// reuse, then lose it. Each step is a counter-example against a different way the rule could
/// be implemented wrongly:
///
/// * if the mode were still a ceiling, the first step would be a `Deny` instead of an `Ask`,
///   and the approved write could never happen;
/// * if the approval were not bound to the session and its permission revision, a mode change
///   would not invalidate it and a tightening would be unenforceable;
/// * if the approval were reusable past its term, `revalidate` at reservation would accept it
///   after it had expired.
#[test]
fn a_precise_lower_mode_approval_is_authority_and_the_newest_facts_still_govern_it() {
    let mut c = Case::new(false);
    c.op.content.capabilities = vec![ResolvedCapability::WorkspaceFileWrite];
    c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
    c.snapshot.mode = PermissionMode::ReadOnly;
    c.refresh();

    // 1. The mode does not authorize it, so the user is asked — not refused.
    let PolicyAuthorization::Ask { proposal } = authorize(&c.input(), &c.backend, None) else {
        panic!("ReadOnly writing a workspace file must ask, not deny");
    };
    // A single use is offered, and the session answer too: the user chooses the term, and
    // nothing about an ordinary workspace write is forced to one use.
    assert!(proposal.allowed_decisions.contains(&ApprovalDecisionKind::AllowOnce));
    assert!(
        proposal.allowed_decisions.contains(&ApprovalDecisionKind::AllowSession),
        "an ordinary workspace write may be approved for the session"
    );

    // 2. The approval is recorded for this session and becomes the authority.
    c.approve(AuthorizationLifetime::Session { session_id: c.snapshot.session_id });
    let authority = c.allow();
    assert!(
        matches!(authority.basis, AuthorizationBasis::Grant { .. }),
        "an approved write dispatches on the grant the user issued, not on the mode"
    );

    // 3. It survives an unchanged session, so the approved target stays usable.
    let request = c.request(&authority);
    assert!(
        dispatch_guard::revalidate(
            request.clone(),
            &authority,
            &c.op,
            &c.input(),
            &c.backend,
            &cancel()
        )
        .is_ok(),
        "the approved write can actually be dispatched"
    );

    // 4. A tightening of the session is enforced where the session's own revision is read.
    //
    // The layering matters and is asserted as such rather than assumed: a grant-bound decision
    // is re-checked by `revalidate` for its own liveness/revision/scope, while the *session*
    // permission revision is compared at reservation, in the same barrier that reads the live
    // mode (`PersistenceCoordinator::with_session_permission_authority`). So the correct local
    // statement here is the one `revalidate` owns: the approval is bound to the exact session
    // snapshot identity the dispatch carries, and a snapshot that names a different revision is
    // a different authority.
    //
    // What must NOT be true is that the grant quietly outlives a mode downgrade. It cannot:
    // `authorize` re-derives the whole binding on every call, the mode is part of the snapshot
    // the operation identity carries, and a grant is only ever matched for the session and
    // profile it was issued under. The dispatch-time comparison is covered by the reservation
    // tests in `aura-core`, which is where the live mode reader lives.
    let mut other_snapshot = c.snapshot.clone();
    other_snapshot.session_permission_revision += 1;
    other_snapshot.id = PermissionSnapshotId::new();
    let mut other = c.input();
    other.snapshot = &other_snapshot;
    assert!(
        dispatch_guard::revalidate(
            request.clone(),
            &authority,
            &c.op,
            &other,
            &c.backend,
            &cancel()
        )
        .is_err(),
        "a decision bound to one session snapshot is not honored under another"
    );

    // 5. Revoking the grant stops the call at reservation, and the mode change alone does not
    // resurrect it: a decision for one authority is never replayed for another.
    let mut revoked = c.input();
    let mut grants = c.grants.clone();
    grants[0].revoked_at = Some(c.input().now);
    revoked.grants = &grants;
    assert!(
        dispatch_guard::revalidate(
            request.clone(),
            &authority,
            &c.op,
            &revoked,
            &c.backend,
            &cancel()
        )
        .is_err(),
        "a revoked approval cannot dispatch"
    );
}
