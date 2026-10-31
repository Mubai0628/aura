//! File normalization and native identity checks follow fs.read, with media's narrower source
//! rule: a workspace object or one explicitly authorized external file, in every mode.
use super::super::*;
use crate::agent_execution::operation::stored_form;
use aura_agent_domain::*;
use aura_permission_domain::{ExternalGrantCapability, ExternalGrantTargetKind};
use aura_tool_registry::agent_release::AgentBuiltinHandler as H;
use aura_windows_platform::{NativeFileAccess, VerifiedFile};
use serde_json::{Value, json};

pub(super) struct Source {
    pub file: VerifiedFile,
    pub size: u64,
    pub media_type: String,
}

pub(in crate::agent_execution) fn resolve(
    service: &AgentExecution,
    handler: H,
    action: &mut Value,
    capabilities: &mut Vec<ResolvedCapability>,
    batch: &ToolBatchRequestV1,
    snapshot: &PermissionSnapshot,
) -> anyhow::Result<Vec<ResourceDescriptor>> {
    if handler == H::MediaFrames {
        let input: MediaFramesInput = serde_json::from_value(action.clone())?;
        input.validate().map_err(|error| {
            anyhow::anyhow!("{error}: 请指定最多12个有效时间，或1至12的均匀帧数；最长边64至2048。")
        })?;
        *action = stored_form(&input)?;
    } else {
        let input: MediaProbeInput = serde_json::from_value(action.clone())?;
        *action = stored_form(&input)?;
    }
    let resources =
        service.resolve_file_target(H::FileRead, action, capabilities, batch, snapshot)?;
    let allowed = match resources.as_slice() {
        [ResourceDescriptor::WorkspacePath { .. }] => true,
        [ResourceDescriptor::ExternalGrant { grant_id }] => {
            service.external_grants.record(*grant_id)?.target_kind == ExternalGrantTargetKind::File
        }
        _ => false,
    };
    anyhow::ensure!(
        allowed,
        "agent.path_outside_workspace:path: media tools require a workspace file or an explicitly authorized single external file"
    );
    Ok(resources)
}

pub(super) fn open(service: &AgentExecution, call: &CallContext) -> anyhow::Result<Source> {
    let path = call.operation.content.action["path"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("media.path_missing"))?;
    let relative = WorkspacePath::parse(path)?;
    let file = match call.operation.content.resources.as_slice() {
        [
            ResourceDescriptor::WorkspacePath {
                workspace_id,
                canonical_relative_path,
                file_target,
            },
        ] => {
            anyhow::ensure!(
                *workspace_id == call.snapshot.workspace_id && *canonical_relative_path == relative,
                "media.path_changed"
            );
            let file = service
                .workspace
                .resolver(*workspace_id)?
                .open_existing(&relative, NativeFileAccess::Read)?
                .file;
            let expected =
                file_target.as_ref().ok_or_else(|| anyhow::anyhow!("media.identity_missing"))?;
            anyhow::ensure!(
                expected.identity_key
                    == aura_security_contract::file_object_key(&aura_workspace::domain_identity(
                        &file.info().identity
                    )),
                "media.identity_changed: 视频文件已被替换，请重新读取。"
            );
            file
        }
        [ResourceDescriptor::ExternalGrant { grant_id }] => {
            let record = service.external_grants.record(*grant_id)?;
            anyhow::ensure!(
                record.target_kind == ExternalGrantTargetKind::File && relative.is_root(),
                "media.single_file_required: 视频只可读取已授权的那一个文件。"
            );
            service
                .external_grants
                .resolve_existing(
                    *grant_id,
                    call.snapshot.session_id,
                    &relative,
                    ExternalGrantCapability::Read,
                    NativeFileAccess::Read,
                    call.operation.content.identity.correlation_id,
                )?
                .file
        }
        _ => anyhow::bail!("media.source_not_authorized: 请先在这个会话中附加该外部文件。"),
    };
    anyhow::ensure!(!file.info().is_directory, "media.not_a_file: 请选择一个视频文件。");
    let size = file.file().metadata()?.len();
    let ext = file
        .info()
        .final_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let media_type = match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "ogv" | "ogg" => "video/ogg",
        "mkv" => "video/x-matroska",
        "avi" => "video/x-msvideo",
        _ => "application/octet-stream",
    }
    .to_owned();
    Ok(Source { file, size, media_type })
}

pub(in crate::agent_execution) fn inspect(
    service: &AgentExecution,
    call: &CallContext,
) -> anyhow::Result<Value> {
    let source = open(service, call)?;
    Ok(
        json!({"identity":aura_workspace::domain_identity(&source.file.info().identity),"byte_length":source.size}),
    )
}
