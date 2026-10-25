use super::*;

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum ToolSecuritySourceBuildErrorV1 {
    #[error("the admitted Tool definition reference is inconsistent")]
    DefinitionMismatch,
    #[error("the Registry Snapshot is invalid or differs from Batch Admission")]
    RegistryMismatch,
    #[error("the exact Registry binding is missing, duplicated, or inconsistent")]
    BindingMismatch,
}

/// Builds the only P04 source accepted for a concrete ToolCall. The call must
/// already have passed P07 admission; this function then independently
/// rechecks every identity and fingerprint against the unique binding in the
/// same immutable Registry Snapshot.
pub fn security_source_for_admitted_tool_call_v1(
    admitted_call: &ValidatedToolCallV1,
    registry: &ToolRegistrySnapshotV1,
) -> Result<SecuritySource, ToolSecuritySourceBuildErrorV1> {
    let reference = admitted_call
        .call
        .definition_ref
        .as_ref()
        .ok_or(ToolSecuritySourceBuildErrorV1::DefinitionMismatch)?;
    if registry.validate().is_err() || reference.registry_snapshot_ref != registry.snapshot_ref {
        return Err(ToolSecuritySourceBuildErrorV1::RegistryMismatch);
    }
    if reference.tool_id != admitted_call.definition.tool_id
        || reference.tool_version != admitted_call.definition.tool_version
        || admitted_call.definition.tool_id != admitted_call.binding.tool_id
        || admitted_call.definition.tool_version != admitted_call.binding.tool_version
        || admitted_call.definition.source_kind != admitted_call.binding.source_kind
        || admitted_call.definition.definition_fingerprint
            != admitted_call.binding.definition_fingerprint
    {
        return Err(ToolSecuritySourceBuildErrorV1::DefinitionMismatch);
    }

    // A call made through `tools.call` is bound by the snapshot's entry bindings only.
    let bindings =
        if admitted_call.binding.exposure == aura_tool_contract::ToolExposureV1::UnifiedEntry {
            &registry.entry_bindings
        } else {
            &registry.effective_bindings
        };
    let mut matches = bindings.iter().filter(|binding| {
        binding.tool_id == reference.tool_id && binding.tool_version == reference.tool_version
    });
    let Some(binding) = matches.next() else {
        return Err(ToolSecuritySourceBuildErrorV1::BindingMismatch);
    };
    if matches.next().is_some() || binding != &admitted_call.binding {
        return Err(ToolSecuritySourceBuildErrorV1::BindingMismatch);
    }

    let tool = ToolSecuritySourceRefV1::new(
        reference.clone(),
        binding.tool_instance_id,
        binding.source_kind,
        binding.definition_fingerprint.clone(),
        binding.source_contract_fingerprint.clone(),
    )
    .map_err(|_| ToolSecuritySourceBuildErrorV1::BindingMismatch)?;
    Ok(SecuritySource::Tool { tool: Box::new(tool) })
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum BatchAdmissionAuthorityIssueV1 {
    #[error("the batch invocation identity is inconsistent with Core state")]
    IdentityMismatch,
    #[error("the model invocation boundary is not uniquely established")]
    InvocationBoundaryInvalid,
    #[error("the frozen P08 control state revision or hash no longer matches")]
    ControlStateMismatch,
    #[error("the provider wire mapping is not unique")]
    ProviderMappingAmbiguous,
}

impl BatchAdmissionAuthorityIssueV1 {
    #[must_use]
    pub const fn error_code(self) -> ToolBatchErrorCode {
        match self {
            Self::IdentityMismatch => ToolBatchErrorCode::IdentityMismatch,
            Self::InvocationBoundaryInvalid => ToolBatchErrorCode::InvocationBoundaryInvalid,
            Self::ControlStateMismatch => ToolBatchErrorCode::ControlStateMismatch,
            Self::ProviderMappingAmbiguous => ToolBatchErrorCode::ProviderMappingAmbiguous,
        }
    }
}

pub trait BatchAdmissionAuthorityV1: Send + Sync {
    /// P08/Core read-only admission checkpoint. It verifies the exact frozen
    /// batch and must not replace any field with newer control state.
    fn verify_batch_admission(
        &self,
        batch: &ToolBatchRequestV1,
    ) -> Result<(), BatchAdmissionAuthorityIssueV1>;
}
