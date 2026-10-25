//! Archiving, restoring and deleting conversations, and removing what Aura kept for a
//! removed Workspace.
//!
//! Deleting a conversation removes its messages, process and
//! audit records; its Workspace's files, `.memories` and global memory stay. A deletion is
//! recorded before anything is removed — the Session's state becomes `deleting`, which takes it
//! out of the workspace/session projection — so a deletion cut short by a crash finishes the next
//! time Core starts. A removed Workspace is finished the same way from the primary registry's
//! `workspace.removed` notification.
mod purger;
pub(crate) use purger::Purger;

use crate::*;
use aura_workspace_domain::{SessionDeleteResult, SessionLifecycleRequest};

pub(crate) const METHODS: [&str; 3] = ["session.archive", "session.restore", "session.delete"];

pub(crate) async fn dispatch(
    services: &RuntimeServices,
    request: &JsonRpcRequest,
) -> anyhow::Result<serde_json::Value> {
    anyhow::ensure!(request.command_invocation_id.is_some(), "ipc.command_invocation_id_required");
    let SessionLifecycleRequest { session_id } = serde_json::from_value(request.params.clone())?;
    let purger = Purger::of(services);
    Ok(match request.method.as_str() {
        "session.archive" => {
            purger.ensure_idle(session_id, true).await?;
            serde_json::to_value(purger.set_archived(session_id, true).await?)?
        }
        "session.restore" => serde_json::to_value(purger.set_archived(session_id, false).await?)?,
        "session.delete" => {
            // A task still running is said first: stopping it is what lets it be archived.
            purger.ensure_idle(session_id, false).await?;
            purger.ensure_archived(session_id).await?;
            purger.delete(session_id).await?;
            serde_json::to_value(SessionDeleteResult { session_id })?
        }
        _ => anyhow::bail!("session.route_unknown"),
    })
}

/// Whether a workspace route answered with a completed Workspace removal.
pub(crate) fn removed_workspace(method: &str, value: &serde_json::Value) -> bool {
    matches!(method, "workspace.lifecycle.ack" | "workspace.lifecycle.resolve")
        && value["operation"] == "permanent_delete"
        && value["state"] == "completed"
}
