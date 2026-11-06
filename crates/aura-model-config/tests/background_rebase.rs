//! A model-settings write prepared before one of Aura's own changes took effect: applied on the
//! latest settings when only Aura's own changes came in between and none touched what the write
//! touches, refused as stale otherwise.
use std::collections::BTreeMap;
use std::sync::Arc;

use aura_config::{
    AuraPaths, ConfigModule, ModuleConfigValidator, SettingsFacade, SettingsFacadeError,
};
use aura_domain_common::{EndpointId, ProviderId};
use aura_model_config::{
    ModelConfigError, ModelConfigService, ModelConfigValidator, PreparedModelConfigMutation,
};
use aura_model_domain::{ModelApiFormat, ModelRegistryConfig};
use tempfile::TempDir;

mod support;

struct Fixture {
    _directory: TempDir,
    settings: Arc<SettingsFacade>,
    service: ModelConfigService,
    provider: ProviderId,
    endpoints: Vec<EndpointId>,
}

/// One Provider with three connections, at revision 2.
fn fixture() -> Fixture {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = AuraPaths::from_roots(
        directory.path().join("roaming/Aura"),
        directory.path().join("local/Aura"),
    );
    let validators = BTreeMap::<ConfigModule, Arc<dyn ModuleConfigValidator>>::from([(
        ConfigModule::Models,
        Arc::new(ModelConfigValidator) as Arc<dyn ModuleConfigValidator>,
    )]);
    let settings = SettingsFacade::open_with_validators(paths, validators).expect("model settings");
    let service = ModelConfigService::new(Arc::clone(&settings));
    let (registry, endpoints) = support::registry(
        "openai",
        &[
            ("https://a.example/v1", ModelApiFormat::OpenAiResponses),
            ("https://b.example/v1", ModelApiFormat::OpenAiResponses),
            ("https://c.example/v1", ModelApiFormat::OpenAiResponses),
        ],
    );
    let provider = registry.provider_order[0];
    service
        .update(1, |candidate| {
            *candidate = registry;
            Ok(())
        })
        .expect("initial model settings");
    assert_eq!(service.snapshot().expect("settings").revision, 2);
    Fixture { _directory: directory, settings, service, provider, endpoints }
}

type Mutation = Box<dyn FnOnce(&mut ModelRegistryConfig) -> Result<(), ModelConfigError>>;

fn rename_connection(endpoint: EndpointId, name: &str) -> Mutation {
    let name = name.to_owned();
    Box::new(move |registry| {
        registry.endpoints.get_mut(&endpoint).expect("connection").display_name = name;
        Ok(())
    })
}

fn rename_provider(provider: ProviderId, name: &str) -> Mutation {
    let name = name.to_owned();
    Box::new(move |registry| {
        registry.providers.get_mut(&provider).expect("provider").display_name = name;
        Ok(())
    })
}

impl Fixture {
    fn background(&self, expected: u64, mutation: Mutation) -> PreparedModelConfigMutation<()> {
        self.service
            .prepare_background_update("background".into(), expected, mutation)
            .expect("background change")
    }

    fn user(
        &self,
        expected: u64,
        mutation: Mutation,
    ) -> Result<PreparedModelConfigMutation<()>, ModelConfigError> {
        self.service.prepare_update("user".into(), expected, mutation)
    }

    /// Publishes and activates a prepared change, as the model-config journal does.
    fn take_effect(&self, prepared: PreparedModelConfigMutation<()>) -> u64 {
        let bundle = prepared.prepared_bundle.expect("a changed mutation has a bundle");
        self.service.publish_prepared_update(&bundle).expect("publish");
        self.service.activate_prepared_update(&bundle).expect("activate").revision
    }

    fn connection_name(&self, index: usize) -> String {
        let snapshot = self.service.snapshot().expect("settings");
        snapshot.registry.endpoints[&self.endpoints[index]].display_name.clone()
    }
}

fn assert_stale<T>(result: Result<T, ModelConfigError>, expected: u64, actual: u64) {
    match result {
        Err(ModelConfigError::Settings(SettingsFacadeError::RevisionConflict {
            module: ConfigModule::Models,
            expected: refused_expected,
            actual: refused_actual,
        })) => assert_eq!((refused_expected, refused_actual), (expected, actual)),
        Err(other) => panic!("expected the stale-revision refusal, got {other}"),
        Ok(_) => panic!("expected the stale-revision refusal, the write was prepared"),
    }
}

#[test]
fn a_write_prepared_before_a_background_change_to_another_connection_is_applied_on_the_latest() {
    let f = fixture();
    let background = f.background(2, rename_connection(f.endpoints[0], "Found"));
    assert_eq!(f.take_effect(background), 3);

    let write = f.user(2, rename_connection(f.endpoints[1], "Mine")).expect("applied over it");
    assert_eq!(write.previous.revision, 3, "prepared on the settings in effect");
    assert_eq!(write.candidate_revision.value(), 4);
    assert_eq!(f.take_effect(write), 4);
    assert_eq!(f.connection_name(0), "Found", "the background change is kept");
    assert_eq!(f.connection_name(1), "Mine");
}

#[test]
fn a_write_to_a_connection_a_background_change_also_changed_is_refused_as_stale() {
    let f = fixture();
    let background = f.background(2, rename_connection(f.endpoints[0], "Found"));
    f.take_effect(background);
    assert_stale(f.user(2, rename_connection(f.endpoints[0], "Mine")), 2, 3);
    assert_eq!(f.connection_name(0), "Found");
}

#[test]
fn a_user_change_in_between_refuses_a_stale_write() {
    let f = fixture();
    let first = f.user(2, rename_connection(f.endpoints[0], "First")).expect("current");
    assert_eq!(f.take_effect(first), 3);
    assert_stale(f.user(2, rename_connection(f.endpoints[1], "Second")), 2, 3);
}

#[test]
fn a_user_change_anywhere_in_between_refuses_the_write_and_later_writes_rebase_from_it() {
    let f = fixture();
    let found = f.background(2, rename_connection(f.endpoints[0], "Found"));
    f.take_effect(found);
    let user = f.user(3, rename_connection(f.endpoints[1], "Mine")).expect("current");
    f.take_effect(user);
    let learned = f.background(4, rename_connection(f.endpoints[2], "Learned"));
    assert_eq!(f.take_effect(learned), 5);

    assert_stale(f.user(2, rename_provider(f.provider, "Renamed")), 2, 5);
    let after_user = f.user(4, rename_provider(f.provider, "Renamed")).expect("only Aura's own");
    assert_eq!(after_user.previous.revision, 5);
}

#[test]
fn a_background_change_that_did_not_take_effect_does_not_count() {
    let f = fixture();
    let abandoned = f.background(2, rename_connection(f.endpoints[0], "Found"));
    let bundle = abandoned.prepared_bundle.expect("bundle");
    f.service.abort_prepared_update(&bundle).expect("abort");
    let user = f.user(2, rename_connection(f.endpoints[0], "Other")).expect("current");
    assert_eq!(f.take_effect(user), 3);

    assert_stale(f.user(2, rename_connection(f.endpoints[1], "Mine")), 2, 3);
}

#[test]
fn after_a_restart_a_stale_write_is_refused() {
    let f = fixture();
    let background = f.background(2, rename_connection(f.endpoints[0], "Found"));
    f.take_effect(background);

    let restarted = ModelConfigService::new(Arc::clone(&f.settings));
    assert_stale(
        restarted.prepare_update("user".into(), 2, rename_connection(f.endpoints[1], "Mine")),
        2,
        3,
    );
}

#[test]
fn a_write_expecting_a_revision_the_settings_have_not_reached_is_refused() {
    let f = fixture();
    let background = f.background(2, rename_connection(f.endpoints[0], "Found"));
    f.take_effect(background);

    assert_stale(f.user(4, rename_connection(f.endpoints[1], "Mine")), 4, 3);
}

#[test]
fn a_deletion_prepared_before_a_background_change_is_applied_when_it_touches_other_objects() {
    let f = fixture();
    let background = f.background(2, rename_connection(f.endpoints[0], "Found"));
    f.take_effect(background);

    assert_stale(f.service.prepare_delete_endpoint("delete".into(), 2, f.endpoints[0]), 2, 3);
    let deletion = f
        .service
        .prepare_delete_endpoint("delete".into(), 2, f.endpoints[1])
        .expect("applied over it");
    assert_eq!(deletion.previous.revision, 3);
    assert_eq!(deletion.candidate_revision.value(), 4);
    f.service.publish_prepared_update(&deletion.prepared_bundle).expect("publish");
    let after = f.service.activate_prepared_update(&deletion.prepared_bundle).expect("activate");
    assert_eq!(after.revision, 4);
    assert!(!after.registry.endpoints.contains_key(&f.endpoints[1]));
    assert_eq!(after.registry.endpoints[&f.endpoints[0]].display_name, "Found");
}
