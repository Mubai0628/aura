//! A step's request is frozen in the standard mode, whatever the model's settings allow: a turn
//! sets the mode its conversation chose. Allowing speed modes is kept with the model's other
//! settings, and a mode a profile named before turns chose their own still loads, never refused,
//! as a stored thinking level is not.
use std::{collections::BTreeMap, sync::Arc};

use aura_config::{AuraPaths, ConfigModule, ModuleConfigValidator, SettingsFacade};
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, ModelProfileId, ModelStepId, PromptBundleSnapshotId,
    UtcTimestamp,
};
use aura_model_config::{
    CreateModelExecutionSnapshot, ModelConfigService, ModelConfigValidator,
    create_model_from_catalog,
};
use aura_model_domain::{
    ModelApiFormat, ModelRegistryConfig, ModelSpeedMode, ProviderCatalogModelMode,
};
use tempfile::TempDir;

mod support;

const OPENAI: &str = "https://api.openai.com/v1";

/// OpenAI's GPT fixture, with `fast` and `flex` as models.dev lists them.
fn registry_with_model() -> (ModelRegistryConfig, ModelProfileId) {
    let listed = |tier: &str| ProviderCatalogModelMode {
        price: None,
        body: BTreeMap::from([("service_tier".to_owned(), tier.to_owned())]),
        headers: BTreeMap::new(),
    };
    let model = aura_model_domain::ProviderCatalogModelEntry {
        modes: BTreeMap::from([
            ("fast".to_owned(), listed("priority")),
            ("flex".to_owned(), listed("flex")),
        ]),
        ..support::catalog_model("gpt-fixture", None)
    };
    let catalog = support::catalog("openai", Some(OPENAI), vec![model]);
    let (mut registry, endpoints) =
        support::registry("openai", &[(OPENAI, ModelApiFormat::OpenAiResponses)]);
    let entry = &catalog.providers["openai"].models["gpt-fixture"];
    let hash = ContentHash::blake3(&serde_json::to_vec(entry).expect("model JSON"));
    let model_id = create_model_from_catalog(
        &mut registry,
        endpoints[0],
        &catalog,
        "openai",
        "gpt-fixture",
        &hash,
        UtcTimestamp::now(),
    )
    .expect("model from the catalog");
    (registry, model_id)
}

fn service(directory: &TempDir) -> ModelConfigService {
    let paths = AuraPaths::from_roots(
        directory.path().join("roaming/Aura"),
        directory.path().join("local/Aura"),
    );
    let validators = BTreeMap::<ConfigModule, Arc<dyn ModuleConfigValidator>>::from([(
        ConfigModule::Models,
        Arc::new(ModelConfigValidator) as Arc<dyn ModuleConfigValidator>,
    )]);
    ModelConfigService::new(
        SettingsFacade::open_with_validators(paths, validators).expect("model settings"),
    )
}

/// The speed mode a new step of the model is frozen with, its profile allowing speed modes or
/// not and naming `name`; and whether the saved settings still allow them.
fn frozen_with(allowed: bool, name: Option<&str>) -> (Option<ModelSpeedMode>, bool) {
    let directory = tempfile::tempdir().expect("temp root");
    let service = service(&directory);
    let (mut registry, model_id) = registry_with_model();
    let defaults = &mut registry.models.get_mut(&model_id).expect("model").request_defaults;
    defaults.allow_speed_modes = allowed;
    defaults.speed_mode = name.map(str::to_owned);
    service
        .update(1, |candidate| {
            *candidate = registry.clone();
            Ok(())
        })
        .expect("a stored mode is never refused");
    let saved = service.snapshot().expect("model settings").registry.models[&model_id]
        .request_defaults
        .allow_speed_modes;
    let frozen = service
        .create_execution_snapshot(CreateModelExecutionSnapshot {
            model_step_id: ModelStepId::new(),
            model_profile_id: model_id,
            context_snapshot_id: ContextProjectionSnapshotId::new(),
            prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
            historical_tool_call_wire_bindings: Vec::new(),
            credential_binding: None,
            endpoint_id: None,
        })
        .expect("execution snapshot")
        .request_options
        .speed_mode;
    (frozen, saved)
}

#[test]
fn a_step_is_frozen_in_the_standard_mode_and_allowing_is_kept() {
    for (allowed, name) in
        [(false, None), (true, None), (true, Some("fast")), (false, Some("flex"))]
    {
        assert_eq!(frozen_with(allowed, name), (None, allowed), "{allowed} {name:?}");
    }
}
