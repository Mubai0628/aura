use super::*;

const RESERVED_AURA_NAMESPACES: &[&str] = &[
    "agent",
    "agents",
    "artifact",
    "commands",
    "context",
    "fs",
    "git",
    "goal",
    "memory",
    "normal",
    "plan",
    "process",
    "query",
    "shell",
    "task_lineage",
    "terminal",
    "todo",
    "tools",
    "workspace_init",
    "web",
];

pub(super) fn validate_candidates(
    candidates: &mut [RegistryCandidateV1],
) -> Result<(), RegistryBuildError> {
    candidates.sort_by(|left, right| {
        left.definition.tool_id.cmp(&right.definition.tool_id).then_with(|| {
            canonical_sha256(&left.definition.tool_version)
                .expect("validated ToolVersion is canonical")
                .hex()
                .cmp(
                    canonical_sha256(&right.definition.tool_version)
                        .expect("validated ToolVersion is canonical")
                        .hex(),
                )
        })
    });
    for candidate in candidates.iter() {
        candidate.definition.validate().map_err(|_| RegistryBuildError::CandidateIdentity)?;
        candidate
            .instance
            .overrides
            .validate()
            .map_err(|_| RegistryBuildError::CandidateIdentity)?;
        if candidate.definition.tool_id != candidate.instance.tool_id
            || candidate.definition.tool_version != candidate.instance.tool_version
            || candidate.definition.source_kind != candidate.instance.source_kind
            || candidate.instance.source_contract_fingerprint.source_kind()
                != candidate.instance.source_kind
        {
            return Err(RegistryBuildError::CandidateIdentity);
        }
        validate_source_evidence(candidate)?;
    }
    for pair in candidates.windows(2) {
        if pair[0].definition.tool_id == pair[1].definition.tool_id
            && pair[0].definition.tool_version == pair[1].definition.tool_version
        {
            return Err(RegistryBuildError::DuplicateExactCandidate);
        }
    }
    let mut names = BTreeMap::<&str, ToolId>::new();
    let mut active = BTreeSet::new();
    for candidate in candidates {
        if let Some(existing) =
            names.insert(candidate.definition.canonical_name.as_str(), candidate.definition.tool_id)
            && existing != candidate.definition.tool_id
        {
            return Err(RegistryBuildError::DuplicateCanonicalName);
        }
        if candidate.active_for_unpinned && !active.insert(candidate.definition.tool_id) {
            return Err(RegistryBuildError::DuplicateActiveVersion);
        }
    }
    Ok(())
}

pub(super) fn validate_source_evidence(
    candidate: &RegistryCandidateV1,
) -> Result<(), RegistryBuildError> {
    match (&candidate.definition.source_kind, &candidate.source_evidence) {
        (
            ToolSourceKindV1::BuiltIn,
            ToolSourceEligibilityEvidenceV1::VerifiedAgentBuiltinRelease { release_manifest_hash },
        ) => {
            let release = crate::agent_release::release()
                .map_err(|_| RegistryBuildError::CandidateSourceEvidence)?;
            if release.validate_candidate(candidate, release_manifest_hash) {
                Ok(())
            } else {
                Err(RegistryBuildError::CandidateSourceEvidence)
            }
        }
        (
            ToolSourceKindV1::BuiltIn,
            ToolSourceEligibilityEvidenceV1::VerifiedBuiltinRelease {
                release_manifest_hash,
                verification_evidence_hash,
            },
        ) => {
            let release = embedded_builtin_tool_definition_set_v1()
                .map_err(|_| RegistryBuildError::CandidateSourceEvidence)?;
            let entry = release
                .entries()
                .iter()
                .find(|entry| {
                    entry.definition.tool_id == candidate.definition.tool_id
                        && entry.definition.tool_version == candidate.definition.tool_version
                        && entry.tool_instance_id == candidate.instance.tool_instance_id
                })
                .ok_or(RegistryBuildError::CandidateSourceEvidence)?;
            let expected_definition = entry
                .definition_record(candidate.definition.created_at)
                .map_err(|_| RegistryBuildError::CandidateSourceEvidence)?;
            let expected_instance = entry.instance_record(candidate.instance.created_at);
            if release.signed_release_hash() != release_manifest_hash
                || entry.source_contract_fingerprint.fingerprint() != verification_evidence_hash
                || candidate.definition != expected_definition
                || candidate.instance.tool_id != expected_instance.tool_id
                || candidate.instance.tool_version != expected_instance.tool_version
                || candidate.instance.source_kind != expected_instance.source_kind
                || candidate.instance.source_contract_fingerprint
                    != expected_instance.source_contract_fingerprint
                || candidate.instance.enabled != expected_instance.enabled
                || candidate.instance.runtime_health != expected_instance.runtime_health
                || candidate.instance.overrides != expected_instance.overrides
                || candidate.instance.entity_revision != expected_instance.entity_revision
            {
                return Err(RegistryBuildError::CandidateSourceEvidence);
            }
            Ok(())
        }
        (
            ToolSourceKindV1::UserCreated
            | ToolSourceKindV1::ModelCreated
            | ToolSourceKindV1::Imported,
            ToolSourceEligibilityEvidenceV1::Package { trust_record, .. },
        ) => {
            let package_id = candidate
                .definition
                .tool_package_id
                .ok_or(RegistryBuildError::CandidateSourceEvidence)?;
            if trust_record.as_ref().is_some_and(|record| record.tool_package_id != package_id) {
                return Err(RegistryBuildError::CandidateSourceEvidence);
            }
            if candidate.definition.audience
                == aura_tool_contract::ToolAudienceV1::ChildRuntimeRequired
            {
                return Err(RegistryBuildError::CandidateSourceEvidence);
            }
            Ok(())
        }
        (
            ToolSourceKindV1::Mcp | ToolSourceKindV1::Plugin,
            ToolSourceEligibilityEvidenceV1::ExternalSource { .. },
        ) => {
            if candidate.definition.audience
                == aura_tool_contract::ToolAudienceV1::ChildRuntimeRequired
            {
                return Err(RegistryBuildError::CandidateSourceEvidence);
            }
            Ok(())
        }
        _ => Err(RegistryBuildError::CandidateSourceEvidence),
    }
}

pub(super) fn ineligibility_reason(
    candidate: &RegistryCandidateV1,
) -> Option<RegistryExclusionReasonV1> {
    if !candidate.instance.enabled {
        return Some(RegistryExclusionReasonV1::Disabled);
    }
    if candidate.instance.runtime_health != ToolRuntimeHealthV1::Healthy {
        return Some(RegistryExclusionReasonV1::RuntimeUnavailable);
    }
    match &candidate.source_evidence {
        ToolSourceEligibilityEvidenceV1::VerifiedBuiltinRelease { .. }
        | ToolSourceEligibilityEvidenceV1::VerifiedAgentBuiltinRelease { .. } => None,
        ToolSourceEligibilityEvidenceV1::Package {
            current_security_fingerprint,
            trust_record,
            payload_verified,
            conformance_passed,
            instance_configuration_ready,
            credential_binding_ready,
            deletion_pending,
        } => {
            if *deletion_pending {
                return Some(RegistryExclusionReasonV1::DeletionPending);
            }
            if !payload_verified {
                return Some(RegistryExclusionReasonV1::PayloadUnavailable);
            }
            if !conformance_passed {
                return Some(RegistryExclusionReasonV1::ConformanceRequired);
            }
            if !instance_configuration_ready {
                return Some(RegistryExclusionReasonV1::InstanceConfigurationRequired);
            }
            if !credential_binding_ready {
                return Some(RegistryExclusionReasonV1::CredentialBindingRequired);
            }
            let trusted = trust_record.as_ref().is_some_and(|record| {
                record.trusted
                    && record.package_security_fingerprint == *current_security_fingerprint
            });
            (!trusted).then_some(RegistryExclusionReasonV1::TrustRequired)
        }
        ToolSourceEligibilityEvidenceV1::ExternalSource {
            source_enabled,
            source_trusted,
            source_runtime_healthy,
        } => {
            if !source_enabled {
                Some(RegistryExclusionReasonV1::Disabled)
            } else if !source_trusted {
                Some(RegistryExclusionReasonV1::TrustRequired)
            } else if !source_runtime_healthy {
                Some(RegistryExclusionReasonV1::RuntimeUnavailable)
            } else {
                None
            }
        }
    }
}

pub(super) fn is_reserved_namespace(canonical_name: &str) -> bool {
    canonical_name
        .split('.')
        .next()
        .is_some_and(|namespace| RESERVED_AURA_NAMESPACES.contains(&namespace))
}
