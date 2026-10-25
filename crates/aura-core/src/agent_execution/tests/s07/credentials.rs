use super::*;
use aura_domain_common::{CommandInvocationId, CredentialRef};
use aura_persistence::ToolCredentialMutationIntentV1;
use aura_tool_domain::ToolCredentialCreateRequestV1;
use aura_windows_platform::{SecretBytes, WindowsCredentialStore};

const SECRET: &str = "s07-isolated-synthetic-http-secret";

struct Cleanup(CredentialRef);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = WindowsCredentialStore.delete(&self.0);
    }
}

fn credential(f: &Fixture, origin: &str) -> Cleanup {
    let release = aura_tool_registry::agent_release::release().expect("builtin identities");
    let entry = release.entry("http.request").expect("HTTP credential owner");
    let request: ToolCredentialCreateRequestV1 = serde_json::from_value(json!({
        "target": {
            "tool_instance_id":entry.instance.tool_instance_id,
            "expected_instance_revision":entry.instance.entity_revision,
            "credential_slot_key":"api_key",
            "placement":{"placement":"http_bearer_authorization"},
            "workspace_id":f.turn.workspace_id,
            "session_id":f.turn.session_id
        },
        "display_name":"S07 isolated local service",
        "scope":{"scope":"session","workspace_id":f.turn.workspace_id,"session_id":f.turn.session_id},
        "allowed_origins":[origin]
    })).expect("explicit allowed-origin credential metadata");
    let target = request.target.clone();
    let service = crate::tool_credential_service::ToolCredentialService::new(Arc::clone(
        &f.service().persistence,
    ));
    let secret = SecretBytes::new(SECRET.as_bytes().to_vec());
    let record = service
        .mutate(
            CommandInvocationId::new(),
            ToolCredentialMutationIntentV1::Create { request },
            Some(&secret),
        )
        .expect("create isolated synthetic credential")
        .credential
        .expect("credential metadata");
    let (_, reference) = f
        .service()
        .persistence
        .authorized_tool_secret_binding_v1(
            target.clone(),
            record.credential_id,
            record.credential_revision,
        )
        .expect("cleanup reference belongs to this isolated fixture");
    let owner = f.service().persistence.current_tool_principal().expect("local owner");
    let candidates = f
        .service()
        .persistence
        .query_tool_credential_candidates_v1(aura_persistence::ToolCredentialCandidateQueryV1 {
            authenticated_owner: owner,
            tool_instance_id: target.tool_instance_id,
            credential_slot_key: target.credential_slot_key.clone(),
            placement: target.placement.clone(),
            workspace_id: target.workspace_id,
            session_id: target.session_id,
            expected_instance_revision: target.expected_instance_revision,
        })
        .expect("real candidate authority");
    f.service()
        .persistence
        .commit_tool_instance_credential_settings_change(
            aura_persistence::ToolInstanceCredentialSettingsChangeRequestV1 {
                authenticated_owner: owner,
                tool_instance_id: target.tool_instance_id,
                credential_slot_key: target.credential_slot_key,
                placement: target.placement,
                expected_entity_revision: target.expected_instance_revision,
                expected_binding_revision: None,
                expected_candidate_set_revision: candidates.candidate_set_revision,
                credential_id: record.credential_id,
                credential_revision: record.credential_revision,
                workspace_id: target.workspace_id,
                session_id: target.session_id,
                changed_at: aura_domain_common::UtcTimestamp::now(),
                correlation_id: aura_domain_common::CorrelationId::new(),
            },
        )
        .expect("user explicitly binds the created credential");
    Cleanup(reference)
}

#[tokio::test]
async fn s07_n10_bound_credential_cannot_be_sent_to_another_origin() {
    let allowed = Server::start().await;
    let other = Server::start().await;
    let mut f = fixture(PermissionMode::FullAccess, &["http.request"]).await;
    let _cleanup = credential(&f, &allowed.url);
    let first = f.admit(&[("http.request", json!({"url":allowed.url,"credential":"api_key"}))]);
    let id = first.batch.calls[0].tool_call_id;
    let batch_id = first.batch.tool_batch_id;
    let group = f.execute(first).await;
    f.output(id);
    assert_eq!(allowed.count(), 1);
    assert!(allowed.requests.lock().expect("allowed request")[0].contains(SECRET));
    f.continue_after_tools(batch_id, &group);
    let second = f.admit(&[("http.request", json!({"url":other.url,"credential":"api_key"}))]);
    let id = second.batch.calls[0].tool_call_id;
    f.execute_rejecting(second).await;
    no_attempt(&f, id);
    assert_eq!(other.count(), 0, "origin mismatch must refuse before any request");
}

#[tokio::test]
async fn s07_n10_cross_origin_redirect_strips_the_bound_credential() {
    let allowed = Server::start().await;
    let other = Server::start().await;
    let f = fixture(PermissionMode::FullAccess, &["http.request"]).await;
    let _cleanup = credential(&f, &allowed.url);
    let call = f.admit(&[(
        "http.request",
        json!({
            "url":format!("{}/redirect/{}/destination",allowed.url,other.url),
            "credential":"api_key"
        }),
    )]);
    let id = call.batch.calls[0].tool_call_id;
    f.execute(call).await;
    f.output(id);
    assert!(allowed.requests.lock().expect("initial request")[0].contains(SECRET));
    assert_eq!(other.count(), 1);
    let requests = other.requests.lock().expect("redirect request");
    assert!(!requests[0].to_ascii_lowercase().contains("authorization:"));
    assert!(!requests[0].contains(SECRET));
}

#[tokio::test]
async fn s07_n10_credential_reflection_never_enters_the_model_result_or_history() {
    let server = Server::start().await;
    let f = fixture(PermissionMode::FullAccess, &["http.request"]).await;
    let _cleanup = credential(&f, &server.url);
    let batch = f.admit(&[(
        "http.request",
        json!({"url":format!("{}/echo",server.url),"credential":"api_key"}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    f.execute_rejecting(batch).await;
    assert_eq!(server.count(), 1, "credential was sent only to the configured origin");
    let store = f.service().broker.lock().expect("safe result journal");
    let db = store.database.connection();
    let call = aura_persistence_v2::tool::execution::call(db, id).expect("reflection result");
    assert!(call.attempt_id.is_some());
    assert!(!matches!(
        call.result.expect("paired refusal").outcome,
        ToolResultOutcomeV1::Succeeded
    ));
    let (_, output) =
        aura_persistence_v2::tool::result::execution_output(db, id).expect("model-visible refusal");
    assert!(!output.to_string().contains(SECRET));
    assert!(
        !serde_json::to_string(
            &aura_persistence_v2::context::history(db, f.turn.session_id).expect("durable history")
        )
        .expect("history serialization")
        .contains(SECRET)
    );
}
