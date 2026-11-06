//! Servers on this computer.
use aura_model_domain::{
    ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat, ModelDiscoveryMode,
};

use super::shape::{alternatives, chat, openai_prompt_cache, recommended};
use crate::{HostPattern, ProviderProfile, ProviderRoute, RequestRule, RequestTarget, RouteChoice};

/// A gateway on this computer (CLIProxyAPI and the like) in front of OpenAI's Responses API is
/// answered by OpenAI's cache: through one without the key, a turn's steps were cached
/// erratically. A Chat Completions endpoint there is not sent the key: other servers on this
/// computer may not know the field, and a field a gateway does not know can fail every request.
pub(super) fn local_responses_gateway() -> ProviderProfile {
    ProviderProfile {
        profile_id: "local-responses-gateway".to_owned(),
        catalog_provider_ids: Vec::new(),
        route_choice: RouteChoice::OneOf,
        routes: Vec::new(),
        requests: vec![RequestRule {
            target: RequestTarget {
                scheme: None,
                host: HostPattern::Loopback,
                api_formats: vec![ModelApiFormat::OpenAiResponses],
            },
            rules: openai_prompt_cache(),
        }],
    }
}

/// LM Studio's server at its default port, which takes no key unless it is set to. It is reached
/// by Chat Completions: a Responses endpoint on this computer is sent OpenAI's cache key, which
/// LM Studio need not know.
pub(super) fn lm_studio() -> ProviderProfile {
    alternatives(
        "lmstudio",
        &["lmstudio"],
        vec![recommended(ProviderRoute {
            auth_scheme: EndpointAuthScheme::None,
            ..chat(
                "LM Studio",
                "http://127.0.0.1:1234/v1",
                ModelDiscoveryMode::Automatic,
                ChatOutputTokenField::MaxTokens,
            )
        })],
    )
}
