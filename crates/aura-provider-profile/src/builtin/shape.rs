//! The shapes the built-in profiles are written in.
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat,
    ModelDiscoveryMode,
};

use crate::{
    HostPattern, ProviderProfile, ProviderRoute, RequestRule, RequestRules, RequestTarget,
    RouteChoice, RoutingKey,
};

/// A route with every default written out.
macro_rules! route {
    ($id:literal, $name:literal, $base:expr, $format:ident, $auth:ident, $discovery:ident, $role:expr, $token:expr, $recommended:literal $(,)?) => {
        $crate::ProviderRoute {
            route_id: $id.to_owned(),
            display_name: $name.to_owned(),
            base_url: ($base).to_owned(),
            api_format: ::aura_model_domain::ModelApiFormat::$format,
            auth_scheme: ::aura_model_domain::EndpointAuthScheme::$auth,
            discovery_mode: ::aura_model_domain::ModelDiscoveryMode::$discovery,
            chat_instruction_role: $role,
            chat_output_token_field: $token,
            recommended: $recommended,
            catalog_routes: Vec::new(),
            parameters: Vec::new(),
        }
    };
}
pub(super) use route;

pub(super) fn profile(
    profile_id: &str,
    catalog_provider_id: &str,
    routes: Vec<ProviderRoute>,
    requests: Vec<RequestRule>,
) -> ProviderProfile {
    ProviderProfile {
        profile_id: profile_id.to_owned(),
        catalog_provider_ids: vec![catalog_provider_id.to_owned()],
        route_choice: RouteChoice::OneOf,
        routes,
        requests,
    }
}

/// A profile whose routes are alternatives and whose requests carry nothing beyond the protocol,
/// for each of `catalog_provider_ids`: a plan sold at its Provider's own addresses is reached
/// the same way as the Provider.
pub(super) fn alternatives(
    profile_id: &str,
    catalog_provider_ids: &[&str],
    routes: Vec<ProviderRoute>,
) -> ProviderProfile {
    ProviderProfile {
        profile_id: profile_id.to_owned(),
        catalog_provider_ids: catalog_provider_ids.iter().map(|id| (*id).to_owned()).collect(),
        route_choice: RouteChoice::OneOf,
        routes,
        requests: Vec::new(),
    }
}

/// `provider`'s Chat Completions route at `base_url`, which takes the key as a bearer token and
/// the instructions in the system role.
pub(super) fn chat(
    provider: &str,
    base_url: &str,
    discovery_mode: ModelDiscoveryMode,
    token_field: ChatOutputTokenField,
) -> ProviderRoute {
    ProviderRoute {
        chat_instruction_role: Some(ChatInstructionRole::System),
        chat_output_token_field: Some(token_field),
        ..plain(
            "chat-completions",
            &format!("{provider} Chat Completions"),
            base_url,
            ModelApiFormat::OpenAiChatCompletions,
            EndpointAuthScheme::OpenAiBearerApiKey,
            discovery_mode,
        )
    }
}

/// `provider`'s Responses route at `base_url`, which takes the key as a bearer token.
pub(super) fn responses(
    provider: &str,
    base_url: &str,
    discovery_mode: ModelDiscoveryMode,
) -> ProviderRoute {
    plain(
        "responses",
        &format!("{provider} Responses"),
        base_url,
        ModelApiFormat::OpenAiResponses,
        EndpointAuthScheme::OpenAiBearerApiKey,
        discovery_mode,
    )
}

/// `provider`'s Anthropic Messages route at `base_url`, where Aura adds `messages`: one less than
/// the address an Anthropic SDK is given, which adds `/v1/messages`. It takes the key in
/// `x-api-key`, and no Provider documents a models list there in Anthropic's form.
pub(super) fn messages(provider: &str, base_url: &str) -> ProviderRoute {
    plain(
        "anthropic-messages",
        &format!("{provider} Anthropic Messages"),
        base_url,
        ModelApiFormat::AnthropicMessages,
        EndpointAuthScheme::AnthropicApiKey,
        ModelDiscoveryMode::Disabled,
    )
}

/// The route a connection to the Provider starts from.
pub(super) fn recommended(route: ProviderRoute) -> ProviderRoute {
    ProviderRoute { recommended: true, ..route }
}

fn plain(
    route_id: &str,
    display_name: &str,
    base_url: &str,
    api_format: ModelApiFormat,
    auth_scheme: EndpointAuthScheme,
    discovery_mode: ModelDiscoveryMode,
) -> ProviderRoute {
    ProviderRoute {
        route_id: route_id.to_owned(),
        display_name: display_name.to_owned(),
        base_url: base_url.to_owned(),
        api_format,
        auth_scheme,
        discovery_mode,
        chat_instruction_role: None,
        chat_output_token_field: None,
        recommended: false,
        catalog_routes: Vec::new(),
        parameters: Vec::new(),
    }
}

/// OpenAI's prompt cache sends the requests that share a key to the same cache, so the steps of
/// one conversation, each the one before with more appended, find what the one before left. It
/// kept a turn's steps only as far as the instructions while a few hundred tokens or more followed
/// the conversation, and as far as the last step once nothing did; the task's state changes rarely
/// within a turn, so it goes first.
pub(super) fn openai_prompt_cache() -> RequestRules {
    RequestRules {
        routing_key: Some(RoutingKey::PromptPrefixField { name: "prompt_cache_key".to_owned() }),
        aura_user_agent: false,
        state_before_conversation: true,
        cache_breakpoints: false,
        store_off: false,
        encrypted_reasoning: false,
    }
}

/// The requests that carry nothing more than the header `name` standing for their conversation.
pub(super) fn conversation_header(name: &str) -> RequestRules {
    RequestRules {
        routing_key: Some(RoutingKey::ConversationHeader { name: name.to_owned() }),
        ..RequestRules::default()
    }
}

/// `rules` for the requests over https to `host` itself, in `api_formats` or, where it is empty,
/// in every protocol.
pub(super) fn https_host(
    host: &str,
    api_formats: Vec<ModelApiFormat>,
    rules: RequestRules,
) -> RequestRule {
    RequestRule {
        target: RequestTarget {
            scheme: Some("https".to_owned()),
            host: HostPattern::Exact(host.to_owned()),
            api_formats,
        },
        rules,
    }
}
