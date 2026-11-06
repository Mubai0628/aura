//! A model-settings write prepared before one of Aura's own changes took effect goes through the
//! parent journal on the revision it was applied to, and is refused as stale when the change
//! touched the same Provider.
use aura_domain_common::{CommandInvocationId, CorrelationId, ProviderId, Revision, UtcTimestamp};
use aura_model_config::{
    ModelConfigError, PreparedModelConfigMutation, create_provider, rename_provider,
};
use aura_model_domain::{CreateModelProviderInput, ModelProviderSource};
use aura_persistence::PersistentEventDraft;
use serde_json::json;

use super::super::{ModelConfigCommandCompletion, ModelConfigCommandPlan, ModelConfigJournalError};
use super::fixture::Fixture;

impl Fixture {
    /// Runs one change through the parent journal with the plan every model-settings command
    /// builds: the journal expects the revision the change was prepared on.
    fn journaled(
        &self,
        prepare: impl FnOnce(String) -> Result<PreparedModelConfigMutation<()>, ModelConfigError>,
    ) -> Result<ModelConfigCommandCompletion, ModelConfigJournalError> {
        let correlation_id = CorrelationId::new();
        self.coordinator.execute(CommandInvocationId::new(), correlation_id, |journal_id| {
            let prepared = prepare(journal_id.to_string())?;
            Ok(ModelConfigCommandPlan::Publish {
                prepared_bundle: Box::new(prepared.prepared_bundle.expect("a changed mutation")),
                expected_models_revision: Revision::new(prepared.previous.revision),
                reserved_models_revision: Some(prepared.candidate_revision),
                expected_aura_revision: None,
                reserved_aura_revision: None,
                credential_slot_creations: Vec::new(),
                session_binding_clears: Vec::new(),
                metadata_events: vec![PersistentEventDraft::system(
                    "ModelProviderRenamed",
                    correlation_id,
                    json!({"models_revision": prepared.candidate_revision}),
                )],
                projection_drafts: vec![self.model_settings_projection(
                    prepared.candidate_revision,
                    (*prepared.candidate_registry).clone(),
                )],
                success_result: json!({"revision": prepared.candidate_revision}),
            })
        })
    }

    fn create(&self, name: &str) -> ProviderId {
        let revision = self.models_revision();
        let mut created = None;
        self.journaled(|journal| {
            self.model_config.prepare_update(journal, revision, |registry| {
                created = Some(create_provider(
                    registry,
                    CreateModelProviderInput {
                        display_name: name.to_owned(),
                        source: ModelProviderSource::Custom { catalog_provider_id: None },
                    },
                    UtcTimestamp::now(),
                )?);
                Ok(())
            })
        })
        .expect("create Provider");
        created.expect("created Provider")
    }

    fn provider_name(&self, provider: ProviderId) -> String {
        let snapshot = self.model_config.snapshot().expect("models snapshot");
        snapshot.registry.providers[&provider].display_name.clone()
    }
}

fn rename(
    provider: ProviderId,
    name: &str,
) -> impl FnOnce(&mut aura_model_domain::ModelRegistryConfig) -> Result<(), ModelConfigError> {
    let name = name.to_owned();
    move |registry| rename_provider(registry, provider, name, UtcTimestamp::now())
}

/// Two Providers, then Aura's own change renaming the first; the revision before that change.
fn after_a_background_change() -> (Fixture, ProviderId, ProviderId, u64) {
    let fixture = Fixture::new();
    let found = fixture.create("A");
    let mine = fixture.create("B");
    let prepared_on = fixture.models_revision();
    fixture
        .journaled(|journal| {
            fixture.model_config.prepare_background_update(
                journal,
                prepared_on,
                rename(found, "Found"),
            )
        })
        .expect("Aura's own change");
    assert_eq!(fixture.models_revision(), prepared_on + 1);
    (fixture, found, mine, prepared_on)
}

#[test]
fn a_write_prepared_before_a_background_change_is_journaled_on_the_latest_revision() {
    let (fixture, found, mine, prepared_on) = after_a_background_change();
    fixture
        .journaled(|journal| {
            fixture.model_config.prepare_update(journal, prepared_on, rename(mine, "Mine"))
        })
        .expect("the stale write is applied over Aura's own change");
    assert_eq!(fixture.models_revision(), prepared_on + 2);
    assert_eq!(fixture.provider_name(found), "Found");
    assert_eq!(fixture.provider_name(mine), "Mine");
}

#[test]
fn a_write_to_the_provider_a_background_change_renamed_is_refused_as_stale() {
    let (fixture, found, _, prepared_on) = after_a_background_change();
    let refused = fixture
        .journaled(|journal| {
            fixture.model_config.prepare_update(journal, prepared_on, rename(found, "Mine"))
        })
        .expect_err("the same Provider");
    assert!(
        matches!(
            refused,
            ModelConfigJournalError::ModelConfig(ModelConfigError::Settings(
                aura_config::SettingsFacadeError::RevisionConflict { .. }
            ))
        ),
        "{refused}"
    );
    assert_eq!(fixture.models_revision(), prepared_on + 1);
    assert_eq!(fixture.provider_name(found), "Found");
}
