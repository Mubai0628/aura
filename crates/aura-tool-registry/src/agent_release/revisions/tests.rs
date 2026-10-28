use super::*;

/// A changed definition fails here, naming the fingerprint its new revision publishes.
#[test]
fn every_definition_is_its_newest_published_revision() {
    let at = "2026-09-09T00:00:00Z".parse().expect("release timestamp");
    let changed = crate::agent_release::definitions::entries(at)
        .expect("definitions")
        .into_iter()
        .filter(|entry| {
            current(entry.handler.name())
                .is_ok_and(|row| row.fingerprint != entry.definition.definition_fingerprint.hex())
        })
        .map(|entry| {
            format!(
                "{} r{} {} {}",
                entry.handler.name(),
                entry.definition.tool_version.definition_revision.value(),
                entry.instance.tool_instance_id,
                entry.definition.definition_fingerprint.hex()
            )
        })
        .collect::<Vec<_>>();
    assert!(changed.is_empty(), "{changed:#?}");
}

#[test]
fn published_identity_cannot_be_reused_for_changed_model_contract() {
    let release = crate::agent_release::release().expect("current verified release");
    let entry = release.entry("http.request").expect("HTTP tool");
    for change in 0..3 {
        let mut changed = entry.clone();
        match change {
            0 => changed.definition.model_description.push_str(" changed"),
            1 => changed.definition.capabilities.clear(),
            _ => changed.definition.strict = false,
        }
        let d = changed.definition;
        changed.definition = aura_tool_domain::ToolDefinitionRecordV1::new(
            d.tool_id,
            d.tool_version,
            d.tool_package_id,
            d.package_key,
            d.tool_key,
            d.canonical_name,
            d.display_name,
            d.model_description,
            d.model_summary,
            d.input_schema,
            d.strict,
            d.result_contract,
            d.runtime,
            d.capabilities,
            d.resource_claims,
            d.audience,
            d.source_kind,
            d.execution_semantics,
            d.runtime_file_closure_hash,
            d.entity_revision,
            d.created_at,
        )
        .expect("valid changed contract with recomputed fingerprint");
        changed
            .definition
            .validate()
            .expect("the failure must be published identity reuse, not a broken hash");
        assert!(
            verify(&[changed]).is_err(),
            "published key may not mask changed description, capabilities or strict contract"
        );
    }
}

#[test]
fn candidate_configuration_does_not_make_immutable_release_identity_mutable() {
    let release = crate::agent_release::release().expect("release");
    let mut candidate = release
        .candidates()
        .into_iter()
        .find(|c| c.definition.canonical_name.as_str() == "http.request")
        .expect("HTTP candidate");
    candidate.instance.enabled = false;
    candidate.instance.overrides.timeout_ms = Some(60_000);
    candidate.instance.entity_revision = aura_domain_common::Revision::new(2);
    assert!(release.validate_candidate(&candidate, release.fingerprint()));
    candidate.instance.tool_instance_id = aura_domain_common::ToolInstanceId::new();
    assert!(!release.validate_candidate(&candidate, release.fingerprint()));
}
