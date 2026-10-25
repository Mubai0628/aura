//! Workspace/session publication also reconciles process-local terminal owners.
use crate::*;
pub(super) async fn workspace_session(
    state: &RuntimeState,
    queue: &AsyncPriorityQueue,
    subscriptions: &ProjectionSubscriptions,
    payload: aura_workspace_domain::WorkspaceSessionProjection,
) -> Result<()> {
    if let StartupMode::Ready(services) = &state.startup_mode
        && let Ok(commands) = services.commands()
    {
        commands.close_removed_terminal_sessions(&payload.sessions).await;
    }
    let revision =
        Revision::new(state.workspace_session_revision.fetch_add(1, Ordering::AcqRel) + 1);
    if subscriptions.workspace_session.load(Ordering::Acquire) {
        let change = ProjectionChange::WorkspaceSession {
            runtime_instance_id: state.options.runtime_instance_id,
            projection_scope: ProjectionScope::Global,
            revision,
            payload,
        };
        send_notification(queue, QueuePriority::P2, method::PROJECTION_CHANGE, &change).await?;
    }
    Ok(())
}
