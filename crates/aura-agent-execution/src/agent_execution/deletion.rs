//! `fs.delete`. The object is the one the operation was authorized for —
//! a workspace file or folder, one under a session grant, or an outside path the user approved — and
//! it has to still be that object. It goes to the Recycle Bin; one the bin cannot take stays where it
//! is and the call says so. Only `permanent`, which the user approved for this one call, deletes for
//! good.
use super::*;
use anyhow::{Result, bail, ensure};
use aura_permission_domain::ExternalGrantCapability;
use aura_windows_platform::{NativeFileAccess, NativePathInfo, RecycleOutcome};
use aura_workspace_domain::{AgentFileDeleteInput, AgentFileDeleteOutcome, AgentFileDeleteResult};
use std::path::PathBuf;

pub(super) const NOT_RECYCLABLE: &str = "fs.delete.not_recyclable: 回收站放不下这一项（或它所在的位置没有回收站），没有删除，它还在原处。要永久删除，请先告诉用户，再用 permanent: true，Aura 会先问用户。";
const WORKSPACE_ROOT: &str = "fs.delete.workspace_root: 不能删除整个工作区，没有删除任何东西；请只删除其中具体的文件或文件夹。";
const REPLACED: &str = "fs.delete.replaced: 这个位置现在是另一个文件，没有删除；请重新查看后再删。";
const LINK: &str = "fs.delete.link: 这是链接或挂载点，Aura 不删除它，它还在原处；如果确实要删，请让用户在资源管理器中处理。";
const TARGET_MISSING: &str = "fs.delete.target_missing: 这次调用没有指向可删除的对象，没有删除任何东西；请用 fs.list 确认路径后再调用。";

/// The refusals that leave the object where it was, each with how the model's refusal classes it.
/// The text of each begins with its code.
pub(super) const REFUSALS: [(&str, aura_domain_common::ErrorCategory); 5] = [
    ("fs.delete.not_recyclable", aura_domain_common::ErrorCategory::Unavailable),
    ("fs.delete.link", aura_domain_common::ErrorCategory::Validation),
    ("fs.delete.replaced", aura_domain_common::ErrorCategory::Conflict),
    ("fs.delete.workspace_root", aura_domain_common::ErrorCategory::Validation),
    ("fs.delete.target_missing", aura_domain_common::ErrorCategory::Validation),
];

/// The object the operation names, found again now: where it is and what it is.
fn target(service: &AgentExecution, call: &CallContext) -> Result<(PathBuf, NativePathInfo)> {
    let content = &call.operation.content;
    for resource in &content.resources {
        match resource {
            ResourceDescriptor::WorkspacePath { canonical_relative_path, file_target, .. } => {
                ensure!(!canonical_relative_path.is_root(), WORKSPACE_ROOT);
                let resolver = service.workspace.resolver(call.snapshot.workspace_id)?;
                let resolved = resolver.resolve_existing(canonical_relative_path)?;
                let info =
                    aura_windows_platform::inspect_existing_path(&resolved.absolute_path, true)?;
                if let Some(expected) = file_target {
                    ensure!(
                        expected.identity_key
                            == aura_security_contract::file_object_key(
                                &aura_workspace::domain_identity(&info.identity)
                            ),
                        REPLACED
                    );
                }
                return Ok((resolved.absolute_path, info));
            }
            ResourceDescriptor::ExternalGrant { grant_id } => {
                let relative = WorkspacePath::parse(
                    content.action["path"].as_str().unwrap_or(WorkspacePath::ROOT),
                )?;
                let file = service.external_grants.resolve_existing(
                    *grant_id,
                    call.snapshot.session_id,
                    &relative,
                    ExternalGrantCapability::Delete,
                    NativeFileAccess::Metadata,
                    content.identity.correlation_id,
                )?;
                let info = file.file.info().clone();
                return Ok((info.final_path.clone(), info));
            }
            ResourceDescriptor::ExternalPath { canonical_path, identity, .. }
            | ResourceDescriptor::SystemPath { canonical_path, identity, .. }
            | ResourceDescriptor::SensitiveUserPath { canonical_path, identity, .. } => {
                let path = PathBuf::from(&canonical_path.display);
                let info = aura_windows_platform::inspect_existing_path(&path, true)?;
                if let Some(expected) = identity {
                    ensure!(aura_workspace::domain_identity(&info.identity) == *expected, REPLACED);
                }
                return Ok((path, info));
            }
            _ => {}
        }
    }
    bail!(TARGET_MISSING)
}

/// The path Windows' shell takes: the short spelling of the same object.
fn shell_path(info: &NativePathInfo) -> PathBuf {
    PathBuf::from(super::protected_paths::strip_verbatim_prefix(
        &info.final_path.display().to_string(),
    ))
}

pub(super) fn delete(
    service: &AgentExecution,
    call: &CallContext,
) -> Result<AgentFileDeleteResult> {
    let input: AgentFileDeleteInput =
        serde_json::from_value(call.operation.content.action.clone())?;
    let (path, info) = target(service, call)?;
    ensure!(!info.is_reparse_point, LINK);
    let shell = shell_path(&info);
    let outcome = if input.permanent {
        if info.is_directory {
            std::fs::remove_dir_all(&shell)?;
        } else {
            std::fs::remove_file(&shell)?;
        }
        AgentFileDeleteOutcome::Deleted
    } else {
        match super::recycle::move_to_recycle_bin(&shell)? {
            RecycleOutcome::Recycled => AgentFileDeleteOutcome::Recycled,
            RecycleOutcome::NotRecyclable => bail!(NOT_RECYCLABLE),
        }
    };
    Ok(AgentFileDeleteResult {
        path: super::protected_paths::strip_verbatim_prefix(&path.display().to_string()),
        was_folder: info.is_directory,
        outcome,
    })
}

/// Whether the call deleted what it named: gone now means applied; still there means nothing
/// happened; anything else stays unknown, so the call is never replayed on its own.
pub(super) fn side_effect(
    service: &AgentExecution,
    call: &CallContext,
    output: &Result<serde_json::Value>,
    invoked: bool,
) -> aura_execution_contract::SideEffectStateV1 {
    use aura_execution_contract::SideEffectStateV1 as State;
    if !invoked {
        return State::NoEffect;
    }
    let error = match output {
        Ok(_) => return State::FullyApplied,
        Err(error) => error.to_string(),
    };
    // Refused before anything was touched.
    if REFUSALS.iter().any(|(code, _)| error.contains(code)) {
        return State::NoEffect;
    }
    let permanent = call.operation.content.action["permanent"].as_bool() == Some(true);
    match target(service, call) {
        // The Recycle Bin takes an item whole or not at all: still there is nothing done.
        Ok(_) if !permanent => State::NoEffect,
        // A folder deleted for good may be partly gone.
        _ => State::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each refusal's text begins with a code the refusal list holds, so the call is recorded as
    /// having changed nothing and the model reads the refusal itself; and each says so.
    #[test]
    fn every_refusal_begins_with_a_listed_code_and_says_nothing_was_deleted() {
        for text in [NOT_RECYCLABLE, WORKSPACE_ROOT, REPLACED, LINK, TARGET_MISSING] {
            let (code, words) = text.split_once(": ").expect("a code, then words");
            assert!(REFUSALS.iter().any(|(listed, _)| *listed == code), "{code}");
            assert!(words.contains("没有删除") || words.contains("不删除"), "{text}");
        }
    }
}
