//! These two routes exist only on the authenticated Main connection.
use aura_agent_domain::*;
use serde_json::Value;
pub(super) async fn dispatch(
    agent: &crate::agent_application::AgentApplication,
    method: &str,
    params: Value,
) -> anyhow::Result<Value> {
    match method {
        "media.host.read" => {
            Ok(serde_json::to_value(agent.media_read(serde_json::from_value(params)?).await?)?)
        }
        "media.host.result" => {
            let result: MediaHostResultV1 = serde_json::from_value(params)?;
            Ok(serde_json::to_value(MediaHostResultAck {
                accepted: agent.media().complete(result),
            })?)
        }
        _ => anyhow::bail!("media.unknown_host_method"),
    }
}
