//! Services that run other makers' models: Amazon Bedrock, Groq, Fireworks AI and Together AI.
use aura_model_domain::{
    ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat, ModelDiscoveryMode,
    ProviderPresetParameterKind,
};

use super::shape::{alternatives, chat, recommended, responses};
use crate::{
    CatalogRoute, HostPattern, ProviderProfile, ProviderRoute, RequestRule, RequestRules,
    RequestTarget, RouteChoice, RouteParameter,
};

/// Amazon Bedrock's Mantle endpoint speaks OpenAI's Responses API in each region it runs in and
/// takes a Bedrock API key as a bearer token. Each of its models is served under one of two path
/// prefixes, and a request under the other one is refused. Its model list names the models of
/// both, so models come from the catalog by prefix instead. Most of Bedrock's models go by its
/// own protocol, which Aura does not speak. Bedrock keeps a Responses request and its reply for
/// 30 days unless asked not to, so nothing is kept; it does not document encrypted reasoning, so
/// none is asked for. Only Bedrock speaks Responses under api.aws.
pub(super) fn bedrock() -> ProviderProfile {
    let route = |route_id: &str, prefix: &str| ProviderRoute {
        route_id: route_id.to_owned(),
        display_name: format!("Amazon Bedrock Mantle {prefix}"),
        base_url: format!("https://bedrock-mantle.{{region}}.api.aws{prefix}"),
        api_format: ModelApiFormat::OpenAiResponses,
        auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
        discovery_mode: ModelDiscoveryMode::Disabled,
        chat_instruction_role: None,
        chat_output_token_field: None,
        recommended: false,
        catalog_routes: vec![CatalogRoute {
            npm_package: "@ai-sdk/amazon-bedrock/mantle".to_owned(),
            api_url: format!("https://bedrock-mantle.${{AWS_REGION}}.api.aws{prefix}"),
            shape: Some("responses".to_owned()),
        }],
        parameters: vec![RouteParameter {
            name: "region".to_owned(),
            display_name: "区域".to_owned(),
            kind: ProviderPresetParameterKind::DomainLabel,
            example: "us-east-1".to_owned(),
        }],
    };
    ProviderProfile {
        profile_id: "amazon-bedrock".to_owned(),
        catalog_provider_ids: vec!["amazon-bedrock".to_owned()],
        route_choice: RouteChoice::ByModel,
        routes: vec![route("mantle-v1", "/v1"), route("mantle-openai-v1", "/openai/v1")],
        requests: vec![RequestRule {
            target: RequestTarget {
                scheme: Some("https".to_owned()),
                host: HostPattern::Domain("api.aws".to_owned()),
                api_formats: vec![ModelApiFormat::OpenAiResponses],
            },
            rules: RequestRules { store_off: true, ..RequestRules::default() },
        }],
    }
}

/// Groq's Chat Completions takes `max_completion_tokens`, `max_tokens` being deprecated there;
/// it serves the same models by Responses.
pub(super) fn groq() -> ProviderProfile {
    let base_url = "https://api.groq.com/openai/v1";
    alternatives(
        "groq",
        &["groq"],
        vec![
            recommended(chat(
                "Groq",
                base_url,
                ModelDiscoveryMode::Automatic,
                ChatOutputTokenField::MaxCompletionTokens,
            )),
            responses("Groq", base_url, ModelDiscoveryMode::Automatic),
        ],
    )
}

/// Discovery is off: Fireworks AI's models list is its account's, at no address of this route.
pub(super) fn fireworks() -> ProviderProfile {
    alternatives(
        "fireworks-ai",
        &["fireworks-ai"],
        vec![recommended(chat(
            "Fireworks AI",
            "https://api.fireworks.ai/inference/v1",
            ModelDiscoveryMode::Disabled,
            ChatOutputTokenField::MaxTokens,
        ))],
    )
}

pub(super) fn together() -> ProviderProfile {
    alternatives(
        "togetherai",
        &["togetherai"],
        vec![recommended(chat(
            "Together AI",
            "https://api.together.ai/v1",
            ModelDiscoveryMode::Automatic,
            ChatOutputTokenField::MaxTokens,
        ))],
    )
}
