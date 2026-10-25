//! Which of a provider's connections a model's request goes through, in the order
//! they are tried. The default the user chose, else the first created; then every other connection, in the
//! order they were created. A connection that speaks another API format cannot carry the model's
//! request and is skipped.
use aura_domain_common::EndpointId;
use aura_model_domain::{
    ModelEndpointConfig, ModelProfileConfig, ModelProviderConfig, ModelRegistryConfig,
};

use crate::ModelConfigError;

pub fn connection_order(
    registry: &ModelRegistryConfig,
    profile: &ModelProfileConfig,
) -> Vec<EndpointId> {
    let Some(own) = registry.endpoints.get(&profile.endpoint_id) else {
        return vec![profile.endpoint_id];
    };
    let Some(provider) = registry.providers.get(&own.provider_id) else {
        return vec![own.id];
    };
    let speaks = |id: &EndpointId| {
        registry.endpoints.get(id).is_some_and(|endpoint| {
            endpoint.provider_id == own.provider_id && endpoint.api_format == own.api_format
        })
    };
    let mut order: Vec<EndpointId> =
        provider.default_endpoint_id.filter(speaks).into_iter().collect();
    for id in provider.endpoint_order.iter().chain([&own.id]) {
        if speaks(id) && !order.contains(id) {
            order.push(*id);
        }
    }
    order
}

/// The connection to use now: the first in order that has not failed in this run's current streak.
pub fn next_connection(order: &[EndpointId], failed: &[EndpointId]) -> Option<EndpointId> {
    order.iter().copied().find(|id| !failed.contains(id))
}

/// The connection a step is sent through, with its provider: the profile's own, or `routed` when
/// that is another connection of the same provider speaking the same API format.
pub(crate) fn step_connection<'a>(
    registry: &'a ModelRegistryConfig,
    model: &ModelProfileConfig,
    routed: Option<EndpointId>,
) -> Result<(&'a ModelEndpointConfig, &'a ModelProviderConfig), ModelConfigError> {
    let own = registry
        .endpoints
        .get(&model.endpoint_id)
        .ok_or(ModelConfigError::EndpointNotFound(model.id))?;
    let endpoint = match routed {
        Some(id) if id != model.endpoint_id => {
            let routed =
                registry.endpoints.get(&id).ok_or(ModelConfigError::EndpointIdNotFound(id))?;
            if routed.provider_id != own.provider_id || routed.api_format != own.api_format {
                return Err(ModelConfigError::InvalidProviderConnections(own.provider_id));
            }
            routed
        }
        _ => own,
    };
    let provider = registry
        .providers
        .get(&endpoint.provider_id)
        .ok_or(ModelConfigError::ProviderNotFound(model.id))?;
    Ok((endpoint, provider))
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::{ModelProfileId, ProviderId, Revision, UtcTimestamp};
    use aura_model_domain::*;

    /// One provider with four connections, in the order they were created: OpenAI Responses, Chat
    /// Completions, then two more OpenAI Responses; the model is on connection `model_on`.
    fn fixture(
        default: Option<usize>,
        backups: &[usize],
        model_on: usize,
    ) -> (ModelRegistryConfig, ModelProfileConfig, Vec<EndpointId>) {
        let provider_id = ProviderId::new();
        let ids = vec![EndpointId::new(), EndpointId::new(), EndpointId::new(), EndpointId::new()];
        let model_id = ModelProfileId::new();
        let now = UtcTimestamp::now();
        let endpoint = |index: usize, format: ModelApiFormat| ModelEndpointConfig {
            id: ids[index],
            revision: Revision::INITIAL,
            provider_id,
            display_name: format!("Connection {index}"),
            slug: format!("connection-{index}").parse().expect("slug"),
            api_format: format,
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
            max_parallel_model_requests: 1,
            model_order: if index == model_on { vec![model_id] } else { vec![] },
            compatibility: None,
            created_at: now,
            updated_at: now,
        };
        let model = ModelProfileConfig {
            id: model_id,
            revision: Revision::INITIAL,
            endpoint_id: ids[model_on],
            provider_model_name: "m".to_owned(),
            display_name: "M".to_owned(),
            notes: None,
            origin: ModelProfileOrigin::Manual,
            enabled: true,
            available_for_subagents: false,
            catalog: None,
            context: ModelContextWindowConfig {
                catalog_window_tokens: None,
                user_override_window_tokens: None,
                user_override_revision: 0,
                runtime_corrected_window_tokens: None,
                runtime_correction_basis: None,
                runtime_effective_window_tokens: 1_000,
                historical_snapshot_max_tokens: 100,
            },
            request_defaults: ModelRequestDefaults::default(),
            created_at: now,
            updated_at: now,
        };
        let provider = ModelProviderConfig {
            id: provider_id,
            display_name: "P".to_owned(),
            slug: "p".parse().expect("slug"),
            source: ModelProviderSource::ModelsDev { catalog_provider_id: "p".to_owned() },
            endpoint_order: ids.clone(),
            created_at: now,
            updated_at: now,
            default_endpoint_id: default.map(|index| ids[index]),
            backup_endpoint_ids: backups.iter().map(|index| ids[*index]).collect(),
        };
        let registry = ModelRegistryConfig {
            provider_order: vec![provider_id],
            providers: [(provider_id, provider)].into(),
            endpoints: [
                (ids[0], endpoint(0, ModelApiFormat::OpenAiResponses)),
                (ids[1], endpoint(1, ModelApiFormat::OpenAiChatCompletions)),
                (ids[2], endpoint(2, ModelApiFormat::OpenAiResponses)),
                (ids[3], endpoint(3, ModelApiFormat::OpenAiResponses)),
            ]
            .into(),
            models: [(model_id, model.clone())].into(),
        };
        (registry, model, ids)
    }

    #[test]
    fn without_a_choice_the_first_created_is_the_default_and_every_other_a_backup() {
        let (registry, profile, ids) = fixture(None, &[], 3);
        // The Chat connection cannot carry a Responses model's request.
        assert_eq!(connection_order(&registry, &profile), vec![ids[0], ids[2], ids[3]]);
        // A list of backups saved by an older version no longer decides anything.
        let (registry, profile, ids) = fixture(None, &[3], 0);
        assert_eq!(connection_order(&registry, &profile), vec![ids[0], ids[2], ids[3]]);
    }

    #[test]
    fn the_chosen_default_goes_first_then_the_others_in_the_order_they_were_created() {
        let (registry, profile, ids) = fixture(Some(3), &[], 0);
        assert_eq!(connection_order(&registry, &profile), vec![ids[3], ids[0], ids[2]]);
        // A default of another format cannot carry the request: the first of its own goes first.
        let (registry, profile, ids) = fixture(Some(1), &[], 2);
        assert_eq!(connection_order(&registry, &profile), vec![ids[0], ids[2], ids[3]]);
    }

    #[test]
    fn when_the_default_is_deleted_the_next_created_takes_its_place() {
        let (mut registry, profile, ids) = fixture(Some(0), &[], 3);
        crate::crud::delete_endpoint(&mut registry, ids[0]).expect("deleted");
        assert_eq!(connection_order(&registry, &profile), vec![ids[2], ids[3]]);
        crate::crud::delete_endpoint(&mut registry, ids[2]).expect("deleted");
        assert_eq!(connection_order(&registry, &profile), vec![ids[3]]);
    }

    #[test]
    fn the_next_connection_skips_those_that_failed_in_this_streak() {
        let (registry, profile, ids) = fixture(Some(2), &[], 0);
        let order = connection_order(&registry, &profile);
        assert_eq!(next_connection(&order, &[]), Some(ids[2]));
        assert_eq!(next_connection(&order, &[ids[2]]), Some(ids[0]));
        assert_eq!(next_connection(&order, &[ids[2], ids[0], ids[3]]), None);
    }
}
