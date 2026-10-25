use super::*;
use crate::tool_reference::resolved as resolved_tool;

impl ExactToolExecutionAuthorityV1 {
    /// Builds the P04 Package Runtime scope only after independently binding an
    /// admitted call to the unique effective binding in the same immutable
    /// Registry Snapshot and to the live-verified exact Package payload.
    pub fn package_runtime_scope_for_admitted_call(
        &self,
        admitted_call: &ValidatedToolCallV1,
    ) -> Result<ResolvedScope, PersistenceError> {
        security_source_for_admitted_tool_call_v1(admitted_call, &self.registry)
            .map_err(|_| PersistenceError::ToolRegistryIntegrity)?;
        if admitted_call.definition != self.definition
            || admitted_call.binding.tool_instance_id != self.instance.tool_instance_id
            || admitted_call.binding.source_kind != self.instance.source_kind
            || admitted_call.binding.source_contract_fingerprint
                != self.instance.source_contract_fingerprint
            || resolved_tool(&admitted_call.call)?.registry_snapshot_ref
                != self.frozen.registry_snapshot_ref
            || resolved_tool(&admitted_call.call)?.tool_id != self.frozen.tool_id
            || resolved_tool(&admitted_call.call)?.tool_version != self.frozen.tool_version
            || self.frozen.tool_instance_id != self.instance.tool_instance_id
        {
            return Err(PersistenceError::ToolRegistryIntegrity);
        }

        let package = self.package.as_ref().ok_or(PersistenceError::ToolRegistryIntegrity)?;
        let tool_key =
            self.definition.tool_key.as_ref().ok_or(PersistenceError::ToolRegistryIntegrity)?;
        let source_runtime_closure_hash = match &admitted_call.binding.source_contract_fingerprint {
            ToolSourceContractFingerprintV1::UserCreated { tool_contract_fingerprint }
            | ToolSourceContractFingerprintV1::ModelCreated { tool_contract_fingerprint }
            | ToolSourceContractFingerprintV1::Imported { tool_contract_fingerprint } => {
                &tool_contract_fingerprint.package_runtime_closure_hash
            }
            ToolSourceContractFingerprintV1::BuiltIn { .. }
            | ToolSourceContractFingerprintV1::Mcp { .. }
            | ToolSourceContractFingerprintV1::Plugin { .. } => {
                return Err(PersistenceError::ToolRegistryIntegrity);
            }
        };
        if self.definition.tool_package_id != Some(package.tool_package_id)
            || package.manifest_tool.tool_key != *tool_key
            || self.definition.runtime_file_closure_hash != package.runtime_closure_hash.0
            || source_runtime_closure_hash != &package.runtime_closure_hash
        {
            return Err(PersistenceError::ToolRegistryIntegrity);
        }

        Ok(ResolvedScope::PackageRuntime {
            package: PackageRuntimeScopeV1 {
                tool_package_id: package.tool_package_id,
                package_version: package.package_version.clone(),
                runtime_closure_hash: package.runtime_closure_hash.clone(),
            },
        })
    }
}
