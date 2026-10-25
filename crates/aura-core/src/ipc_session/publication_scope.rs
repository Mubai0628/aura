//! Typed file publication scheduling; physical/permission facts remain with the common owner.
use super::*;
use aura_domain_common::{ExternalGrantId, QuarantineRecordId};
use aura_permission_domain::{QuarantineReleaseTarget, ReleaseQuarantineInput};
use aura_workspace_domain::{ArtifactExportRequest, ArtifactExportTarget};

#[derive(Clone, Debug, Eq, PartialEq)]
enum Destination {
    Workspace(FileTargetLaneKey),
    External { grant: ExternalGrantId, path: WorkspacePath },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct PublicationScope {
    destination: Destination,
    quarantine: Option<QuarantineRecordId>,
}

impl PublicationScope {
    pub(super) fn overlaps_file(&self, other: &FileTargetLaneKey) -> bool {
        matches!(&self.destination, Destination::Workspace(mine) if mine.inner.overlaps(&other.inner))
    }

    pub(super) fn overlaps(&self, other: &Self) -> bool {
        if self.quarantine.is_some() && self.quarantine == other.quarantine {
            return true;
        }
        match (&self.destination, &other.destination) {
            (Destination::Workspace(mine), Destination::Workspace(theirs)) => {
                mine.inner.overlaps(&theirs.inner)
            }
            (
                Destination::External { grant: mine, path: a },
                Destination::External { grant: theirs, path: b },
            ) => mine == theirs && a == b,
            _ => false,
        }
    }
}

fn workspace(workspace: WorkspaceId, path: WorkspacePath) -> Destination {
    let root = RawWorkspace::of_id(workspace);
    Destination::Workspace(FileTargetLaneKey::of(root.clone(), vec![RawTarget::file(path, &root)]))
}

pub(super) fn scope(request: &JsonRpcRequest) -> Option<PublicationScope> {
    // Parameter validation and registry membership are both required. Invalid DTOs
    // stay ordered and keep the handler's original validation errors.
    let descriptor = aura_ipc::routes::core_route(&request.method)?;
    if descriptor.idempotency == aura_ipc::routes::RouteIdempotency::ReadOnly {
        return None;
    }
    match request.method.as_str() {
        aura_ipc::method::ARTIFACT_EXPORT => {
            let request: ArtifactExportRequest = typed_params(request)?;
            let destination = match request.target {
                ArtifactExportTarget::Workspace { path } => workspace(request.workspace_id, path),
                ArtifactExportTarget::ExternalGrant { external_grant_id, relative_path } => {
                    Destination::External { grant: external_grant_id, path: relative_path }
                }
            };
            Some(PublicationScope { destination, quarantine: None })
        }
        aura_ipc::method::SECURITY_QUARANTINE_RELEASE => {
            let request: ReleaseQuarantineInput = typed_params(request)?;
            let destination = match request.target {
                QuarantineReleaseTarget::Workspace { path } => {
                    workspace(request.workspace_id, path)
                }
                QuarantineReleaseTarget::ExternalGrant { external_grant_id, relative_path } => {
                    Destination::External { grant: external_grant_id, path: relative_path }
                }
            };
            Some(PublicationScope { destination, quarantine: Some(request.quarantine_id) })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::{ArtifactId, CorrelationId, SessionId};
    use aura_workspace_domain::{FsExpectedState, FsWriteFileRequest, WorkspaceFileTarget};
    fn request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: CorrelationId::new(),
            method: method.into(),
            params,
            command_invocation_id: None,
        }
    }
    #[test]
    fn publication_targets_coordinate_with_file_writes_without_blocking_siblings() {
        let workspace = WorkspaceId::new();
        let session = SessionId::new();
        let export = request(
            aura_ipc::method::ARTIFACT_EXPORT,
            serde_json::to_value(ArtifactExportRequest {
                artifact_id: ArtifactId::new(),
                workspace_id: workspace,
                session_id: session,
                target: ArtifactExportTarget::Workspace {
                    path: WorkspacePath::parse("one.txt").expect("path"),
                },
                expected: FsExpectedState::Absent,
            })
            .expect("actual DTO"),
        );
        let publication = lane_key_for(&export);
        assert!(matches!(publication, LaneKey::Publication(_)));
        for (path, blocked) in [("one.txt", true), ("two.txt", false)] {
            let write = request(
                aura_ipc::method::FS_WRITE_FILE,
                serde_json::to_value(FsWriteFileRequest {
                    workspace_id: workspace,
                    session_id: session,
                    target: WorkspaceFileTarget { path: WorkspacePath::parse(path).expect("path") },
                    content: "value".into(),
                    expected: FsExpectedState::Absent,
                    encoding: None,
                    line_ending: None,
                })
                .expect("actual file DTO"),
            );
            assert_eq!(publication.holds(&lane_key_for(&write)), blocked);
        }
        let invalid = request(
            aura_ipc::method::ARTIFACT_EXPORT,
            serde_json::json!({"workspace_id": workspace, "target":"one.txt"}),
        );
        assert_eq!(lane_key_for(&invalid), LaneKey::Connection(Lane::Serial));
    }
}
