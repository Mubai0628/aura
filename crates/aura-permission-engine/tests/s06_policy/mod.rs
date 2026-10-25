use super::*;
mod dispatch_reuse;
mod missing_capabilities;

// Review: these cases keep the real assessment/authorization path and name
// the confirmed defaults. They deliberately stay red until the policy is repaired.
fn s06_review_external_case(mode: PermissionMode, capability: ResolvedCapability) -> Case {
    let mut c = Case::new(false);
    c.snapshot.mode = mode;
    c.op.content.risks.clear();
    c.op.content.network = OperationNetwork::None;
    let display = "C:\\review-synthetic\\ordinary.txt";
    c.op.content.resources = vec![ResourceDescriptor::ExternalPath {
        canonical_path: CanonicalPath {
            display: display.into(),
            utf16: display.encode_utf16().collect(),
        },
        identity: None,
        parent_identity: None,
        directory: false,
        recursive: false,
        file_target: None,
    }];
    c.op.content.capabilities = vec![capability];
    c.backend.capabilities = [capability].into_iter().collect();
    c.refresh();
    c
}

#[test]
fn s06_review_workspace_mode_asks_before_reading_an_unapproved_external_file() {
    let mut c =
        s06_review_external_case(PermissionMode::FullAccess, ResolvedCapability::ExternalFileRead);
    // Reject an implementation that simply denies every external read.
    c.allow();
    c.snapshot.mode = PermissionMode::WorkspaceReadWrite;
    let actual = authorize(&c.input(), &c.backend, None);
    assert!(
        matches!(actual, PolicyAuthorization::Ask { .. }),
        "workspace defaults do not authorize an unapproved external read: {actual:?}"
    );
}

#[test]
fn s06_review_ordinary_delete_does_not_add_an_ask_for_the_outside_workspace_flag() {
    let mut c = s06_review_external_case(
        PermissionMode::FullAccess,
        ResolvedCapability::ExternalFileDelete,
    );
    c.allow();
    c.op.content.risks = vec![OperationRiskFlag::OutsideWorkspaceDelete];
    c.refresh();
    let actual = authorize(&c.input(), &c.backend, None);
    assert!(
        matches!(actual, PolicyAuthorization::Allow { .. }),
        "ordinary deletion in the default writable scope must not ask again: {actual:?}"
    );
}

fn s06_review_verified_grant_survives_readonly_default(capability: ResolvedCapability) {
    let mut c = s06_review_external_case(PermissionMode::FullAccess, capability);
    // This descriptor is the trusted Core resolver's output, not model input.
    // Core still owes the actual record, scope, capability and liveness checks.
    c.op.content.resources = vec![ResourceDescriptor::ExternalGrant {
        grant_id: aura_domain_common::ExternalGrantId::new(),
    }];
    c.verified_external = vec![capability];
    c.refresh();
    c.allow();
    c.snapshot.mode = PermissionMode::ReadOnly;
    let actual = authorize(&c.input(), &c.backend, None);
    assert!(
        matches!(actual, PolicyAuthorization::Allow { .. }),
        "Core-resolved granted {capability:?} must not ask again just because the default is ReadOnly: {actual:?}"
    );
}

#[test]
fn s06_review_granted_external_read_is_not_reasked_in_readonly() {
    s06_review_verified_grant_survives_readonly_default(ResolvedCapability::ExternalFileRead);
}

#[test]
fn s06_review_granted_external_write_is_not_reasked_in_readonly() {
    s06_review_verified_grant_survives_readonly_default(ResolvedCapability::ExternalFileWrite);
}

#[test]
fn s06_review_session_file_approval_covers_a_new_call_to_the_same_target() {
    let mut c = Case::new(false);
    c.snapshot.mode = PermissionMode::ReadOnly;
    c.op.content.risks.clear();
    c.op.content.network = OperationNetwork::None;
    c.op.content.action_kind = "fs.write".parse().expect("action kind");
    c.op.content.action = serde_json::json!({"path":"approved.txt", "text":"first"});
    c.op.content.capabilities = vec![ResolvedCapability::WorkspaceFileWrite];
    c.op.content.resources = vec![ResourceDescriptor::WorkspacePath {
        workspace_id: c.snapshot.workspace_id,
        canonical_relative_path: "approved.txt"
            .parse::<aura_domain_common::WorkspacePath>()
            .expect("path"),
        file_target: Some(aura_security_contract::FileConsentTarget {
            canonical_path: ("approved.txt"
                .parse::<aura_domain_common::WorkspacePath>()
                .expect("path"))
            .as_str()
            .to_owned(),
            identity_key: format!(
                "fixture:{}",
                ("approved.txt".parse::<aura_domain_common::WorkspacePath>().expect("path"))
                    .as_str()
            ),
            ..Default::default()
        }),
    }];
    c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
    c.refresh();
    c.approve(AuthorizationLifetime::Session { session_id: c.snapshot.session_id });
    c.allow();
    let prior_fingerprint = c.op.fingerprint.clone();
    c.op.operation_id = aura_domain_common::SecurityOperationId::new();
    c.op.content.action["text"] = serde_json::json!("second");
    c.refresh();
    assert_ne!(
        c.op.fingerprint, prior_fingerprint,
        "a genuinely new call, not the original operation again"
    );
    let actual = authorize(&c.input(), &c.backend, None);
    assert!(
        matches!(actual, PolicyAuthorization::Allow { .. }),
        "a session answer for this tool capability and target must be usable by the next legal call: {actual:?}"
    );
}

/// Builds the reviewed case: a ReadOnly session whose user answered `fs.write` on `approved.txt`
/// for the session.
fn session_file_case() -> Case {
    let mut c = Case::new(false);
    c.snapshot.mode = PermissionMode::ReadOnly;
    c.op.content.risks.clear();
    c.op.content.network = OperationNetwork::None;
    c.op.content.action_kind = "fs.write".parse().expect("action kind");
    c.op.content.action = serde_json::json!({"path":"approved.txt", "text":"first"});
    c.op.content.capabilities = vec![ResolvedCapability::WorkspaceFileWrite];
    c.op.content.resources = vec![ResourceDescriptor::WorkspacePath {
        workspace_id: c.snapshot.workspace_id,
        canonical_relative_path: "approved.txt"
            .parse::<aura_domain_common::WorkspacePath>()
            .expect("path"),
        file_target: Some(aura_security_contract::FileConsentTarget {
            canonical_path: ("approved.txt"
                .parse::<aura_domain_common::WorkspacePath>()
                .expect("path"))
            .as_str()
            .to_owned(),
            identity_key: format!(
                "fixture:{}",
                ("approved.txt".parse::<aura_domain_common::WorkspacePath>().expect("path"))
                    .as_str()
            ),
            ..Default::default()
        }),
    }];
    c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
    c.refresh();
    c.approve(AuthorizationLifetime::Session { session_id: c.snapshot.session_id });
    c.allow();
    c
}

/// The reuse is real, so an implementation that simply refused every different fingerprint would
/// fail. Stated as its own test because "never reuse" is the easiest way to make the negative
/// cases below pass while breaking the product rule the consent exists for.
#[test]
fn s06_review_session_file_consent_is_really_reusable() {
    let mut c = session_file_case();
    for (text, path) in [("second", "approved.txt"), ("third", "approved.txt")] {
        c.op.operation_id = aura_domain_common::SecurityOperationId::new();
        c.op.content.action["text"] = serde_json::json!(text);
        c.op.content.resources = vec![ResourceDescriptor::WorkspacePath {
            workspace_id: c.snapshot.workspace_id,
            canonical_relative_path: path
                .parse::<aura_domain_common::WorkspacePath>()
                .expect("path"),
            file_target: Some(aura_security_contract::FileConsentTarget {
                canonical_path: (path.parse::<aura_domain_common::WorkspacePath>().expect("path"))
                    .as_str()
                    .to_owned(),
                identity_key: format!(
                    "fixture:{}",
                    (path.parse::<aura_domain_common::WorkspacePath>().expect("path")).as_str()
                ),
                ..Default::default()
            }),
        }];
        c.refresh();
        let actual = authorize(&c.input(), &c.backend, None);
        assert!(
            matches!(actual, PolicyAuthorization::Allow { .. }),
            "the same target must stay usable across calls ({text}): {actual:?}"
        );
    }
}

/// The consent covers a capability and a target, never "this tool" or "this directory" by
/// accident. Each case here is a way a broader reading could pass the reuse test above.
#[test]
fn s06_review_file_consent_does_not_widen_beyond_its_capability_or_target() {
    // A different target is a different question, even though the capability and tool match.
    let mut other_target = session_file_case();
    other_target.op.operation_id = aura_domain_common::SecurityOperationId::new();
    other_target.op.content.resources = vec![ResourceDescriptor::WorkspacePath {
        workspace_id: other_target.snapshot.workspace_id,
        canonical_relative_path: "unapproved.txt"
            .parse::<aura_domain_common::WorkspacePath>()
            .expect("path"),
        file_target: Some(aura_security_contract::FileConsentTarget {
            canonical_path: ("unapproved.txt"
                .parse::<aura_domain_common::WorkspacePath>()
                .expect("path"))
            .as_str()
            .to_owned(),
            identity_key: format!(
                "fixture:{}",
                ("unapproved.txt".parse::<aura_domain_common::WorkspacePath>().expect("path"))
                    .as_str()
            ),
            ..Default::default()
        }),
    }];
    other_target.refresh();
    let actual = authorize(&other_target.input(), &other_target.backend, None);
    assert!(
        !matches!(actual, PolicyAuthorization::Allow { .. }),
        "an answer for one target must not authorize another: {actual:?}"
    );

    // A write answer must not become a delete answer.
    let mut other_capability = session_file_case();
    other_capability.op.operation_id = aura_domain_common::SecurityOperationId::new();
    other_capability.op.content.action_kind = "fs.delete".parse().expect("action kind");
    other_capability.op.content.capabilities = vec![ResolvedCapability::WorkspaceFileDelete];
    other_capability.backend.capabilities =
        other_capability.op.content.capabilities.iter().copied().collect();
    other_capability.refresh();
    let actual = authorize(&other_capability.input(), &other_capability.backend, None);
    assert!(
        !matches!(actual, PolicyAuthorization::Allow { .. }),
        "a write answer must not authorize a delete: {actual:?}"
    );

    // Another session cannot borrow the answer.
    let mut other_session = session_file_case();
    other_session.op.operation_id = aura_domain_common::SecurityOperationId::new();
    other_session.op.content.action["text"] = serde_json::json!("second");
    let other_session_id = aura_domain_common::SessionId::new();
    other_session.op.content.identity.session_id = Some(other_session_id);
    other_session.op.content.identity.correlation_id = aura_domain_common::CorrelationId::new();
    other_session.refresh();
    let actual = authorize(&other_session.input(), &other_session.backend, None);
    assert!(
        !matches!(actual, PolicyAuthorization::Allow { .. }),
        "another session must not borrow this session's answer: {actual:?}"
    );
}

/// File consent survives a restart and all mode revisions.
#[test]
fn s06_b4_file_consent_survives_core_restart_and_permission_changes() {
    let mut c = session_file_case();
    c.op.operation_id = aura_domain_common::SecurityOperationId::new();
    c.op.content.action["text"] = serde_json::json!("second");
    c.snapshot.session_permission_revision += 1;
    c.runtime_instance = aura_domain_common::RuntimeInstanceId::new();
    c.refresh();
    let actual = authorize(&c.input(), &c.backend, None);
    assert!(
        matches!(actual, PolicyAuthorization::Allow { .. }),
        "file session consent survives restart and mode changes: {actual:?}"
    );
}
