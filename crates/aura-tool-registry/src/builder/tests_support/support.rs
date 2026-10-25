use super::*;

pub(super) fn timestamp() -> UtcTimestamp {
    "2026-09-02T00:00:00Z".parse().expect("timestamp")
}

pub(super) fn version(revision: u64) -> ToolVersion {
    ToolVersion::new(None, Revision::new(revision)).expect("version")
}

pub(crate) fn package_candidate(
    tool_byte: u8,
    version_revision: u64,
    active: bool,
) -> RegistryCandidateV1 {
    let tool_id = ToolId::from_bytes([tool_byte; 16]);
    let tool_version = version(version_revision);
    let package_id = ToolPackageId::from_bytes([tool_byte.wrapping_add(40); 16]);
    let security_fingerprint =
        PackageSecurityFingerprintV1(CanonicalBlake3HashV1::blake3(&[tool_byte]));
    let source_contract_fingerprint = ToolSourceContractFingerprintV1::package(
        ToolSourceKindV1::Imported,
        aura_tool_contract::ToolContractFingerprintV1 {
            package_runtime_closure_hash: aura_tool_contract::PackageRuntimeClosureHashV1(
                CanonicalHashV1::sha256(&[version_revision as u8]),
            ),
            fingerprint: CanonicalHashV1::sha256(&[tool_byte, version_revision as u8]),
        },
    )
    .expect("package source fingerprint");
    let definition = ToolDefinitionRecordV1::new(
        tool_id,
        tool_version.clone(),
        Some(package_id),
        Some(PackageKey::new(format!("vendor.package{tool_byte}")).expect("package key")),
        Some(ToolKey::new(format!("tool{tool_byte}")).expect("tool key")),
        ToolCanonicalName::new(format!("vendor.tool{tool_byte}")).expect("name"),
        format!("Tool {tool_byte}"),
        format!("Execute tool {tool_byte}"),
        format!("Tool {tool_byte} summary"),
        ToolInputSchemaV1::new(json!({"type": "object", "additionalProperties": false}))
            .expect("schema"),
        false,
        ToolResultContractV1::Unit,
        ToolRuntimeDescriptorV1::ExternalSource {
            source_contract_key: format!("fixture.package.{tool_byte}.v{version_revision}"),
        },
        vec![],
        vec![],
        ToolAudienceV1::SubagentConfigurable,
        ToolSourceKindV1::Imported,
        ToolExecutionSemanticsV1 {
            automatic_retry_allowed: false,
            repeat_is_idempotent: true,
            may_create_persistent_effects: false,
        },
        CanonicalHashV1::sha256(&[version_revision as u8]),
        Revision::INITIAL,
        timestamp(),
    )
    .expect("definition");
    let instance = ToolInstanceRecordV1 {
        tool_instance_id: ToolInstanceId::from_bytes(
            [tool_byte.wrapping_add(version_revision as u8); 16],
        ),
        tool_id,
        tool_version,
        source_kind: ToolSourceKindV1::Imported,
        source_contract_fingerprint,
        enabled: true,
        runtime_health: ToolRuntimeHealthV1::Healthy,
        overrides: ToolInstanceOverridesV1 {
            timeout_ms: None,
            max_retries: None,
            max_concurrent_attempts: None,
            subagent_enabled: None,
        },
        entity_revision: Revision::INITIAL,
        created_at: timestamp(),
        updated_at: timestamp(),
    };
    let trust_record = ToolPackageTrustRecordV1 {
        tool_package_id: package_id,
        package_security_fingerprint: security_fingerprint.clone(),
        trusted: true,
        entity_revision: Revision::INITIAL,
        reviewed_at: timestamp(),
    };
    RegistryCandidateV1 {
        definition,
        instance,
        source_evidence: ToolSourceEligibilityEvidenceV1::Package {
            current_security_fingerprint: security_fingerprint,
            trust_record: Some(trust_record),
            payload_verified: true,
            conformance_passed: true,
            instance_configuration_ready: true,
            credential_binding_ready: true,
            deletion_pending: false,
        },
        active_for_unpinned: active,
    }
}

pub(super) fn core_candidate(tool_byte: u8) -> RegistryCandidateV1 {
    let release = crate::embedded_builtin_tool_definition_set_v1().expect("signed release");
    let entry = &release.entries()[usize::from(tool_byte) % release.entries().len()];
    RegistryCandidateV1 {
        definition: entry.definition_record(timestamp()).expect("definition"),
        instance: entry.instance_record(timestamp()),
        source_evidence: ToolSourceEligibilityEvidenceV1::VerifiedBuiltinRelease {
            release_manifest_hash: release.signed_release_hash().clone(),
            verification_evidence_hash: entry.source_contract_fingerprint.fingerprint().clone(),
        },
        active_for_unpinned: true,
    }
}

pub(super) fn child_required_core_candidate(tool_byte: u8) -> RegistryCandidateV1 {
    core_candidate(tool_byte)
}

pub(super) fn pin(
    tool_id: ToolId,
    tool_version: ToolVersion,
    state: ToolPinResolutionStateV1,
) -> ToolPinV1 {
    ToolPinV1 { tool_id, tool_version, state, safe_reason: None, revision: Revision::INITIAL }
}

pub(crate) fn base_input(candidates: Vec<RegistryCandidateV1>) -> ParentRegistryBuildInputV1 {
    ParentRegistryBuildInputV1 {
        registry_revision: Revision::INITIAL,
        scope: ToolRegistryScopeV1::Session {
            workspace_id: WorkspaceId::from_bytes([11; 16]),
            session_id: SessionId::from_bytes([12; 16]),
            turn_task_id: TurnTaskId::from_bytes([13; 16]),
            agent_run_id: AgentRunId::from_bytes([14; 16]),
        },
        settings: ToolSettingsSnapshotV1::default(),
        mode_permission_context_hash: CanonicalHashV1::sha256(b"mode-and-permission"),
        core_exposed_tool_ids: BTreeSet::new(),
        candidates,
        workspace_pins: vec![],
        session_pins: vec![],
        ephemeral_loads: vec![],
        created_at: timestamp(),
    }
}

pub(crate) fn search_fixture_input() -> ParentRegistryBuildInputV1 {
    base_input(vec![
        package_candidate(21, 1, true),
        package_candidate(22, 1, true),
        package_candidate(23, 1, true),
    ])
}

pub(super) fn child_input(
    parent_candidates: Vec<RegistryCandidateV1>,
    allowed: BTreeSet<ToolId>,
) -> ChildRegistryDerivationInputV1 {
    let mut parent_input = base_input(parent_candidates.clone());
    for candidate in &parent_candidates {
        if candidate.definition.source_kind == ToolSourceKindV1::BuiltIn {
            parent_input.core_exposed_tool_ids.insert(candidate.definition.tool_id);
        }
    }
    let parent_snapshot =
        build_parent_registry_snapshot(parent_input).expect("parent snapshot").snapshot;
    ChildRegistryDerivationInputV1 {
        registry_revision: Revision::new(2),
        scope: ToolRegistryScopeV1::ChildAgent {
            workspace_id: WorkspaceId::from_bytes([11; 16]),
            session_id: SessionId::from_bytes([12; 16]),
            turn_task_id: TurnTaskId::from_bytes([13; 16]),
            child_agent_run_id: AgentRunId::from_bytes([15; 16]),
        },
        parent_snapshot,
        parent_tools: parent_candidates
            .iter()
            .map(|candidate| ChildRegistryParentToolV1 {
                definition: candidate.definition.clone(),
                instance: candidate.instance.clone(),
            })
            .collect(),
        child_runtime_required_candidates: vec![],
        workspace_mode_hash: CanonicalHashV1::sha256(b"workspace-mode"),
        user_subagent_policy_revision: Revision::new(4),
        user_subagent_policy_hash: CanonicalHashV1::sha256(b"subagent-policy"),
        parent_permission_snapshot_id: PermissionSnapshotId::from_bytes([16; 16]),
        parent_permission_snapshot_hash: CanonicalHashV1::sha256(b"parent-permission"),
        system_hard_allowed_tool_ids: allowed.clone(),
        user_subagent_allowed_tool_ids: allowed.clone(),
        parent_permission_allowed_tool_ids: allowed.clone(),
        workspace_mode_allowed_tool_ids: allowed.clone(),
        parent_delegated_tool_ids: allowed.clone(),
        runtime_suitable_tool_ids: allowed.clone(),
        capacity_allowed_tool_ids: allowed,
        created_at: timestamp(),
    }
}
