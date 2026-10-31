//! A workspace file's verified facts: root identity, each path's identity, kind and hashes.
use aura_domain_common::{SessionId, WorkspaceId, WorkspacePath};
use aura_workspace_domain::{FsStatRequest, WorkspaceFileTarget};
use serde_json::{Value, json};

pub fn facts(
    fs: &aura_fs_tools::FsToolService,
    runtime: &aura_workspace::WorkspaceRuntime,
    workspace: WorkspaceId,
    session: SessionId,
    paths: &[WorkspacePath],
) -> anyhow::Result<Value> {
    let resolver = runtime.resolver(workspace)?;
    resolver.verify_root_identity()?;
    let mut values = Vec::new();
    for path in paths {
        match resolver.resolve_existing(path) {
            Ok(resolved) => {
                let stat = fs.stat(FsStatRequest {
                    workspace_id: workspace,
                    session_id: session,
                    target: WorkspaceFileTarget { path: path.clone() },
                })?;
                let disk_hash = if stat.kind == aura_workspace_domain::FsNodeKind::File {
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
