use aura_domain_common::{CommandInvocationId, CorrelationId, Revision, UtcTimestamp};
use aura_model_config::create_provider;
use aura_model_domain::{CreateModelProviderInput, ModelProviderSource};
use aura_persistence::{ModelConfigMutationJournalState, PersistentEventDraft};
use serde_json::json;

use super::super::{ModelConfigCommandPlan, ModelConfigJournalError};
use super::fixture::Fixture;

#[test]
fn no_change_does_not_create_a_parent_journal() {
    let fixture = Fixture::new();
    let command_invocation_id = CommandInvocationId::new();
    let result = fixture
        .coordinator
        .execute(command_invocation_id, CorrelationId::new(), |_| {
            Ok(ModelConfigCommandPlan::NoChange {
                success_result: json!({"changed": false, "revision": "1"}),
            })
        })
        .expect("complete no-op");

    assert_eq!(result.success_result["changed"], false);
    assert!(result.projection_changes.is_empty());
    assert!(
        fixture
            .persistence
            .model_config_mutation_journal_by_command(command_invocation_id)
            .expect("query parent journal")
            .is_none()
    );
    assert_eq!(fixture.model_config.snapshot().expect("models snapshot").revision, 1);
}

#[test]
fn publish_commits_metadata_activates_config_and_replays_idempotently() {
    let fixture = Fixture::new();
    let command_invocation_id = CommandInvocationId::new();
    let correlation_id = CorrelationId::new();
    let completion = fixture
        .coordinator
        .execute(command_invocation_id, correlation_id, |journal_id| {
            let prepared =
                fixture.model_config.prepare_update(journal_id.to_string(), 1, |registry| {
                    create_provider(
                        registry,
                        CreateModelProviderInput {
                            display_name: "Example Provider".to_owned(),
                            source: ModelProviderSource::Custom { catalog_provider_id: None },
                        },
                        UtcTimestamp::now(),
                    )
                })?;
            let provider_id = prepared.value;
            let prepared_bundle = prepared
                .prepared_bundle
                .ok_or(ModelConfigJournalError::CredentialChildRequiresDedicatedCoordinator)?;
            Ok(ModelConfigCommandPlan::Publish {
                prepared_bundle: Box::new(prepared_bundle),
                expected_models_revision: Revision::INITIAL,
                reserved_models_revision: Some(Revision::new(2)),
                expected_aura_revision: None,
                reserved_aura_revision: None,
                credential_slot_creations: Vec::new(),
                session_binding_clears: Vec::new(),
                metadata_events: vec![PersistentEventDraft::system(
                    "ModelProviderCreated",
                    correlation_id,
                    json!({"provider_id": provider_id}),
                )],
                projection_drafts: vec![fixture.model_settings_projection(
                    Revision::new(2),
                    (*prepared.candidate_registry).clone(),
                )],
                success_result: json!({
                    "changed": true,
                    "revision": "2",
                    "provider_id": provider_id,
                }),
            })
        })
        .expect("commit model config parent journal");

    assert_eq!(completion.success_result["changed"], true);
    assert_eq!(completion.projection_changes.len(), 1);
    assert_eq!(fixture.persistence.event_count().expect("event count"), 1);
    let snapshot = fixture.model_config.snapshot().expect("activated models snapshot");
    assert_eq!(snapshot.revision, 2);
    assert_eq!(snapshot.registry.providers.len(), 1);
    let journal = fixture
        .persistence
        .model_config_mutation_journal_by_command(command_invocation_id)
        .expect("query parent journal")
        .expect("parent journal exists");
    assert_eq!(journal.state, ModelConfigMutationJournalState::Completed);

    let replay = fixture
        .coordinator
        .execute(command_invocation_id, CorrelationId::new(), |_| {
            panic!("idempotent replay must not prepare a second mutation")
        })
        .expect("replay committed command");
    assert_eq!(replay.success_result, completion.success_result);
    assert!(replay.projection_changes.is_empty());
    assert_eq!(fixture.persistence.event_count().expect("event count"), 1);
    assert_eq!(fixture.model_config.snapshot().expect("models snapshot").revision, 2);
}
