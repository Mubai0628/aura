//! Bounded delay at the actual file owner/effect boundary. No result or state is manufactured.
//! Compiled only for an acceptance-feature dev image, never a shipped release or default image.
use aura_domain_common::{CorrelationId, SecurityOperationId, SessionId, WorkspaceId};
use aura_persistence::barrier::{Barrier, Point};
use std::sync::OnceLock;

fn bound() -> &'static Barrier {
    static BARRIER: OnceLock<Barrier> = OnceLock::new();
    BARRIER.get_or_init(Barrier::from_environment)
}

pub fn hold_commit(workspace: WorkspaceId, session: SessionId, id: CorrelationId) {
    let identity =
        format!("aura.s045.file-boundary.v1/workspace={workspace}/session={session}/request={id}");
    let _ = bound().arrive_identified(Point::FileMutationCommit, "file_commit", Some(&identity));
}

pub fn hold_publication(published: bool, operation: SecurityOperationId, id: CorrelationId) {
    let identity = format!("aura.s045.file-boundary.v1/operation={operation}/request={id}");
    let point =
        if published { Point::FilePublicationPublished } else { Point::FilePublicationApplying };
    let _ = bound().arrive_identified(point, "file_publication", Some(&identity));
}
