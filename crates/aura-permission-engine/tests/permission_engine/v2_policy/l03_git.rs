//! Git's four Native actions keep their Git approval matrix and cannot lend the exception.
use super::*;

fn git(action: &str, mode: PermissionMode, write_or_force: bool) -> Case {
    let mut c = Case::new(false);
    c.snapshot.mode = mode;
    c.op.content.action_kind = action.into();
    c.op.content.action = serde_json::json!({"kind":"fixture"});
    c.op.content.capabilities = match action {
        "git.commit" | "git.branch" => vec![ResolvedCapability::GitWrite],
        "git.sync" => {
            let mut caps = vec![ResolvedCapability::GitRemoteRead];
            if write_or_force {
                caps.push(ResolvedCapability::GitWrite);
            }
            caps
        }
        "git.push" => {
            let mut caps = vec![ResolvedCapability::GitRemoteWrite];
            if write_or_force {
                caps.push(ResolvedCapability::GitDestructive);
            }
            caps
        }
        _ => panic!("closed Git fixture"),
    };
    c.op.content.network = if matches!(action, "git.sync" | "git.push") {
        OperationNetwork::BroadProcess
    } else {
        OperationNetwork::None
    };
    c.op.content.resources =
        vec![ResourceDescriptor::GitRepository { workspace_id: c.snapshot.workspace_id }];
    if matches!(action, "git.sync" | "git.push") {
        c.op.content.resources.push(ResourceDescriptor::GitRemoteDestination {
            facts: GitRemoteFactsV1 {
                target: GitRemoteTargetV1 {
                    repository: aura_domain_common::jcs::canonical_sha256(
                        &"fixture physical repository",
                    )
                    .expect("hash"),
                    remote_url: "https://fixture.invalid/repo.git".into(),
                    endpoint_fingerprint: aura_domain_common::jcs::canonical_sha256(
                        &"fixture endpoint",
                    )
                    .expect("hash"),
                    remote_branch: "main".into(),
                },
                remote: "origin".into(),
                local_branch: Some("main".into()),
                local_head: Some("a".repeat(40)),
                force_with_lease: (action == "git.push" && write_or_force).then(|| "b".repeat(40)),
                outgoing_count: Some(1),
                outgoing_subjects: vec!["fixture subject".into()],
            },
        });
    }
    c.backend.class = BackendClass::Native;
    c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
    c.backend.guarantees =
        [EnforcementGuarantee::ChildProcessControl, EnforcementGuarantee::BoundedOutput].into();
    c.backend.known_limitations.clear();
    c.refresh();
    c
}

#[test]
fn local_git_native_keeps_readonly_ask_and_workspace_full_allow() {
    for action in ["git.commit", "git.branch"] {
        for mode in [
            PermissionMode::ReadOnly,
            PermissionMode::WorkspaceReadWrite,
            PermissionMode::FullAccess,
        ] {
            let c = git(action, mode, false);
            let assessed = assess(&c.input()).expect("Git Native is structurally eligible");
            assert!(assessed.requirement.allowed_classes.contains(&BackendClass::Native));
            assert!(
                !assessed.requirement.guarantees.contains(&EnforcementGuarantee::ReadOnlyUserFiles)
            );
            assert!(
                !assessed
                    .requirement
                    .guarantees
                    .contains(&EnforcementGuarantee::AuthorizedWritesOnly)
            );
            let answer = authorize(&c.input(), &c.backend, None);
            if mode == PermissionMode::ReadOnly {
                assert!(matches!(answer, PolicyAuthorization::Ask { .. }), "{action}: {answer:?}");
            } else {
                assert!(
                    matches!(answer, PolicyAuthorization::Allow { .. }),
                    "{action}: {answer:?}"
                );
            }
        }
    }
}

#[test]
fn remote_git_native_preserves_ask_matrix_and_force_always_asks() {
    for (action, extra) in
        [("git.sync", false), ("git.sync", true), ("git.push", false), ("git.push", true)]
    {
        for mode in [
            PermissionMode::ReadOnly,
            PermissionMode::WorkspaceReadWrite,
            PermissionMode::FullAccess,
        ] {
            let c = git(action, mode, extra);
            let answer = authorize(&c.input(), &c.backend, None);
            let allowed = match action {
                "git.sync" => mode != PermissionMode::ReadOnly,
                "git.push" => mode == PermissionMode::FullAccess && !extra,
                _ => unreachable!(),
            };
            if allowed {
                assert!(
                    matches!(answer, PolicyAuthorization::Allow { .. }),
                    "{action}: {answer:?}"
                );
                let authority = c.allow();
                dispatch_guard::revalidate(
                    c.request(&authority),
                    &authority,
                    &c.op,
                    &c.input(),
                    &c.backend,
                    &cancel(),
                )
                .expect("mode authority must survive the actual dispatch guard");
            } else {
                let PolicyAuthorization::Ask { proposal } = answer else {
                    panic!("Git needs its ordinary approval: {action} {mode:?} {answer:?}");
                };
                if action == "git.push" && extra {
                    assert_eq!(
                        proposal.allowed_decisions,
                        vec![ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny]
                    );
                }
                assert!(proposal.native_session_consent.is_none(), "Git is not Shell consent");
            }
        }
    }
}

#[test]
fn l03_native_dispatch_accepts_git_and_refuses_other_actions_and_extra_capabilities() {
    let mut c = git("git.commit", PermissionMode::WorkspaceReadWrite, false);
    let authority = c.allow();
    dispatch_guard::revalidate(
        c.request(&authority),
        &authority,
        &c.op,
        &c.input(),
        &c.backend,
        &cancel(),
    )
    .expect("Git mode authorization reaches the real dispatch boundary");
    for action in ["git.status", "ordinary.process", "extension.git.commit"] {
        c.op.content.action_kind = action.into();
        c.refresh();
        let mut forged = authority.clone();
        forged.scope.operation_fingerprint = c.op.fingerprint.clone();
        assert!(
            AuthorizedDispatch::revalidate(
                c.request(&forged),
                &forged,
                &c.backend,
                &cancel(),
                c.snapshot.created_at
            )
            .is_err(),
            "{action} cannot borrow Git's Native composite exception"
        );
    }
    let mut c = git("git.commit", PermissionMode::WorkspaceReadWrite, false);
    c.op.content.capabilities.push(ResolvedCapability::ProcessExecute);
    c.backend.capabilities.insert(ResolvedCapability::ProcessExecute);
    c.refresh();
    // Refused outright, not merely asked: an Ask would let an approval route a widened
    // operation through the Native exception.
    assert!(matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Deny { .. }));
    let mut c = git("git.branch", PermissionMode::WorkspaceReadWrite, false);
    c.op.content.source = SecuritySourceRef::UserInterface;
    c.refresh();
    assert!(matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Deny { .. }));
}

#[test]
fn git_native_still_requires_job_and_output_guarantees_and_respects_strict_profile() {
    for missing in [EnforcementGuarantee::ChildProcessControl, EnforcementGuarantee::BoundedOutput]
    {
        let mut c = git("git.commit", PermissionMode::WorkspaceReadWrite, false);
        c.backend.guarantees.remove(&missing);
        assert!(matches!(
            authorize(&c.input(), &c.backend, None),
            PolicyAuthorization::Deny { .. }
        ));
    }
    let mut c = git("git.branch", PermissionMode::WorkspaceReadWrite, false);
    c.profile = SecurityProfile::Strict;
    assert!(matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Deny { .. }));
}
