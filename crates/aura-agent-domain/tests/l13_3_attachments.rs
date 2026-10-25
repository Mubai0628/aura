#![allow(clippy::unwrap_used)]
use aura_agent_domain::AgentMessageAttachment;
use aura_domain_common::{ArtifactId, ExternalGrantId};
use serde_json::json;

#[test]
fn external_files_require_a_typed_grant_and_round_trip_with_optional_user_listing() {
    let grant = ExternalGrantId::new();
    let listing = ArtifactId::new();
    let mut value = json!({"kind":"external_file","name":"bundle.zip",
        "path":"C:\\Downloads\\bundle.zip","byte_length":128,"external_grant_id":grant});
    let attachment: AgentMessageAttachment = serde_json::from_value(value.clone())
        .expect("an externally authorized file has its own attachment variant");
    assert_eq!(attachment.name(), "bundle.zip");
    assert_eq!(serde_json::to_value(attachment).unwrap(), value);
    value["listing_artifact_id"] = json!(listing);
    let attachment: AgentMessageAttachment = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(attachment).unwrap(), value);
    value.as_object_mut().unwrap().remove("external_grant_id");
    assert!(serde_json::from_value::<AgentMessageAttachment>(value.clone()).is_err());
    value["external_grant_id"] = json!(listing);
    assert!(
        serde_json::from_value::<AgentMessageAttachment>(value).is_err(),
        "artifact identity is not file authority"
    );
}

#[test]
fn old_workspace_file_facts_still_round_trip_without_external_authority() {
    let value = json!({"kind":"file","name":"spec.pdf","path":"docs/spec.pdf","byte_length":19});
    let attachment: AgentMessageAttachment = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(attachment).unwrap(), value);
    let mut with_authority = value;
    with_authority["external_grant_id"] = json!(ExternalGrantId::new());
    assert!(serde_json::from_value::<AgentMessageAttachment>(with_authority).is_err());
}
