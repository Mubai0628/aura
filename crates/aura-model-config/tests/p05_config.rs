use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    sync::Arc,
};

use aura_config::{
    AuraPaths, ConfigModule, ConfigSource, ModuleConfigDocument, ModuleConfigValidator,
    SettingsFacade, encode_document,
};
use aura_domain_common::{
    ContextProjectionSnapshotId, CredentialId, EndpointId, ModelProfileId, ModelStepId,
    PromptBundleSnapshotId, ProviderId, Revision, TransportAttemptId, UtcTimestamp,
};
use aura_model_config::{
    CreateModelExecutionSnapshot, LearnChatOutputTokenFieldAliasResult, ModelConfigError,
    ModelConfigService, ModelConfigValidationError, ModelConfigValidator,
    credential_binding_fingerprint, credential_ref_for_credential, endpoint_fingerprint,
    learn_chat_output_token_field_alias, validate_model_registry,
};
use aura_model_domain::{
    AddManualModelInput, CatalogMatchState, ChatInstructionRole, ChatOutputTokenField,
    DEFAULT_HISTORICAL_SNAPSHOT_MAX_TOKENS, DEFAULT_MAX_PARALLEL_MODEL_REQUESTS,
    DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS, EndpointAuthScheme, EndpointCompatibilityOverrides,
    ModelApiFormat, ModelContextWindowConfig, ModelCredentialSlot, ModelDiscoveryMode,
    ModelEndpointConfig, ModelEndpointTimeouts, ModelProfileConfig, ModelProfileOrigin,
    ModelProviderConfig, ModelProviderSource, ModelRegistryConfig, ModelRequestDefaults,
    ProviderCatalogEntry, ProviderCatalogModelCapabilities, ProviderCatalogModelEntry,
    ProviderCatalogSnapshot, UpdateModelEndpointInput,
};
use aura_runtime_contract::BundleNamespace;
use tempfile::TempDir;

struct Fixture {
    provider_id: ProviderId,
    endpoint_id: EndpointId,
    model_id: ModelProfileId,
    registry: ModelRegistryConfig,
}

/// A snapshot request for a new step of `model_profile_id`, with no history, credential or
/// other connection.
fn step_of(
    model_profile_id: ModelProfileId,
    context_snapshot_id: ContextProjectionSnapshotId,
    prompt_bundle_snapshot_id: PromptBundleSnapshotId,
) -> CreateModelExecutionSnapshot {
    CreateModelExecutionSnapshot {
        model_step_id: ModelStepId::new(),
        model_profile_id,
        context_snapshot_id,
        prompt_bundle_snapshot_id,
        historical_tool_call_wire_bindings: Vec::new(),
        credential_binding: None,
        endpoint_id: None,
    }
}

#[test]
fn output_token_alias_learning_is_sorted_idempotent_and_conflict_safe() {
    let mut fixture = Fixture::new();
    let endpoint = fixture.registry.endpoints.get_mut(&fixture.endpoint_id).expect("endpoint");
    endpoint.api_format = ModelApiFormat::OpenAiChatCompletions;
    endpoint.chat_output_token_field = Some(ChatOutputTokenField::MaxCompletionTokens);
    let attempt_id = TransportAttemptId::new();
    let learned_at = UtcTimestamp::now();

    let first = learn_chat_output_token_field_alias(
        &mut fixture.registry,
        fixture.endpoint_id,
        "Example-MixedCase",
        ChatOutputTokenField::MaxCompletionTokens,
        ChatOutputTokenField::MaxTokens,
        attempt_id,
        learned_at,
    )
    .expect("learn alias");
    let LearnChatOutputTokenFieldAliasResult::Learned(alias) = first else {
        panic!("first learning must persist a new alias")
    };
    assert_eq!(alias.learned_from_transport_attempt_id, attempt_id);

    let duplicate = learn_chat_output_token_field_alias(
        &mut fixture.registry,
        fixture.endpoint_id,
        "Example-MixedCase",
        ChatOutputTokenField::MaxCompletionTokens,
        ChatOutputTokenField::MaxTokens,
        TransportAttemptId::new(),
        UtcTimestamp::now(),
    )
    .expect("same learning is idempotent");
    assert_eq!(duplicate, LearnChatOutputTokenFieldAliasResult::AlreadyPresent(alias.clone()));
    let persisted = &fixture
        .registry
        .endpoints
        .get(&fixture.endpoint_id)
        .expect("endpoint")
        .compatibility
        .as_ref()
        .expect("compatibility")
        .output_token_field_aliases;
    assert_eq!(persisted, &[alias]);

    let error = learn_chat_output_token_field_alias(
        &mut fixture.registry,
        fixture.endpoint_id,
        "Example-MixedCase",
        ChatOutputTokenField::MaxTokens,
        ChatOutputTokenField::MaxCompletionTokens,
        TransportAttemptId::new(),
        UtcTimestamp::now(),
    )
    .expect_err("opposite learning must not overwrite the existing alias");
    assert!(matches!(error, ModelConfigError::OutputTokenAliasConfiguredFieldChanged(_)));
}

impl Fixture {
    fn new() -> Self {
        let provider_id = ProviderId::new();
        let endpoint_id = EndpointId::new();
        let model_id = ModelProfileId::new();
        let now = UtcTimestamp::now();
        let provider = ModelProviderConfig {
            id: provider_id,
            display_name: "Example Provider".to_owned(),
            slug: "example-provider".parse().expect("valid Provider slug"),
            source: ModelProviderSource::ModelsDev { catalog_provider_id: "example".to_owned() },
            endpoint_order: vec![endpoint_id],
            created_at: now,
            updated_at: now,
            default_endpoint_id: None,
            backup_endpoint_ids: vec![],
        };
        let endpoint = ModelEndpointConfig {
            id: endpoint_id,
            revision: Revision::INITIAL,
            provider_id,
            display_name: "Primary Endpoint".to_owned(),
            slug: "primary".parse().expect("valid Endpoint slug"),
            api_format: ModelApiFormat::OpenAiResponses,
            base_url: "https://api.example.test/v1".to_owned(),
            discovery_mode: ModelDiscoveryMode::Automatic,
            chat_output_token_field: None,
            chat_instruction_role: None,
            auth_scheme: EndpointAuthScheme::None,
            openai_organization_id: None,
            openai_project_id: None,
            anthropic_workspace_id: None,
            timeouts: ModelEndpointTimeouts {
                connect_ms: 15_000,
                first_event_ms: 120_000,
                stream_idle_ms: 120_000,
                total_ms: None,
            },
            max_parallel_model_requests: DEFAULT_MAX_PARALLEL_MODEL_REQUESTS,
            model_order: vec![model_id],
            compatibility: None,
            created_at: now,
            updated_at: now,
        };
        let model = ModelProfileConfig {
            id: model_id,
            revision: Revision::INITIAL,
            endpoint_id,
            provider_model_name: "Example-MixedCase".to_owned(),
            display_name: "Example Model".to_owned(),
            notes: None,
            origin: ModelProfileOrigin::Discovered,
            enabled: true,
            available_for_subagents: false,
            catalog: None,
            context: ModelContextWindowConfig {
                catalog_window_tokens: None,
                user_override_window_tokens: None,
                user_override_revision: 0,
                runtime_corrected_window_tokens: None,
                runtime_correction_basis: None,
                runtime_effective_window_tokens: DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS,
                historical_snapshot_max_tokens: DEFAULT_HISTORICAL_SNAPSHOT_MAX_TOKENS,
            },
            request_defaults: ModelRequestDefaults {
                max_output_tokens: Some(4_096),
                ..ModelRequestDefaults::default()
            },
            created_at: now,
            updated_at: now,
        };
        Self {
            provider_id,
            endpoint_id,
            model_id,
            registry: ModelRegistryConfig {
                provider_order: vec![provider_id],
                providers: BTreeMap::from([(provider_id, provider)]),
                endpoints: BTreeMap::from([(endpoint_id, endpoint)]),
                models: BTreeMap::from([(model_id, model)]),
            },
        }
    }
}

fn paths(directory: &TempDir) -> AuraPaths {
    AuraPaths::from_roots(
        directory.path().join("roaming/Aura"),
        directory.path().join("local/Aura"),
    )
}

fn facade(paths: AuraPaths) -> Arc<SettingsFacade> {
    let validators = BTreeMap::<ConfigModule, Arc<dyn ModuleConfigValidator>>::from([(
        ConfigModule::Models,
        Arc::new(ModelConfigValidator) as Arc<dyn ModuleConfigValidator>,
    )]);
    SettingsFacade::open_with_validators(paths, validators).expect("model settings")
}

fn document(registry: &ModelRegistryConfig, revision: u64) -> ModuleConfigDocument {
    let values = serde_json::from_value(serde_json::to_value(registry).expect("registry JSON"))
        .expect("registry object");
    ModuleConfigDocument { schema_version: 1, revision, values }
}

#[test]
fn validates_identity_order_references_and_exact_model_names() {
    let fixture = Fixture::new();
    validate_model_registry(&fixture.registry).expect("valid registry");

    let mut mismatched = fixture.registry.clone();
    mismatched.providers.get_mut(&fixture.provider_id).expect("provider").id = ProviderId::new();
    assert!(matches!(
        validate_model_registry(&mismatched),
        Err(ModelConfigValidationError::MapKeyMismatch { collection: "providers", .. })
    ));

    let mut missing_order = fixture.registry.clone();
    missing_order.provider_order.clear();
    assert!(matches!(
        validate_model_registry(&missing_order),
        Err(ModelConfigValidationError::MissingOrderEntry { .. })
    ));

    let mut missing_reference = fixture.registry.clone();
    missing_reference.models.get_mut(&fixture.model_id).expect("model").endpoint_id =
        EndpointId::new();
    missing_reference
        .endpoints
        .get_mut(&fixture.endpoint_id)
        .expect("endpoint")
        .model_order
        .clear();
    assert!(matches!(
        validate_model_registry(&missing_reference),
        Err(ModelConfigValidationError::MissingEndpoint { .. })
    ));

    let second_id = ModelProfileId::new();
    let mut duplicate = fixture.registry.clone();
    let mut second = duplicate.models.get(&fixture.model_id).expect("model").clone();
    second.id = second_id;
    duplicate.models.insert(second_id, second);
    duplicate
        .endpoints
        .get_mut(&fixture.endpoint_id)
        .expect("endpoint")
        .model_order
        .push(second_id);
    assert!(matches!(
        validate_model_registry(&duplicate),
        Err(ModelConfigValidationError::DuplicateProviderModelName { .. })
    ));
}

#[test]
fn rejects_stale_effective_context_window() {
    let fixture = Fixture::new();
    let mut stale = fixture.registry.clone();
    let model = stale.models.get_mut(&fixture.model_id).expect("model");
    model.context.user_override_window_tokens = Some(64_000);
    model.context.user_override_revision = 1;
    assert!(matches!(
        validate_model_registry(&stale),
        Err(ModelConfigValidationError::StaleEffectiveWindow {
            expected: 64_000,
            actual: DEFAULT_MODEL_CONTEXT_WINDOW_TOKENS,
            ..
        })
    ));
}

#[test]
fn hot_reload_accepts_valid_models_and_rejects_invalid_models_without_replacing_lkg() {
    let directory = tempfile::tempdir().expect("temp root");
    let paths = paths(&directory);
    let settings = facade(paths.clone());
    let service = ModelConfigService::new(Arc::clone(&settings));
    let fixture = Fixture::new();

    service
        .update(1, |candidate| {
            *candidate = fixture.registry.clone();
            Ok(())
        })
        .expect("initial model configuration");
    assert_eq!(service.snapshot().expect("current models").revision, 2);

    let mut valid = fixture.registry.clone();
    valid.models.get_mut(&fixture.model_id).expect("model").display_name =
        "Renamed Externally".to_owned();
    fs::write(
        paths.config_path(ConfigModule::Models),
        encode_document(ConfigModule::Models, &document(&valid, 3)).expect("encode"),
    )
    .expect("external valid edit");
    assert!(settings.reload(ConfigModule::Models).expect("accept valid edit"));
    assert_eq!(
        service
            .snapshot()
            .expect("refreshed")
            .registry
            .models
            .get(&fixture.model_id)
            .expect("model")
            .display_name,
        "Renamed Externally"
    );

    let mut invalid = valid;
    invalid.provider_order.clear();
    fs::write(
        paths.config_path(ConfigModule::Models),
        encode_document(ConfigModule::Models, &document(&invalid, 4)).expect("encode"),
    )
    .expect("external invalid edit");
    assert!(settings.reload(ConfigModule::Models).is_err());
    let active = service.snapshot().expect("last known good remains active");
    assert_eq!(active.revision, 3);
    assert_eq!(active.source, ConfigSource::File);
}

#[test]
fn execution_snapshots_are_immutable_across_config_renames() {
    let directory = tempfile::tempdir().expect("temp root");
    let settings = facade(paths(&directory));
    let service = ModelConfigService::new(settings);
    let fixture = Fixture::new();
    service
        .update(1, |candidate| {
            *candidate = fixture.registry.clone();
            Ok(())
        })
        .expect("initial model configuration");
    let context_id = ContextProjectionSnapshotId::new();
    let prompt_id = PromptBundleSnapshotId::new();
    let before = service
        .create_execution_snapshot(step_of(fixture.model_id, context_id, prompt_id))
        .expect("first execution snapshot");

    service
        .update(2, |candidate| {
            candidate.providers.get_mut(&fixture.provider_id).expect("provider").display_name =
                "Renamed Provider".to_owned();
            candidate.endpoints.get_mut(&fixture.endpoint_id).expect("endpoint").display_name =
                "Renamed Endpoint".to_owned();
            candidate.models.get_mut(&fixture.model_id).expect("model").provider_model_name =
                "Renamed-Model".to_owned();
            Ok(())
        })
        .expect("rename configuration");
    let after = service
        .create_execution_snapshot(step_of(fixture.model_id, context_id, prompt_id))
        .expect("second execution snapshot");

    assert_eq!(before.provider_display_name, "Example Provider");
    assert_eq!(before.endpoint_display_name, "Primary Endpoint");
    assert_eq!(before.model_name, "Example-MixedCase");
    assert_eq!(before.model_config_revision, 2);
    assert_eq!(after.provider_display_name, "Renamed Provider");
    assert_eq!(after.endpoint_display_name, "Renamed Endpoint");
    assert_eq!(after.model_name, "Renamed-Model");
    assert_eq!(after.model_config_revision, 3);
    assert_ne!(before.id, after.id);
}

#[test]
fn identical_provider_model_names_remain_scoped_to_their_own_endpoints() {
    let directory = tempfile::tempdir().expect("temp root");
    let service = ModelConfigService::new(facade(paths(&directory)));
    let mut fixture = Fixture::new();
    let second_endpoint_id = EndpointId::new();
    let second_model_id = ModelProfileId::new();

    let mut second_endpoint =
        fixture.registry.endpoints.get(&fixture.endpoint_id).expect("primary endpoint").clone();
    second_endpoint.id = second_endpoint_id;
    second_endpoint.display_name = "Chat Endpoint".to_owned();
    second_endpoint.slug = "chat".parse().expect("valid endpoint slug");
    second_endpoint.api_format = ModelApiFormat::OpenAiChatCompletions;
    second_endpoint.base_url = "https://chat.example.test/v2".to_owned();
    second_endpoint.chat_output_token_field = Some(ChatOutputTokenField::MaxTokens);
    second_endpoint.chat_instruction_role = Some(ChatInstructionRole::System);
    second_endpoint.model_order = vec![second_model_id];

    let mut second_model =
        fixture.registry.models.get(&fixture.model_id).expect("primary model").clone();
    second_model.id = second_model_id;
    second_model.endpoint_id = second_endpoint_id;
    fixture
        .registry
        .providers
        .get_mut(&fixture.provider_id)
        .expect("provider")
        .endpoint_order
        .push(second_endpoint_id);
    fixture.registry.endpoints.insert(second_endpoint_id, second_endpoint);
    fixture.registry.models.insert(second_model_id, second_model);

    validate_model_registry(&fixture.registry).expect("same name on separate endpoints is valid");
    service
        .update(1, |candidate| {
            *candidate = fixture.registry.clone();
            Ok(())
        })
        .expect("seed two endpoints");

    let snapshot = |model_profile_id| {
        service
            .create_execution_snapshot(step_of(
                model_profile_id,
                ContextProjectionSnapshotId::new(),
                PromptBundleSnapshotId::new(),
            ))
            .expect("execution snapshot")
    };
    let primary = snapshot(fixture.model_id);
    let chat = snapshot(second_model_id);

    assert_eq!(primary.model_name, chat.model_name);
    assert_eq!(primary.endpoint_id, fixture.endpoint_id);
    assert_eq!(primary.api_format, ModelApiFormat::OpenAiResponses);
    assert_eq!(primary.base_url, "https://api.example.test/v1/");
    assert_eq!(chat.endpoint_id, second_endpoint_id);
    assert_eq!(chat.api_format, ModelApiFormat::OpenAiChatCompletions);
    assert_eq!(chat.base_url, "https://chat.example.test/v2/");
    assert_eq!(chat.chat_output_token_field, Some(ChatOutputTokenField::MaxTokens));
}

#[test]
fn discovery_reuses_exact_names_creates_defaults_and_never_deletes_missing_profiles() {
    let directory = tempfile::tempdir().expect("temp root");
    let settings = facade(paths(&directory));
    let service = ModelConfigService::new(settings);
    let fixture = Fixture::new();
    service
        .update(1, |candidate| {
            *candidate = fixture.registry.clone();
            candidate
                .models
                .get_mut(&fixture.model_id)
                .expect("existing model")
                .available_for_subagents = true;
            Ok(())
        })
        .expect("seed registry");

    let catalog = catalog_snapshot();
    let names = vec!["Example-MixedCase".to_owned(), "new-model".to_owned()];
    let (first, result) = service
        .apply_discovery(2, fixture.endpoint_id, &names, Some(&catalog), UtcTimestamp::now())
        .expect("merge discovery");
    assert_eq!(result.returned_profiles.len(), 2);
    assert_eq!(result.returned_profiles[0].model_profile_id, fixture.model_id);
    let created =
        result.returned_profiles.iter().find(|profile| profile.created).expect("new profile");
    assert_eq!(created.catalog_match_state, CatalogMatchState::Matched);
    assert!(!created.needs_manual_confirmation);
    let created_model =
        first.registry.models.get(&created.model_profile_id).expect("created model");
    assert_eq!(created_model.context.runtime_effective_window_tokens, 128_000);
    assert!(!created_model.available_for_subagents);

    let only_new = vec!["new-model".to_owned()];
    let (second, result) = service
        .apply_discovery(3, fixture.endpoint_id, &only_new, Some(&catalog), UtcTimestamp::now())
        .expect("second discovery");
    assert_eq!(result.not_returned_profile_ids, vec![fixture.model_id]);
    assert!(second.registry.models.contains_key(&fixture.model_id));
    assert!(second.registry.models[&fixture.model_id].enabled);
}

#[test]
fn provider_subagent_batch_updates_only_current_models_and_is_not_inherited() {
    let directory = tempfile::tempdir().expect("temp root");
    let service = ModelConfigService::new(facade(paths(&directory)));
    let fixture = Fixture::new();
    service
        .update(1, |candidate| {
            *candidate = fixture.registry.clone();
            Ok(())
        })
        .expect("seed registry");

    let (after_batch, changed) = service
        .set_provider_current_models_subagent_availability(
            2,
            fixture.provider_id,
            true,
            UtcTimestamp::now(),
        )
        .expect("enable current provider models");
    assert_eq!(changed, vec![fixture.model_id]);
    assert!(after_batch.registry.models[&fixture.model_id].available_for_subagents);

    let (after_discovery, result) = service
        .apply_discovery(
            3,
            fixture.endpoint_id,
            &["new-model".to_owned()],
            Some(&catalog_snapshot()),
            UtcTimestamp::now(),
        )
        .expect("discover a later model");
    let created =
        result.returned_profiles.iter().find(|profile| profile.created).expect("new model");
    assert!(after_discovery.registry.models[&fixture.model_id].available_for_subagents);
    assert!(
        !after_discovery.registry.models[&created.model_profile_id].available_for_subagents,
        "provider batch operations must not become an inheritance rule"
    );
}

#[test]
fn profiles_without_output_limits_cannot_be_default_or_subagent_models() {
    let directory = tempfile::tempdir().expect("temp root");
    let service = ModelConfigService::new(facade(paths(&directory)));
    let mut fixture = Fixture::new();
    fixture
        .registry
        .models
        .get_mut(&fixture.model_id)
        .expect("model")
        .request_defaults
        .max_output_tokens = None;
    service
        .update(1, |candidate| {
            *candidate = fixture.registry.clone();
            Ok(())
        })
        .expect("seed registry");

    assert!(matches!(
        service.set_model_subagent_availability(
            2,
            fixture.model_id,
            true,
            UtcTimestamp::now(),
        ),
        Err(ModelConfigError::OutputLimitConfigurationRequired(id)) if id == fixture.model_id
    ));
    assert!(matches!(
        service.set_provider_current_models_subagent_availability(
            2,
            fixture.provider_id,
            true,
            UtcTimestamp::now(),
        ),
        Err(ModelConfigError::OutputLimitConfigurationRequired(id)) if id == fixture.model_id
    ));
    assert!(matches!(
        service.set_default_model(2, 1, Some(fixture.model_id)),
        Err(ModelConfigError::OutputLimitConfigurationRequired(id)) if id == fixture.model_id
    ));

    let unchanged = service.snapshot().expect("unchanged config after rejected operations");
    assert_eq!(unchanged.revision, 2);
    assert!(!unchanged.registry.models[&fixture.model_id].available_for_subagents);
}

#[test]
fn endpoint_fingerprint_changes_clear_compatibility_but_display_name_changes_do_not() {
    let directory = tempfile::tempdir().expect("temp root");
    let service = ModelConfigService::new(facade(paths(&directory)));
    let mut fixture = Fixture::new();
    let endpoint = fixture.registry.endpoints.get_mut(&fixture.endpoint_id).expect("endpoint");
    endpoint.compatibility = Some(EndpointCompatibilityOverrides {
        endpoint_fingerprint: endpoint_fingerprint(endpoint).expect("endpoint fingerprint"),
        unsupported_parameters: BTreeSet::new(),
        unsupported_features: BTreeSet::from(["streaming_unsupported".to_owned()]),
        output_token_field_aliases: Vec::new(),
        learned_at: Some(UtcTimestamp::now()),
    });
    service
        .update(1, |candidate| {
            *candidate = fixture.registry.clone();
            Ok(())
        })
        .expect("seed compatibility");

    let endpoint = &fixture.registry.endpoints[&fixture.endpoint_id];
    let renamed = service
        .update_endpoint(
            2,
            UpdateModelEndpointInput {
                endpoint_id: fixture.endpoint_id,
                display_name: "Renamed Endpoint".to_owned(),
                api_format: endpoint.api_format,
                base_url: endpoint.base_url.clone(),
                discovery_mode: endpoint.discovery_mode.clone(),
                chat_output_token_field: endpoint.chat_output_token_field,
                chat_instruction_role: endpoint.chat_instruction_role,
                auth_scheme: endpoint.auth_scheme,
                openai_organization_id: endpoint.openai_organization_id.clone(),
                openai_project_id: endpoint.openai_project_id.clone(),
                anthropic_workspace_id: endpoint.anthropic_workspace_id.clone(),
                timeouts: endpoint.timeouts.clone(),
                max_parallel_model_requests: endpoint.max_parallel_model_requests,
            },
            UtcTimestamp::now(),
        )
        .expect("rename endpoint");
    assert!(renamed.registry.endpoints[&fixture.endpoint_id].compatibility.is_some());

    let changed = service
        .update_endpoint(
            3,
            UpdateModelEndpointInput {
                endpoint_id: fixture.endpoint_id,
                display_name: "Renamed Endpoint".to_owned(),
                api_format: endpoint.api_format,
                base_url: "https://api.example.test/v2".to_owned(),
                discovery_mode: endpoint.discovery_mode.clone(),
                chat_output_token_field: endpoint.chat_output_token_field,
                chat_instruction_role: endpoint.chat_instruction_role,
                auth_scheme: endpoint.auth_scheme,
                openai_organization_id: endpoint.openai_organization_id.clone(),
                openai_project_id: endpoint.openai_project_id.clone(),
                anthropic_workspace_id: endpoint.anthropic_workspace_id.clone(),
                timeouts: endpoint.timeouts.clone(),
                max_parallel_model_requests: endpoint.max_parallel_model_requests,
            },
            UtcTimestamp::now(),
        )
        .expect("change endpoint fingerprint");
    assert!(changed.registry.endpoints[&fixture.endpoint_id].compatibility.is_none());
}

#[test]
fn v1_rejects_persisted_compatibility_parameter_removals() {
    let mut fixture = Fixture::new();
    let endpoint = fixture.registry.endpoints.get_mut(&fixture.endpoint_id).expect("endpoint");
    endpoint.compatibility = Some(EndpointCompatibilityOverrides {
        endpoint_fingerprint: endpoint_fingerprint(endpoint).expect("endpoint fingerprint"),
        unsupported_parameters: BTreeSet::from(["stream_options".to_owned()]),
        unsupported_features: BTreeSet::new(),
        output_token_field_aliases: Vec::new(),
        learned_at: Some(UtcTimestamp::now()),
    });

    assert!(matches!(
        aura_model_config::validate_model_registry(&fixture.registry),
        Err(aura_model_config::ModelConfigValidationError::CompatibilityRemovalNotAllowed {
            endpoint_id
        }) if endpoint_id == fixture.endpoint_id
    ));
}

#[test]
fn endpoint_auth_routing_and_credential_binding_follow_the_frozen_contract() {
    let fixture = Fixture::new();
    let mut endpoint = fixture.registry.endpoints[&fixture.endpoint_id].clone();
    endpoint.auth_scheme = EndpointAuthScheme::OpenAiBearerApiKey;
    endpoint.openai_organization_id = Some("org_contract".to_owned());

    let namespace: BundleNamespace =
        "aura.desktop.test.model-config".parse().expect("bundle namespace");
    let credential_id = CredentialId::new();
    let credential_ref = credential_ref_for_credential(&namespace, credential_id)
        .expect("deterministic credential target");
    assert_eq!(
        credential_ref.as_str(),
        format!("wincred://aura/{}/credential/{credential_id}", namespace.object_name_hash())
    );
    assert!(
        serde_json::to_value(&endpoint).expect("endpoint wire").get("credential_ref").is_none(),
        "CredentialRef belongs to P02 binding metadata, not models.json"
    );

    let binding =
        credential_binding_fingerprint(&namespace, credential_id, ModelCredentialSlot::ApiKey)
            .expect("binding fingerprint");
    let full = endpoint_fingerprint(&endpoint).expect("endpoint fingerprint");

    let mut api_format_only = endpoint.clone();
    api_format_only.api_format = ModelApiFormat::OpenAiChatCompletions;
    assert_ne!(endpoint_fingerprint(&api_format_only).expect("format endpoint fingerprint"), full);

    let mut base_path_only = endpoint.clone();
    base_path_only.base_url = "https://api.example.test/v2".to_owned();
    assert_ne!(endpoint_fingerprint(&base_path_only).expect("path endpoint fingerprint"), full);

    let mut routing_only = endpoint.clone();
    routing_only.openai_project_id = Some("project_contract".to_owned());
    assert_ne!(endpoint_fingerprint(&routing_only).expect("routing endpoint fingerprint"), full);

    let mut changed_origin = endpoint.clone();
    changed_origin.base_url = "https://other.example.test/v1".to_owned();
    assert_ne!(endpoint_fingerprint(&changed_origin).expect("origin endpoint fingerprint"), full);

    let mut changed_auth = endpoint.clone();
    changed_auth.api_format = ModelApiFormat::AnthropicMessages;
    changed_auth.auth_scheme = EndpointAuthScheme::AnthropicApiKey;
    changed_auth.openai_organization_id = None;
    changed_auth.anthropic_workspace_id = Some("workspace_contract".to_owned());
    assert_ne!(endpoint_fingerprint(&changed_auth).expect("auth endpoint fingerprint"), full);
    assert_eq!(
        credential_binding_fingerprint(&namespace, credential_id, ModelCredentialSlot::ApiKey)
            .expect("stable storage binding"),
        binding
    );

    let mut registry = fixture.registry.clone();
    registry.endpoints.insert(endpoint.id, endpoint.clone());
    validate_model_registry(&registry).expect("valid OpenAI auth/routing combination");

    registry.endpoints.get_mut(&endpoint.id).expect("endpoint").auth_scheme =
        EndpointAuthScheme::AnthropicApiKey;
    assert!(matches!(
        validate_model_registry(&registry),
        Err(ModelConfigValidationError::InvalidAuthScheme { .. })
    ));

    let invalid = registry.endpoints.get_mut(&endpoint.id).expect("endpoint");
    invalid.auth_scheme = EndpointAuthScheme::OpenAiBearerApiKey;
    invalid.anthropic_workspace_id = Some("workspace_contract".to_owned());
    assert!(matches!(
        validate_model_registry(&registry),
        Err(ModelConfigValidationError::InvalidRoutingField {
            field: "anthropic_workspace_id",
            ..
        })
    ));

    let invalid = registry.endpoints.get_mut(&endpoint.id).expect("endpoint");
    invalid.anthropic_workspace_id = None;
    invalid.auth_scheme = EndpointAuthScheme::None;
    validate_model_registry(&registry).expect("secretless endpoint config is valid");
}

#[test]
fn chat_endpoint_validation_identifies_each_required_field() {
    let fixture = Fixture::new();
    let mut registry = fixture.registry.clone();
    let endpoint = registry.endpoints.get_mut(&fixture.endpoint_id).expect("endpoint");
    endpoint.api_format = ModelApiFormat::OpenAiChatCompletions;
    endpoint.chat_output_token_field = None;
    endpoint.chat_instruction_role = None;
    assert!(matches!(
        validate_model_registry(&registry),
        Err(ModelConfigValidationError::MissingChatOutputTokenField { .. })
    ));

    let endpoint = registry.endpoints.get_mut(&fixture.endpoint_id).expect("endpoint");
    endpoint.chat_output_token_field = Some(ChatOutputTokenField::MaxCompletionTokens);
    assert!(matches!(
        validate_model_registry(&registry),
        Err(ModelConfigValidationError::MissingChatInstructionRole { .. })
    ));

    let endpoint = registry.endpoints.get_mut(&fixture.endpoint_id).expect("endpoint");
    endpoint.chat_instruction_role = Some(ChatInstructionRole::Developer);
    validate_model_registry(&registry).expect("complete Chat endpoint");

    let endpoint = registry.endpoints.get_mut(&fixture.endpoint_id).expect("endpoint");
    endpoint.api_format = ModelApiFormat::OpenAiResponses;
    assert!(matches!(
        validate_model_registry(&registry),
        Err(ModelConfigValidationError::InvalidRoutingField {
            field: "chat_output_token_field",
            ..
        })
    ));
}

#[test]
fn manual_models_use_256k_without_a_window_and_conflict_only_on_exact_endpoint_name() {
    let directory = tempfile::tempdir().expect("temp root");
    let service = ModelConfigService::new(facade(paths(&directory)));
    let fixture = Fixture::new();
    service
        .update(1, |candidate| {
            *candidate = fixture.registry.clone();
            Ok(())
        })
        .expect("seed registry");
    let (snapshot, created) = service
        .create_manual_model(
            2,
            AddManualModelInput {
                endpoint_id: fixture.endpoint_id,
                provider_model_name: "manual-model".to_owned(),
                display_name: None,
                notes: None,
                context_window_tokens: None,
            },
            UtcTimestamp::now(),
        )
        .expect("manual model");
    assert!(created.needs_manual_confirmation);
    let model = &snapshot.registry.models[&created.model_profile_id];
    assert_eq!(model.context.runtime_effective_window_tokens, 256_000);
    assert!(!model.available_for_subagents);
    assert!(
        service
            .create_manual_model(
                3,
                AddManualModelInput {
                    endpoint_id: fixture.endpoint_id,
                    provider_model_name: "manual-model".to_owned(),
                    display_name: None,
                    notes: None,
                    context_window_tokens: Some(64_000),
                },
                UtcTimestamp::now(),
            )
            .is_err()
    );
}

#[test]
fn deleting_the_default_provider_cascades_registry_only_and_preserves_other_aura_fields() {
    let directory = tempfile::tempdir().expect("temp root");
    let settings = facade(paths(&directory));
    let service = ModelConfigService::new(Arc::clone(&settings));
    let fixture = Fixture::new();
    service
        .update(1, |candidate| {
            *candidate = fixture.registry.clone();
            Ok(())
        })
        .expect("seed registry");
    settings
        .update(ConfigModule::Aura, 1, |values| {
            values.insert("default_permission_mode".to_owned(), serde_json::json!("read_only"));
        })
        .expect("seed unrelated aura field");
    let selected =
        service.set_default_model(2, 2, Some(fixture.model_id)).expect("set default model");
    assert_eq!(selected.model_profile_id, Some(fixture.model_id));
    assert_eq!(service.snapshot().expect("models unchanged").revision, 2);

    let (models, deletion) =
        service.delete_provider(2, 3, fixture.provider_id).expect("delete default provider");
    assert!(deletion.default_model_cleared);
    assert_eq!(deletion.deleted.deleted_provider_ids, vec![fixture.provider_id]);
    assert_eq!(deletion.deleted.deleted_endpoint_ids, vec![fixture.endpoint_id]);
    assert_eq!(deletion.deleted.deleted_model_profile_ids, vec![fixture.model_id]);
    assert!(models.registry.providers.is_empty());
    assert!(models.registry.endpoints.is_empty());
    assert!(models.registry.models.is_empty());
    let aura = settings.snapshot(ConfigModule::Aura).expect("aura after delete");
    assert!(!aura.document.values.contains_key("default_model_profile_id"));
    assert_eq!(
        aura.document.values.get("default_permission_mode"),
        Some(&serde_json::json!("read_only"))
    );
}

#[test]
fn context_corrections_follow_frozen_precedence_and_new_user_input_invalidates_runtime_basis() {
    let directory = tempfile::tempdir().expect("temp root");
    let service = ModelConfigService::new(facade(paths(&directory)));
    let fixture = Fixture::new();
    service
        .update(1, |candidate| {
            *candidate = fixture.registry.clone();
            Ok(())
        })
        .expect("seed registry");
    let (_, user) = service
        .set_user_context_window(2, fixture.model_id, Some(128_000), UtcTimestamp::now())
        .expect("user correction");
    assert_eq!(user.user_override_revision, 1);
    assert_eq!(user.runtime_effective_window_tokens, 128_000);
    let before_runtime_correction = service
        .create_execution_snapshot(step_of(
            fixture.model_id,
            ContextProjectionSnapshotId::new(),
            PromptBundleSnapshotId::new(),
        ))
        .expect("snapshot before runtime correction");

    let (_, runtime) = service
        .apply_runtime_context_window_correction(
            3,
            fixture.model_id,
            64_000,
            aura_domain_common::ArtifactId::new(),
            UtcTimestamp::now(),
        )
        .expect("runtime correction");
    assert_eq!(runtime.runtime_effective_window_tokens, 64_000);
    assert!(!runtime.experience_warning);
    let after_runtime_correction = service
        .create_execution_snapshot(step_of(
            fixture.model_id,
            ContextProjectionSnapshotId::new(),
            PromptBundleSnapshotId::new(),
        ))
        .expect("new snapshot after runtime correction");
    assert_eq!(before_runtime_correction.runtime_effective_window_tokens, 128_000);
    assert_eq!(after_runtime_correction.runtime_effective_window_tokens, 64_000);
    assert_ne!(before_runtime_correction.id, after_runtime_correction.id);

    let (snapshot, user_again) = service
        .set_user_context_window(4, fixture.model_id, Some(32_000), UtcTimestamp::now())
        .expect("new user correction");
    assert_eq!(user_again.user_override_revision, 2);
    assert_eq!(user_again.runtime_effective_window_tokens, 32_000);
    assert!(user_again.experience_warning);
    let model = &snapshot.registry.models[&fixture.model_id];
    assert!(model.context.runtime_corrected_window_tokens.is_none());
    assert!(model.context.runtime_correction_basis.is_none());
    assert!(
        service
            .apply_runtime_context_window_correction(
                5,
                fixture.model_id,
                16_000,
                aura_domain_common::ArtifactId::new(),
                UtcTimestamp::now(),
            )
            .is_err(),
        "the frozen automatic sequence stops at 32K"
    );
}

#[test]
fn successful_candidate_chain_skips_no_candidates_and_never_publishes_failed_intermediates() {
    let mut fixture = Fixture::new();
    aura_model_config::set_user_context_window(
        &mut fixture.registry,
        fixture.model_id,
        Some(256000),
        UtcTimestamp::now(),
    )
    .expect("initial window");
    let before = fixture.registry.clone();
    assert!(
        aura_model_config::apply_runtime_context_window_candidates(
            &mut fixture.registry,
            fixture.model_id,
            &[64000],
            aura_domain_common::ArtifactId::new(),
            UtcTimestamp::now()
        )
        .is_err()
    );
    assert_eq!(fixture.registry, before, "a skipped candidate changes no configuration");
    assert!(
        aura_model_config::apply_runtime_context_window_candidates(
            &mut fixture.registry,
            fixture.model_id,
            &[128000, 128000],
            aura_domain_common::ArtifactId::new(),
            UtcTimestamp::now()
        )
        .is_err()
    );
    assert_eq!(fixture.registry, before, "a duplicate candidate changes no configuration");
    let result = aura_model_config::apply_runtime_context_window_candidates(
        &mut fixture.registry,
        fixture.model_id,
        &[128000, 64000],
        aura_domain_common::ArtifactId::new(),
        UtcTimestamp::now(),
    )
    .expect("second candidate actually succeeded");
    assert_eq!(result.runtime_effective_window_tokens, 64000);
    assert_eq!(
        fixture.registry.models[&fixture.model_id].context.runtime_corrected_window_tokens,
        Some(64000)
    );
    assert_eq!(
        fixture.registry.models[&fixture.model_id].context.user_override_revision,
        before.models[&fixture.model_id].context.user_override_revision
    );
}

#[test]
fn endpoint_and_profile_revisions_change_once_for_semantic_mutations_and_not_for_noops() {
    let directory = tempfile::tempdir().expect("temp root");
    let service = ModelConfigService::new(facade(paths(&directory)));
    let fixture = Fixture::new();
    let seeded = service
        .update(1, |candidate| {
            *candidate = fixture.registry.clone();
            Ok(())
        })
        .expect("seed registry");
    assert_eq!(seeded.revision, 2);
    assert_eq!(seeded.registry.endpoints[&fixture.endpoint_id].revision, Revision::INITIAL);
    assert_eq!(seeded.registry.models[&fixture.model_id].revision, Revision::INITIAL);

    let endpoint = &seeded.registry.endpoints[&fixture.endpoint_id];
    let update = UpdateModelEndpointInput {
        endpoint_id: fixture.endpoint_id,
        display_name: "Renamed endpoint".to_owned(),
        api_format: endpoint.api_format,
        base_url: endpoint.base_url.clone(),
        discovery_mode: endpoint.discovery_mode.clone(),
        chat_output_token_field: endpoint.chat_output_token_field,
        chat_instruction_role: endpoint.chat_instruction_role,
        auth_scheme: endpoint.auth_scheme,
        openai_organization_id: endpoint.openai_organization_id.clone(),
        openai_project_id: endpoint.openai_project_id.clone(),
        anthropic_workspace_id: endpoint.anthropic_workspace_id.clone(),
        timeouts: endpoint.timeouts.clone(),
        max_parallel_model_requests: endpoint.max_parallel_model_requests,
    };
    let renamed = service
        .update_endpoint(2, update.clone(), UtcTimestamp::now())
        .expect("semantic endpoint mutation");
    assert_eq!(renamed.revision, 3);
    assert_eq!(renamed.registry.endpoints[&fixture.endpoint_id].revision, Revision::new(2));
    assert_eq!(renamed.registry.models[&fixture.model_id].revision, Revision::INITIAL);

    let noop = service
        .update_endpoint(3, update, UtcTimestamp::now())
        .expect("identical endpoint update is a no-op");
    assert_eq!(noop.revision, 3);
    assert_eq!(noop.registry.endpoints[&fixture.endpoint_id].revision, Revision::new(2));

    let profile = service
        .rename_model_profile(3, fixture.model_id, "Renamed model".to_owned(), UtcTimestamp::now())
        .expect("semantic profile mutation");
    assert_eq!(profile.revision, 4);
    assert_eq!(profile.registry.endpoints[&fixture.endpoint_id].revision, Revision::new(2));
    assert_eq!(profile.registry.models[&fixture.model_id].revision, Revision::new(2));

    let (created, manual) = service
        .create_manual_model(
            4,
            AddManualModelInput {
                endpoint_id: fixture.endpoint_id,
                provider_model_name: "manual-revision-model".to_owned(),
                display_name: None,
                notes: None,
                context_window_tokens: Some(32_000),
            },
            UtcTimestamp::now(),
        )
        .expect("add manual model");
    assert_eq!(created.revision, 5);
    assert_eq!(created.registry.endpoints[&fixture.endpoint_id].revision, Revision::new(3));
    assert_eq!(created.registry.models[&manual.model_profile_id].revision, Revision::INITIAL);
}

fn catalog_snapshot() -> ProviderCatalogSnapshot {
    let model = ProviderCatalogModelEntry {
        catalog_model_id: "new-model".to_owned(),
        display_name: "New Model".to_owned(),
        family: None,
        description: None,
        aliases: std::collections::BTreeSet::new(),
        context_window_tokens: Some(128_000),
        input_limit_tokens: None,
        output_limit_tokens: Some(8_192),
        input_modalities: BTreeSet::from(["text".to_owned()]),
        output_modalities: BTreeSet::from(["text".to_owned()]),
        capabilities: ProviderCatalogModelCapabilities {
            attachment: false,
            reasoning: false,
            tool_call: true,
            structured_output: false,
            temperature: true,
            interleaved: None,
        },
        reasoning_options: BTreeSet::new(),
        knowledge_cutoff: None,
        release_date: None,
        last_updated: None,
        open_weights: None,
        status: None,
        price: None,
    };
    let provider = ProviderCatalogEntry {
        catalog_provider_id: "example".to_owned(),
        display_name: "Example Provider".to_owned(),
        npm_package: None,
        api_url: None,
        documentation_url: None,
        model_order: vec!["new-model".to_owned()],
        models: BTreeMap::from([("new-model".to_owned(), model)]),
    };
    ProviderCatalogSnapshot {
        catalog_revision: Revision::INITIAL,
        catalog_content_hash: aura_domain_common::ContentHash::blake3(b"catalog"),
        checked_at: UtcTimestamp::now(),
        last_successful_refresh_at: UtcTimestamp::now(),
        etag: None,
        last_modified: None,
        raw_artifact_id: aura_domain_common::ArtifactId::new(),
        provider_order: vec!["example".to_owned()],
        providers: BTreeMap::from([("example".to_owned(), provider)]),
    }
}
