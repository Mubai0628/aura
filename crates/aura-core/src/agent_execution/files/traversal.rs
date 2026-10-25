use super::super::protected_paths::{ExternalPathClass, classify};
use super::*;

pub(super) fn scope(
    service: &AgentExecution,
    call: &CallContext,
) -> Result<Option<aura_fs_tools::FileReadScope>> {
    if !matches!(call.operation.content.action_kind.as_str(), "fs.list" | "fs.find" | "fs.search") {
        return Ok(None);
    }
    let target = call
        .operation
        .content
        .resources
        .iter()
        .filter_map(aura_security_contract::file_consent_target_of)
        .next();
    let root = if let Some(target) = target {
        std::path::PathBuf::from(target.canonical_path)
    } else if let Some(grant) = super::external_grant(call) {
        std::path::PathBuf::from(service.external_grants.record(grant)?.canonical_root.display)
    } else {
        anyhow::bail!("agent.traversal_verified_root_missing")
    };
    let root = aura_windows_platform::inspect_existing_path(&root, true)?;
    let root_class = classify(&root.final_path);
    let controls = Arc::clone(&service.control_resources);
    let root_is_control = controls.contains(&root);
    // A walk from outside Aura's data does not go in.
    let data = controls.data_folders();
    let root_in_data = crate::agent_execution::control_resources::in_folders(&data, &root);
    Ok(Some(aura_fs_tools::FileReadScope::new(move |info| {
        if !crate::agent_execution::file_targets::native_scope::within(&root, info) {
            return Some(FsSkippedScopeReason::TargetUnavailable);
        }
        if controls.contains(info) && !root_is_control {
            return Some(FsSkippedScopeReason::ControlAuthority);
        }
        let class = classify(&info.final_path);
        if class != ExternalPathClass::Ordinary && class != root_class {
            return Some(FsSkippedScopeReason::ProtectedScope);
        }
        if !root_in_data && crate::agent_execution::control_resources::in_folders(&data, info) {
            return Some(FsSkippedScopeReason::ProtectedScope);
        }
        None
    })))
}

pub(super) fn finish(
    call: &CallContext,
    value: Value,
    scope: Option<&aura_fs_tools::FileReadScope>,
) -> Result<Value> {
    let skipped = scope.map(aura_fs_tools::FileReadScope::skipped).unwrap_or_default();
    if skipped.is_empty() {
        return Ok(value);
    }
    Ok(match call.operation.content.action_kind.as_str() {
        "fs.list" => {
            let mut result: AgentFileListResult = serde_json::from_value(value)?;
            result.skipped_scopes = skipped;
            serde_json::to_value(result)?
        }
        "fs.find" => {
            let mut result: AgentFileFindResult = serde_json::from_value(value)?;
            result.skipped_scopes = skipped;
            result.complete = false;
            serde_json::to_value(result)?
        }
        "fs.search" => {
            let mut result: FsSearchTextResult = serde_json::from_value(value)?;
            result.skipped_file_count = result.skipped_file_count.max(skipped.len() as u64);
            result.skipped_scopes = skipped;
            match &mut result.completion {
                FsSearchCompletion::Partial { reasons } => {
                    if !reasons.contains(&FsSearchIncompleteReason::ScopeExcluded) {
                        reasons.push(FsSearchIncompleteReason::ScopeExcluded);
                    }
                }
                FsSearchCompletion::Complete => {
                    result.completion = FsSearchCompletion::Partial {
                        reasons: vec![FsSearchIncompleteReason::ScopeExcluded],
                    }
                }
            }
            serde_json::to_value(result)?
        }
        _ => value,
    })
}
