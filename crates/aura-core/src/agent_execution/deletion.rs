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
use std::path::{Path, PathBuf};

pub(super) const NOT_RECYCLABLE: &str = "fs.delete.not_recyclable: 回收站放不下这一项（或它所在的位置没有回收站），没有删除，它还在原处。要永久删除，请先告诉用户，再用 permanent: true，Aura 会先问用户。";

/// The object the operation names, found again now: where it is and what it is.
fn target(service: &AgentExecution, call: &CallContext) -> Result<(PathBuf, NativePathInfo)> {
    let content = &call.operation.content;
    for resource in &content.resources {
        match resource {
            ResourceDescriptor::WorkspacePath { canonical_relative_path, file_target, .. } => {
                ensure!(
                    !canonical_relative_path.is_root(),
                    "fs.delete.workspace_root: 不能删除整个工作区。"
                );
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
                        "fs.delete.replaced: 这个位置现在是另一个文件，没有删除；请重新查看后再删。"
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
                    ensure!(
                        aura_workspace::domain_identity(&info.identity) == *expected,
                        "fs.delete.replaced: 这个位置现在是另一个文件，没有删除；请重新查看后再删。"
                    );
                }
                return Ok((path, info));
            }
            _ => {}
        }
    }
    bail!("fs.delete.target_missing: 这次调用没有可删除的对象。")
}

/// The path Windows' shell takes: the short spelling of the same object.
fn shell_path(info: &NativePathInfo) -> PathBuf {
    PathBuf::from(super::protected_paths::strip_verbatim_prefix(
        &info.final_path.display().to_string(),
    ))
}

#[cfg(not(test))]
fn recycle(path: &Path) -> std::io::Result<RecycleOutcome> {
    aura_windows_platform::move_to_recycle_bin(path)
}

/// Tests never touch the user's Recycle Bin: the item moves to a bin folder beside it, and a name
/// with `not-recyclable` in it is one the bin cannot take.
#[cfg(test)]
fn recycle(path: &Path) -> std::io::Result<RecycleOutcome> {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    if name.contains("not-recyclable") {
        return Ok(RecycleOutcome::NotRecyclable);
    }
    let bin = path.parent().unwrap_or(path).join(".test-recycle-bin");
    std::fs::create_dir_all(&bin)?;
    std::fs::rename(path, bin.join(name))?;
    Ok(RecycleOutcome::Recycled)
}

pub(super) fn delete(
    service: &AgentExecution,
    call: &CallContext,
) -> Result<AgentFileDeleteResult> {
    let input: AgentFileDeleteInput =
        serde_json::from_value(call.operation.content.action.clone())?;
    let (path, info) = target(service, call)?;
    ensure!(
        !info.is_reparse_point,
        "fs.delete.link: 这是链接或挂载点，Aura 不删除它；请在资源管理器中处理。"
    );
    let shell = shell_path(&info);
    let outcome = if input.permanent {
        if info.is_directory {
            std::fs::remove_dir_all(&shell)?;
        } else {
            std::fs::remove_file(&shell)?;
        }
        AgentFileDeleteOutcome::Deleted
    } else {
        match recycle(&shell)? {
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
    if [
        "fs.delete.not_recyclable",
        "fs.delete.link",
        "fs.delete.replaced",
        "fs.delete.workspace_root",
    ]
    .iter()
    .any(|code| error.contains(code))
    {
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
