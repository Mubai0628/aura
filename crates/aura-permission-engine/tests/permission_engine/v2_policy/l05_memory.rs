use super::*;

#[test]
fn repair_preview_and_apply_have_separate_authority() {
    for mode in
        [PermissionMode::ReadOnly, PermissionMode::WorkspaceReadWrite, PermissionMode::FullAccess]
    {
        let mut c = memory("memory.repair", mode, true);
        c.op.content.action = serde_json::json!({"input":{"phase":"preview"}});
        c.refresh();
        assert!(matches!(
            authorize(&c.input(), &c.backend, None),
            PolicyAuthorization::Allow { .. }
        ));
        c.op.content.action["input"]["phase"] = "apply".into();
        c.refresh();
        assert!(
            matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Deny { .. }),
            "an apply cannot borrow read authority"
        );
        c.op.content.capabilities = vec![ResolvedCapability::MemoryWrite];
        c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
        c.refresh();
        let answer = authorize(&c.input(), &c.backend, None);
        assert!(if mode == PermissionMode::ReadOnly {
            matches!(answer, PolicyAuthorization::Ask { .. })
        } else {
            matches!(answer, PolicyAuthorization::Allow { .. })
        });
    }
}

#[test]
fn readonly_session_consent_is_bound_to_the_single_vault_session_and_write_capability() {
    let mut c = memory("memory.create", PermissionMode::ReadOnly, false);
    let session_id = c.snapshot.session_id;
    c.approve(AuthorizationLifetime::Session { session_id });
    let ResourceDescriptor::MemoryVault { scope, vault_identity } = &c.op.content.resources[0]
    else {
        panic!("Vault")
    };
    c.grants[0].scope.memory_consent = Some(MemorySessionConsentRef {
        approval_id: aura_domain_common::ApprovalRequestId::new(),
        session_id,
        principal_id: c.op.content.identity.principal_id,
        scope: scope.clone(),
        vault_identity: vault_identity.clone(),
        approved_operation_fingerprint: c.op.fingerprint.clone(),
        grant_revision: Revision::INITIAL,
        revoked_grant_revision: None,
    });
    c.op.content.action_kind = "memory.update".into();
    c.op.content.action = serde_json::json!({"different":"note"});
    c.refresh();
    assert!(
        matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Allow { .. }),
        "a new write in the approved Vault reuses session consent"
    );
    for mutation in
        ["vault", "session", "revoked", "delete", "source", "extra_capability", "ordinary_path"]
    {
        let mut operation = c.op.clone();
        let mut grants = c.grants.clone();
        match mutation {
            "vault" => {
                operation.content.resources[0] = ResourceDescriptor::MemoryVault {
                    scope: MemoryVaultScope::Project { workspace_id: c.snapshot.workspace_id },
                    vault_identity: ContentHash::blake3(b"other"),
                }
            }
            "session" => {
                operation.content.identity.session_id = Some(aura_domain_common::SessionId::new())
            }
            "revoked" => grants[0].revoked_at = Some(c.input().now),
            "delete" => {
                operation.content.action_kind = "memory.delete".into();
                operation.content.capabilities = vec![ResolvedCapability::MemoryDelete];
            }
            "source" => operation.content.source = SecuritySourceRef::UserInterface,
            "extra_capability" => {
                operation.content.capabilities.push(ResolvedCapability::WorkspaceFileWrite)
            }
            "ordinary_path" => {
                operation.content.resources = vec![ResourceDescriptor::WorkspaceRoot {
                    workspace_id: c.snapshot.workspace_id,
                }]
            }
            _ => unreachable!(),
        }
        operation =
            SecuredOperation::new(operation.operation_id, operation.content).expect("operation");
        assert!(
            !memory_consent_covers(
                &grants[0],
                &operation.content.identity,
                &operation.content.source,
                &operation.content.action_kind,
                &operation.content.capabilities,
                &operation.content.resources
            ),
            "{mutation} cannot borrow consent"
        );
        if matches!(mutation, "source" | "extra_capability" | "ordinary_path") {
            let mut input = c.input();
            input.operation = &operation;
            input.grants = &grants;
            assert!(
                matches!(authorize(&input, &c.backend, None), PolicyAuthorization::Deny { .. }),
                "{mutation} must be denied"
            );
        }
    }
}

fn memory(action: &str, mode: PermissionMode, project: bool) -> Case {
    let mut c = Case::new(false);
    c.snapshot.mode = mode;
    c.op.content.action_kind = action.into();
    c.op.content.action = serde_json::json!({"scope":if project{"project"}else{"global"}});
    c.op.content.risks.clear();
    c.op.content.network = OperationNetwork::None;
    c.op.content.capabilities = vec![match action {
        "memory.create" | "memory.update" | "memory.link" | "memory.forget" | "memory.archive" => {
            ResolvedCapability::MemoryWrite
        }
        "memory.delete" => ResolvedCapability::MemoryDelete,
        _ => ResolvedCapability::MemoryRead,
    }];
    c.op.content.resources = vec![ResourceDescriptor::MemoryVault {
        scope: if project {
            MemoryVaultScope::Project { workspace_id: c.snapshot.workspace_id }
        } else {
            MemoryVaultScope::Global
        },
        vault_identity: ContentHash::blake3(b"TEMP fixture Vault identity"),
    }];
    c.backend.class = BackendClass::CoreBroker;
    c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
    c.backend.guarantees = [
        EnforcementGuarantee::AuthorizedReadsOnly,
        EnforcementGuarantee::AuthorizedWritesOnly,
        EnforcementGuarantee::BoundedOutput,
        EnforcementGuarantee::CredentialIsolation,
    ]
    .into();
    c.backend.known_limitations.clear();
    c.refresh();
    c
}

#[test]
fn memory_read_write_delete_matrix_is_the_same_for_global_and_project() {
    for project in [false, true] {
        for mode in [
            PermissionMode::ReadOnly,
            PermissionMode::WorkspaceReadWrite,
            PermissionMode::FullAccess,
        ] {
            for action in [
                "memory.search",
                "memory.read",
                "memory.list_recent",
                "memory.validate",
                "memory.create",
                "memory.update",
                "memory.link",
                "memory.forget",
                "memory.archive",
                "memory.delete",
            ] {
                let c = memory(action, mode, project);
                let answer = authorize(&c.input(), &c.backend, None);
                if action == "memory.delete" {
                    let PolicyAuthorization::Ask { proposal } = answer else {
                        panic!("delete always asks: {answer:?}");
                    };
                    assert_eq!(
                        proposal.allowed_decisions,
                        vec![ApprovalDecisionKind::AllowOnce, ApprovalDecisionKind::Deny]
                    );
                } else if c.op.content.capabilities == [ResolvedCapability::MemoryWrite]
                    && mode == PermissionMode::ReadOnly
                {
                    let PolicyAuthorization::Ask { proposal } = answer else {
                        panic!("ReadOnly writes ask: {answer:?}");
                    };
                    assert_eq!(
                        proposal.allowed_decisions,
                        vec![
                            ApprovalDecisionKind::AllowOnce,
                            ApprovalDecisionKind::AllowSession,
                            ApprovalDecisionKind::Deny
                        ]
                    );
                } else {
                    assert!(
                        matches!(answer, PolicyAuthorization::Allow { .. }),
                        "{action} {mode:?}: {answer:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn changed_source_extra_capability_ordinary_path_and_foreign_workspace_are_denied() {
    for mutation in ["source", "capability", "path", "other_project", "extra_vault", "action"] {
        let mut c = memory("memory.update", PermissionMode::FullAccess, true);
        match mutation {
            "source" => c.op.content.source = SecuritySourceRef::UserInterface,
            "capability" => c.op.content.capabilities.push(ResolvedCapability::WorkspaceFileWrite),
            "path" => {
                c.op.content.resources = vec![ResourceDescriptor::WorkspaceRoot {
                    workspace_id: c.snapshot.workspace_id,
                }]
            }
            "other_project" => {
                c.op.content.resources = vec![ResourceDescriptor::MemoryVault {
                    scope: MemoryVaultScope::Project {
                        workspace_id: aura_domain_common::WorkspaceId::new(),
                    },
                    vault_identity: ContentHash::blake3(b"TEMP other Vault"),
                }]
            }
            "extra_vault" => c.op.content.resources.push(c.op.content.resources[0].clone()),
            "action" => c.op.content.action_kind = "fs.write".into(),
            _ => unreachable!(),
        }
        c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
        c.refresh();
        assert!(
            matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Deny { .. }),
            "mutation {mutation} must be DENIED"
        );
    }
}

#[test]
fn ordinary_external_file_write_does_not_inherit_managed_global_vault_authority() {
    let mut c = memory("memory.update", PermissionMode::WorkspaceReadWrite, false);
    c.op.content.action_kind = "fs.write".into();
    c.op.content.capabilities = vec![ResolvedCapability::ExternalFileWrite];
    c.op.content.resources = vec![ResourceDescriptor::ExternalPath {
        canonical_path: CanonicalPath {
            display: "C:\\TEMP\\profile\\.aura\\memories\\rules.md".into(),
            utf16: vec![],
        },
        directory: false,
        recursive: false,
        identity: None,
        parent_identity: None,
        file_target: None,
    }];
    c.backend.capabilities = c.op.content.capabilities.iter().copied().collect();
    c.refresh();
    assert!(matches!(authorize(&c.input(), &c.backend, None), PolicyAuthorization::Ask { .. }));
}
