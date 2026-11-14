//! A workspace file's verified facts: root identity, each path's identity, kind and hashes; and
//! what a file read from the UI names, and the activity targets those facts prove.
use aura_domain_common::{SessionId, WorkspaceId, WorkspacePath};
use aura_security_contract::ResolvedCapability;
use aura_workspace_domain::{FsNodeKind, FsStatRequest, WorkspaceFileTarget};
use serde_json::{Value, json};

/// A UI file read's workspace, the conversation reading, the one path it reads and its capability.
pub type ReadScope = (WorkspaceId, Option<SessionId>, Vec<WorkspacePath>, Vec<ResolvedCapability>);

/// The scope a UI file read names. The file search of a conversation not yet started has its
/// workspace and no session.
pub fn read_scope(params: &Value) -> anyhow::Result<ReadScope> {
    let workspace = serde_json::from_value(params["workspace_id"].clone())?;
    let session = match params.get("session_id") {
        None | Some(Value::Null) => None,
        Some(session) => Some(serde_json::from_value(session.clone())?),
    };
    let path = params
        .get("root")
        .or_else(|| params.pointer("/target/path"))
        .or_else(|| params.get("path"));
    let path = path.map_or_else(
        || WorkspacePath::parse(WorkspacePath::ROOT).map_err(anyhow::Error::from),
        |v| serde_json::from_value(v.clone()).map_err(anyhow::Error::from),
    )?;
    Ok((workspace, session, vec![path], vec![ResolvedCapability::WorkspaceFileRead]))
}

/// `session` is the conversation reading. A file's facts include what that conversation's editor
/// holds, so a read without a conversation is prepared on folders alone.
pub fn facts(
    fs: &aura_fs_tools::FsToolService,
    runtime: &aura_workspace::WorkspaceRuntime,
    workspace: WorkspaceId,
    session: Option<SessionId>,
    paths: &[WorkspacePath],
) -> anyhow::Result<Value> {
    let resolver = runtime.resolver(workspace)?;
    resolver.verify_root_identity()?;
    let mut values = Vec::new();
    for path in paths {
        match resolver.resolve_existing(path) {
            Ok(resolved) => {
                let Some(session) = session else {
                    anyhow::ensure!(
                        resolved.info.is_directory && !resolved.info.is_reparse_point,
                        "a read without a conversation reads folders only"
                    );
                    values.push(json!({"path":path,"identity":aura_workspace::domain_identity(&resolved.info.identity),"kind":FsNodeKind::Directory}));
                    continue;
                };
                let stat = fs.stat(FsStatRequest {
                    workspace_id: workspace,
                    session_id: session,
                    target: WorkspaceFileTarget { path: path.clone() },
                })?;
                let disk_hash = if stat.kind == FsNodeKind::File {
                    use std::io::Read;
                    let mut file = resolver
                        .open_existing(path, aura_windows_platform::NativeFileAccess::Read)?;
                    let mut hasher = blake3::Hasher::new();
                    let mut block = [0u8; 65536];
                    loop {
                        let count = file.file.file_mut().read(&mut block)?;
                        if count == 0 {
                            break;
                        }
                        hasher.update(&block[..count]);
                    }
                    Some(aura_domain_common::ContentHash::new(
                        aura_domain_common::ContentHashAlgorithm::Blake3,
                        hasher.finalize().to_hex().to_string(),
                    )?)
                } else {
                    None
                };
                values.push(json!({"path":path,"identity":aura_workspace::domain_identity(&resolved.info.identity),"kind":stat.kind,"hash":stat.content_hash,"disk_hash":disk_hash,"editor":stat.active_editor_session}));
            }
            Err(aura_workspace::WorkspaceError::NotFound { .. }) => {
                // New nested directories may have absent intermediate parents;
                // bind the nearest existing ancestor without following escapes.
                let mut ancestor = path.clone();
                let parent = loop {
                    let text =
                        ancestor.as_str().rsplit_once('/').map_or(WorkspacePath::ROOT, |(p, _)| p);
                    ancestor = WorkspacePath::parse(text)?;
                    match resolver.resolve_existing(&ancestor) {
                        Ok(parent) => break parent,
                        Err(aura_workspace::WorkspaceError::NotFound { .. })
                            if !ancestor.is_root() => {}
                        Err(e) => return Err(e.into()),
                    }
                };
                values.push(json!({"path":path,"absent":true,"parent":ancestor,"parent_identity":aura_workspace::domain_identity(&parent.info.identity)}));
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(json!({"root_identity":resolver.root_identity(),"paths":values}))
}

/// Copies the identities `facts()` already verified into the activity contract's target shape (R1/R3).
///
/// WHY THIS IS A COPY AND NOT A SECOND READ
/// `facts()` above is the one place that really verifies identity: it calls
/// `resolver.verify_root_identity()` and `resolver.resolve_existing(path)` and reports the real
/// `FileIdentity`. This function only reinterprets that already-produced JSON, so publishing
/// costs no additional file I/O, opens no handle and takes no lock.
///
/// WHY THE KEY IS DERIVED THE SAME WAY THE OTHER PUBLISHERS DERIVE IT
/// The platform identity is two opaque strings (`volume_serial_number`, `file_id`). They are
/// folded into the index's compact `(u32, u64)` key space with the same canonical digest the
/// Agent publisher uses (`agent_activity::identity_key`), so a Main row and an Agent row for one
/// physical file land on the **same** `ActivityResourceKey`. A truncated hash or a path string
/// would not.
///
/// A target with no verified identity (an absent path) is reported against its prospective
/// parent's verified identity, which is the honest position fact for a file that does not exist
/// yet, rather than being passed off as a real object or silently dropped.
pub fn verified_targets_of(
    paths: &[WorkspacePath],
    pre_state: Option<&Value>,
) -> Vec<crate::agent_activity::MainFsTarget> {
    let entries = pre_state
        .and_then(|value| value.get("paths"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            let entry = entries.get(index);
            let identity = entry.and_then(|entry| entry.get("identity")).and_then(|value| {
                serde_json::from_value::<aura_domain_common::FileIdentity>(value.clone()).ok()
            });
            let directory = entry
                .and_then(|entry| entry.get("kind"))
                .and_then(|value| serde_json::from_value::<FsNodeKind>(value.clone()).ok())
                .is_some_and(|kind| kind == FsNodeKind::Directory);
            crate::agent_activity::MainFsTarget {
                path: path.as_str().to_owned(),
                target: identity.as_ref().map(|identity| {
                    aura_activity_domain::ActivityResourceTarget::VerifiedObject {
                        identity: identity.clone(),
                        ancestors: Vec::new(),
                        subtree: directory,
                    }
                }),
            }
        })
        .collect()
}
