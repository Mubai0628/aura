//! Test fixture shared by the model config journal and the model credential tests: the complete
//! Projection plan a credential commit publishes for one endpoint.
use aura_domain_common::{EndpointId, ProviderId, Revision, UtcTimestamp};
use aura_model_domain::{
    EndpointAuthScheme, EndpointAuthenticationKind, EndpointAuthenticationProjection,
    EndpointCredentialStatus, EndpointDetailProjection, ModelApiFormat, ModelDiscoveryMode,
    ModelEndpointConfig, ModelEndpointTimeouts, ModelProviderConfig, ModelProviderSource,
    ModelRegistryConfig, ModelSettingsProjection, ProviderCatalogProjection, ProviderCatalogState,
};

use crate::ModelProjectionDraft;

pub(crate) fn credential_projection_plan(
    endpoint_id: EndpointId,
    models_revision: Revision,
    status: EndpointCredentialStatus,
) -> Vec<ModelProjectionDraft> {
    let provider_id = ProviderId::new();
    let now = UtcTimestamp::now();
    vec![
        ModelProjectionDraft::ModelSettings(ModelSettingsProjection {
            models_revision,
            aura_revision: Revision::INITIAL,
            registry: ModelRegistryConfig::default(),
            default_model_profile_id: None,
            active_credentials: vec![status.clone()],
            dormant_credentials: Vec::new(),
            reasoning_efforts: Default::default(),
        }),
        ModelProjectionDraft::EndpointDetail(Box::new(EndpointDetailProjection {
            endpoint_id,
            models_revision,
            provider: ModelProviderConfig {
                id: provider_id,
                display_name: "Provider".to_owned(),
                slug: "provider".parse().expect("provider slug"),
                source: ModelProviderSource::Custom { catalog_provider_id: None },
                endpoint_order: vec![endpoint_id],
                created_at: now,
                updated_at: now,
                default_endpoint_id: None,
                backup_endpoint_ids: vec![],
            },
            endpoint: ModelEndpointConfig {
                id: endpoint_id,
                revision: Revision::INITIAL,
                provider_id,
                display_name: "Endpoint".to_owned(),
                slug: "endpoint".parse().expect("endpoint slug"),
                api_format: ModelApiFormat::OpenAiResponses,
                base_url: "https://api.example.test/v1".to_owned(),
                discovery_mode: ModelDiscoveryMode::Disabled,
                chat_output_token_field: None,
                chat_instruction_role: None,
                auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
                openai_organization_id: None,
                openai_project_id: None,
                anthropic_workspace_id: None,
                timeouts: ModelEndpointTimeouts {
                    connect_ms: 15_000,
                    first_event_ms: 120_000,
                    stream_idle_ms: 120_000,
                    total_ms: None,
                },
                max_parallel_model_requests: 2,
                model_order: Vec::new(),
                compatibility: None,
                created_at: now,
                updated_at: now,
            },
            authentication: EndpointAuthenticationProjection {
                auth_kind: EndpointAuthenticationKind::Secret,
                active_credential: Some(status),
                dormant_credential: None,
            },
            models: Vec::new(),
            latest_discovery_run: None,
        })),
        // Credential commits publish a complete P05 projection plan. Unrelated
        // drafts must remain valid without pretending to contain this slot.
        ModelProjectionDraft::ProviderCatalog(ProviderCatalogProjection {
            state: ProviderCatalogState::Unavailable,
            catalog_revision: None,
            catalog_content_hash: None,
            checked_at: None,
            last_successful_refresh_at: None,
            cards: Vec::new(),
            last_error_code: None,
            projection_revision: Revision::INITIAL,
        }),
    ]
}
