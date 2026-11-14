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

/// The call's arguments in the tool's own input type; arguments that do not fit are refused in
/// Core's words, with what did not fit.
fn arguments<T: serde::de::DeserializeOwned>(handler: H, action: &Value) -> anyhow::Result<T> {
    serde_json::from_value(action.clone()).map_err(|error| {
        anyhow::anyhow!(
            "media.input_invalid: {} 的参数不符合要求（{}），什么都没读；按工具说明改正后再调用。",
            handler.name(),
            super::super::browser::outside_detail(&error.to_string(), 120)
        )
    })
}

/// The rule a frames request broke, by the code its check gives.
fn rule(code: &str) -> &'static str {
    match code {
        "media.max_side_invalid" => "max_side 要在 64 到 2048 之间（不填时是 1024）",
        _ => {
            "times 和 count 要给且只给一个：times 是 1 到 12 个不小于 0 的秒数，count 是 1 到 12 的整数"
        }
    }
}

/// A frames request in its own type, checked against its rules.
pub(super) fn frames(action: &Value) -> anyhow::Result<MediaFramesInput> {
    let input: MediaFramesInput = arguments(H::MediaFrames, action)?;
    input
        .validate()
        .map_err(|code| anyhow::anyhow!("{code}: {}，什么都没读；改正后再调用。", rule(code)))?;
    Ok(input)
}

/// A media file that could not be opened, named with why in words the model can act on. The
/// underlying error stays in the chain for callers that classify it.
fn unavailable(error: impl Into<anyhow::Error>) -> anyhow::Error {
    use aura_security_service::external_grants::ExternalGrantError as Grant;
    use aura_workspace::WorkspaceError;
    let error = error.into();
    let missing =
        matches!(error.downcast_ref::<WorkspaceError>(), Some(WorkspaceError::NotFound { .. }))
            || error.chain().any(|cause| {
                cause
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|io| io.kind() == std::io::ErrorKind::NotFound)
            });
    let (why, next) = if missing {
        ("文件不存在，可能已被移动或删除".to_owned(), "请确认文件位置后再调用")
    } else if matches!(error.downcast_ref::<WorkspaceError>(), Some(WorkspaceError::Offline)) {
        ("这个会话的工作区现在没有打开".to_owned(), "请让用户打开工作区后再调用")
    } else if matches!(
        error.downcast_ref::<Grant>(),
        Some(Grant::Revoked | Grant::Expired | Grant::Suspended | Grant::IdentityChanged)
    ) {
        ("这个外部文件的授权已经失效".to_owned(), "请让用户在这个会话里重新附加这个文件")
    } else {
        (
            super::super::browser::outside_detail(&error.to_string(), 200),
            "请确认文件还在、可以读取后再调用",
        )
    };
    error.context(format!(
        "media.source_unavailable: 打不开要读的视频文件（{why}），什么都没读；{next}。"
    ))
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
        let input = frames(action)?;
        *action = stored_form(&input)?;
    } else {
        let input: MediaProbeInput = arguments(handler, action)?;
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
    let path = call.operation.content.action["path"].as_str().ok_or_else(|| {
        anyhow::anyhow!(
            "media.path_missing: 这次调用的记录里没有文件路径（Aura 内部不一致），什么都没读；可以再调用一次。"
        )
    })?;
    let relative = WorkspacePath::parse(path).map_err(unavailable)?;
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
                "media.path_changed: 这次调用准备好之后，会话的工作区或要读的路径变了，什么都没读；请再调用一次。"
            );
            let file = service
                .workspace
                .resolver(*workspace_id)
                .map_err(unavailable)?
                .open_existing(&relative, NativeFileAccess::Read)
                .map_err(unavailable)?
                .file;
            let expected = file_target.as_ref().ok_or_else(|| {
                anyhow::anyhow!(
                    "media.identity_missing: 这次调用没有记下文件的身份，无法确认读到的是同一个文件，什么都没读；请再调用一次。"
                )
            })?;
            anyhow::ensure!(
                expected.identity_key
                    == aura_security_contract::file_object_key(&aura_workspace::domain_identity(
                        &file.info().identity
                    )),
                "media.identity_changed: 这次调用准备好之后视频文件被替换了，什么都没读；请再调用一次。"
            );
            file
        }
        [ResourceDescriptor::ExternalGrant { grant_id }] => {
            let record = service.external_grants.record(*grant_id)?;
            anyhow::ensure!(
                record.target_kind == ExternalGrantTargetKind::File && relative.is_root(),
                "media.single_file_required: 视频工具只读取用户授权的那一个文件，不读授权文件夹里的文件，什么都没读；请让用户在这个会话里附加要读的视频文件。"
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
                )
                .map_err(unavailable)?
                .file
        }
        _ => anyhow::bail!(
            "media.source_not_authorized: 这个文件不在工作区里，也没有在这个会话里授权，什么都没读；请先在这个会话中附加该外部文件。"
        ),
    };
    anyhow::ensure!(
        !file.info().is_directory,
        "media.not_a_file: 这个路径是文件夹，不是文件，什么都没读；请选择一个视频文件。"
    );
    let size = file.file().metadata().map_err(unavailable)?.len();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frames_request_that_breaks_a_rule_is_refused_with_that_rule() {
        let error = frames(&json!({"path": "clip.mp4", "count": 3, "max_side": 32}))
            .expect_err("a side below 64");
        assert_eq!(
            error.to_string(),
            "media.max_side_invalid: max_side 要在 64 到 2048 之间（不填时是 1024），什么都没读；改正后再调用。"
        );
        let error = frames(&json!({"path": "clip.mp4", "times": [1.0], "count": 3}))
            .expect_err("times and count together");
        assert!(
            error
                .to_string()
                .starts_with("media.frames_selection_invalid: times 和 count 要给且只给一个"),
            "{error}"
        );
        let error = frames(&json!({"path": "clip.mp4", "count": "three"})).expect_err("a word");
        let text = error.to_string();
        assert!(text.starts_with("media.input_invalid: media.frames 的参数不符合要求（"), "{text}");
        assert!(text.contains("invalid type"), "{text}");
    }

    #[test]
    fn a_file_that_cannot_be_opened_says_why_and_keeps_its_cause() {
        let path = WorkspacePath::parse("clips/gone.mp4").expect("a workspace path");
        let error = unavailable(aura_workspace::WorkspaceError::NotFound { path });
        assert_eq!(
            error.to_string(),
            "media.source_unavailable: 打不开要读的视频文件（文件不存在，可能已被移动或删除），什么都没读；请确认文件位置后再调用。"
        );
        assert!(error.downcast_ref::<aura_workspace::WorkspaceError>().is_some());
        let error = unavailable(std::io::Error::other("Access is denied. (os error 5)"));
        assert!(error.to_string().contains("（Access is denied. (os error 5)）"), "{error}");
    }
}
