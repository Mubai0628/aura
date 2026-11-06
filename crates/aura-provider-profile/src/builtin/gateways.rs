//! Gateways in front of many makers' models: OpenCode's two plans, OpenRouter and Vercel's AI
//! Gateway.
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat,
    ModelDiscoveryMode,
};

use super::shape::{
    alternatives, chat, conversation_header, https_host, messages, recommended, responses,
};
use crate::{
    CatalogRoute, HostPattern, ProviderProfile, ProviderRoute, RequestRule, RequestRules,
    RequestTarget, RouteChoice, RoutingKey,
};

/// OpenCode's two plans, pay-as-you-go under `/inference` and Go under `/inference/go`, as
/// OpenCode documents them: one route per protocol, and each model by the protocol OpenCode lists
/// it under, which the catalog gives as its package. Its Gemini models go by Google's protocol,
/// which Aura does not speak. Discovery is off: OpenCode's models list is at neither route's
/// address, and lists the models of every protocol.
pub(super) fn opencode(
    profile_id: &str,
    catalog_provider_id: &str,
    plan: &str,
    inference: &str,
) -> ProviderProfile {
    let openai = format!("{inference}/openai/v1");
    let anthropic = format!("{inference}/anthropic/v1");
    ProviderProfile {
        profile_id: profile_id.to_owned(),
        catalog_provider_ids: vec![catalog_provider_id.to_owned()],
        route_choice: RouteChoice::ByModel,
        routes: vec![
            ProviderRoute {
                chat_instruction_role: Some(ChatInstructionRole::System),
                chat_output_token_field: Some(ChatOutputTokenField::MaxTokens),
                ..by_model_route(
                    "chat-completions",
                    &format!("{plan} Chat Completions"),
                    &openai,
                    ModelApiFormat::OpenAiChatCompletions,
                    "@ai-sdk/openai-compatible",
                )
            },
            by_model_route(
                "responses",
                &format!("{plan} Responses"),
                &openai,
                ModelApiFormat::OpenAiResponses,
                "@ai-sdk/openai",
            ),
            by_model_route(
                "anthropic-messages",
                &format!("{plan} Anthropic Messages"),
                &anthropic,
                ModelApiFormat::AnthropicMessages,
                "@ai-sdk/anthropic",
            ),
        ],
        requests: opencode_requests(),
    }
}

/// A route of OpenCode's that serves the catalog package `npm` at its own address. The
/// Anthropic route takes the key in `x-api-key`, as OpenCode's own client sends it.
fn by_model_route(
    route_id: &str,
    display_name: &str,
    base_url: &str,
    api_format: ModelApiFormat,
    npm: &str,
) -> ProviderRoute {
    ProviderRoute {
        route_id: route_id.to_owned(),
        display_name: display_name.to_owned(),
        base_url: base_url.to_owned(),
        api_format,
        auth_scheme: if api_format == ModelApiFormat::AnthropicMessages {
            EndpointAuthScheme::AnthropicApiKey
        } else {
            EndpointAuthScheme::OpenAiBearerApiKey
        },
        discovery_mode: ModelDiscoveryMode::Disabled,
        chat_instruction_role: None,
        chat_output_token_field: None,
        recommended: false,
        catalog_routes: vec![CatalogRoute {
            npm_package: npm.to_owned(),
            api_url: base_url.to_owned(),
            shape: None,
        }],
        parameters: Vec::new(),
    }
}

/// OpenCode's gateway refuses a request without `x-opencode-session`, the stable id of a
/// conversation it routes and caches by, and asks each client to name itself in User-Agent
/// instead of as an HTTP library. Both plans share its host, so each profile carries the same
/// rule.
fn opencode_requests() -> Vec<RequestRule> {
    vec![RequestRule {
        target: RequestTarget {
            scheme: None,
            host: HostPattern::Domain("opencode.ai".to_owned()),
            api_formats: Vec::new(),
        },
        rules: RequestRules {
            routing_key: Some(RoutingKey::ConversationHeader {
                name: "x-opencode-session".to_owned(),
            }),
            aura_user_agent: true,
            state_before_conversation: false,
            cache_breakpoints: false,
            store_off: false,
            encrypted_reasoning: false,
        },
    }]
}

pub(super) fn openrouter() -> ProviderProfile {
    let base_url = "https://openrouter.ai/api/v1";
    alternatives(
        "openrouter",
        &["openrouter"],
        vec![
            recommended(chat(
                "OpenRouter",
                base_url,
                ModelDiscoveryMode::Automatic,
                ChatOutputTokenField::MaxTokens,
            )),
            responses("OpenRouter", base_url, ModelDiscoveryMode::Automatic),
        ],
    )
}

/// Vercel's AI Gateway serves each of its models by all three protocols, Messages at the same
/// `/v1`, and sends the requests that share an `x-session-id` header to the same place, where
/// the steps before left their cache.
pub(super) fn vercel() -> ProviderProfile {
    let provider = "Vercel AI Gateway";
    let base_url = "https://ai-gateway.vercel.sh/v1";
    ProviderProfile {
        requests: vec![https_host(
            "ai-gateway.vercel.sh",
            Vec::new(),
            conversation_header("x-session-id"),
        )],
        ..alternatives(
            "vercel",
            &["vercel"],
            vec![
                recommended(chat(
                    provider,
                    base_url,
                    ModelDiscoveryMode::Automatic,
                    ChatOutputTokenField::MaxTokens,
                )),
                responses(provider, base_url, ModelDiscoveryMode::Automatic),
                messages(provider, base_url),
            ],
        )
    }
}
