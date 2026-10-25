//! Authenticated desktop inspection/stop, with authority from the original session.
use crate::*;
use anyhow::ensure;
use aura_domain_common::SessionId;
use aura_windows_user_contract::*;

pub(super) async fn dispatch(
    services: &RuntimeServices,
    request: &JsonRpcRequest,
) -> Result<serde_json::Value> {
    let session: SessionId = serde_json::from_value(
        request.params.get("session_id").cloned().context("session required")?,
    )?;
    ensure!(
        services
            .persistence
            .workspace_session_projection()?
            .sessions
            .iter()
            .any(|v| v.session_id == session),
        "agent.session_unknown"
    );
    if request.method == "agent.tool_activity" {
        let input = serde_json::from_value(request.params.clone())?;
        return Ok(serde_json::to_value(services.agent()?.tool_activity(input).await?)?);
    }
    let input = if request.method == "agent.shell.stop" {
        let request: ShellStopRequest = serde_json::from_value(request.params.clone())?;
        services.commands()?.stop_call(session, request.input.tool_call_id).await?;
        ShellQueryInput {
            tool_call_id: request.input.tool_call_id,
            stdout_offset: 0,
            stderr_offset: 0,
            max_bytes: 4096,
        }
    } else {
        let request: ShellQueryRequest = serde_json::from_value(request.params.clone())?;
        request.input
    };
    Ok(serde_json::to_value(aura_persistence_v2::execution::shell::query(
        services.broker.lock()?.database.connection(),
        session,
        &input,
    )?)?)
}
