use super::*;

#[test]
fn s06_b6_a_read_grant_asks_only_for_the_missing_write_capability() {
    let mut c =
        s06_review_external_case(PermissionMode::ReadOnly, ResolvedCapability::ExternalFileRead);
    c.op.content.resources = vec![ResourceDescriptor::ExternalGrant {
        grant_id: aura_domain_common::ExternalGrantId::new(),
    }];
    c.op.content.capabilities.push(ResolvedCapability::ExternalFileWrite);
    c.backend.capabilities.insert(ResolvedCapability::ExternalFileWrite);
    c.verified_external = vec![ResolvedCapability::ExternalFileRead];
    c.refresh();
    let PolicyAuthorization::Ask { proposal } = authorize(&c.input(), &c.backend, None) else {
        panic!("write capability remains unapproved");
    };
    let projection = serde_json::to_value(proposal)
        .expect("serialize the approval's missing-capability projection");
    assert_eq!(
        projection["requested_capabilities"],
        serde_json::json!(["external_file_write"]),
        "the question does not request already-covered read authority"
    );
}
