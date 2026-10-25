//! Selected external files grant exactly one identity; ZIP lists are user content.
#![allow(clippy::unwrap_used)]
use super::*;
use aura_agent_domain::{AgentStartRequest, AgentSubmitRequest};
use aura_domain_common::{CommandInvocationId, WorkspacePath};
use aura_permission_domain::{ExternalGrantCapability, ExternalGrantTargetKind, ExternalGrantTerm};
use aura_windows_platform::NativeFileAccess;

mod boundaries;
mod external;
mod zip;

fn services(runtime: &crate::RuntimeState) -> &crate::RuntimeServices {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    services
}

fn start_request(
    session: SessionId,
    model: ModelProfileId,
    path: &std::path::Path,
    text: &str,
) -> AgentStartRequest {
    serde_json::from_value(json!({"session_id":session,"model_profile_id":model,"text":text,
        "mode":"normal","goal_budget":null,"attachments":[{"kind":"file","path":path}]}))
    .expect("existing file draft and start contract")
}

async fn finish(runtime: &crate::RuntimeState, started: &AgentRunProjection) {
    wait(
        runtime,
        &AgentRunRequest {
            session_id: started.turn.session_id,
            agent_run_id: started.run.agent_run_id,
        },
        |view| view.turn.state.terminal(),
    )
    .await;
}

fn request_text(provider: &Provider) -> String {
    let requests = provider.requests.lock().expect("requests");
    requests.first().expect("a real scripted provider request")["messages"]
        .as_array()
        .expect("chat messages")
        .iter()
        .filter(|message| message["role"] == "user")
        .filter_map(|message| message["content"].as_str())
        .collect::<Vec<_>>()
        .join("\n")
}
