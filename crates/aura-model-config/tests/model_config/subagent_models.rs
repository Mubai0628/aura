//! Allowing a provider's current models for sub-agents changes the models with an output limit
//! and leaves the others as they were, naming them; a single model without one is refused.
use crate::p05_config::{Fixture, facade, paths};
use aura_domain_common::{ModelProfileId, UtcTimestamp};
use aura_model_config::{ModelConfigError, ModelConfigService};

#[test]
fn allowing_a_provider_for_sub_agents_skips_the_models_without_an_output_limit() {
    let directory = tempfile::tempdir().expect("temp root");
    let service = ModelConfigService::new(facade(paths(&directory)));
    let mut fixture = Fixture::new();
    // A second model on the same endpoint, without an output limit.
    let unlimited = ModelProfileId::new();
    let mut model = fixture.registry.models[&fixture.model_id].clone();
    model.id = unlimited;
    model.provider_model_name = "unlimited-model".to_owned();
    model.request_defaults.max_output_tokens = None;
    fixture.registry.models.insert(unlimited, model);
    let endpoint = fixture.registry.endpoints.get_mut(&fixture.endpoint_id).expect("endpoint");
    endpoint.model_order.push(unlimited);
    service
        .update(1, |candidate| {
            *candidate = fixture.registry.clone();
            Ok(())
        })
        .expect("seed registry");

    let (after, done) = service
        .set_provider_current_models_subagent_availability(
            2,
            fixture.provider_id,
            true,
            UtcTimestamp::now(),
        )
        .expect("the batch changes what it can");
    assert_eq!(done.changed, vec![fixture.model_id]);
    assert_eq!(done.skipped, vec![unlimited]);
    assert!(after.registry.models[&fixture.model_id].available_for_subagents);
    assert!(!after.registry.models[&unlimited].available_for_subagents);
    // The single-model change still refuses it.
    assert!(matches!(
        service.set_model_subagent_availability(
            after.revision,
            unlimited,
            true,
            UtcTimestamp::now()
        ),
        Err(ModelConfigError::OutputLimitConfigurationRequired(id)) if id == unlimited
    ));
    // Turning the provider off again touches only the model that was on.
    let (_, off) = service
        .set_provider_current_models_subagent_availability(
            after.revision,
            fixture.provider_id,
            false,
            UtcTimestamp::now(),
        )
        .expect("the batch turns them off");
    assert_eq!((off.changed, off.skipped), (vec![fixture.model_id], vec![]));
}
