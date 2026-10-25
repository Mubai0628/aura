use super::configuration_scope;
use aura_domain_common::{SessionId, WorkspaceId};
use serde_json::json;

#[test]
fn s07_http_credential_binding_accepts_absent_and_null_global_scopes() {
    for params in [json!({}), json!({"workspace_id":null,"session_id":null})] {
        assert_eq!(
            configuration_scope(&params).expect("the public nullable scope means no local scope"),
            (None, None)
        );
    }
}

#[test]
fn s07_http_credential_binding_preserves_explicit_scopes_and_rejects_malformed_ids() {
    let workspace = WorkspaceId::new();
    let session = SessionId::new();
    assert_eq!(
        configuration_scope(&json!({"workspace_id":workspace,"session_id":session}))
            .expect("explicit identities remain exact"),
        (Some(workspace), Some(session))
    );
    assert_eq!(
        configuration_scope(&json!({"workspace_id":workspace,"session_id":null}))
            .expect("workspace scope does not invent a session"),
        (Some(workspace), None)
    );
    for params in [
        json!({"workspace_id":false}),
        json!({"session_id":123}),
        json!({"workspace_id":""}),
        json!({"session_id":workspace}),
    ] {
        assert!(configuration_scope(&params).is_err(), "invalid identities must stay refused");
    }
}
