//! The profiles of the model makers, regional platforms, coding plans, gateways and local servers
//! beside the first presets, each route as its Provider's own documentation gives it: the
//! address, the protocol, the key, whether a models list is documented there with the catalog's
//! model names (discovery), the Chat defaults, and the route a connection starts from. Their
//! routes are alternatives, and exactly one is recommended. A region's version and a plan's
//! version are Providers of their own; a plan at its Provider's addresses shares them.
//!
//! The Anthropic Messages addresses end in `/v1`, where Aura adds `messages`: each Provider
//! documents the address an Anthropic SDK adds `/v1/messages` to.
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat,
    ModelDiscoveryMode,
};
use aura_provider_profile::{
    RouteChoice, profile_for_catalog_provider, routes_for_catalog_provider,
};

#[derive(Debug, PartialEq)]
struct Route {
    catalog: &'static str,
    id: &'static str,
    name: &'static str,
    base_url: &'static str,
    format: ModelApiFormat,
    auth: EndpointAuthScheme,
    discovery: ModelDiscoveryMode,
    role: Option<ChatInstructionRole>,
    token: Option<ChatOutputTokenField>,
    recommended: bool,
}

/// A Chat Completions route taking a Bearer key, with the system role for instructions.
const fn chat(
    catalog: &'static str,
    name: &'static str,
    base_url: &'static str,
    discovery: ModelDiscoveryMode,
    token: ChatOutputTokenField,
    recommended: bool,
) -> Route {
    Route {
        catalog,
        id: "chat-completions",
        name,
        base_url,
        format: ModelApiFormat::OpenAiChatCompletions,
        auth: EndpointAuthScheme::OpenAiBearerApiKey,
        discovery,
        role: Some(ChatInstructionRole::System),
        token: Some(token),
        recommended,
    }
}

const fn responses(
    catalog: &'static str,
    name: &'static str,
    base_url: &'static str,
    discovery: ModelDiscoveryMode,
    recommended: bool,
) -> Route {
    Route {
        catalog,
        id: "responses",
        name,
        base_url,
        format: ModelApiFormat::OpenAiResponses,
        auth: EndpointAuthScheme::OpenAiBearerApiKey,
        discovery,
        role: None,
        token: None,
        recommended,
    }
}

/// An Anthropic Messages route taking the key in `x-api-key`; none of them documents a models
/// list.
const fn messages(
    catalog: &'static str,
    name: &'static str,
    base_url: &'static str,
    recommended: bool,
) -> Route {
    Route {
        catalog,
        id: "anthropic-messages",
        name,
        base_url,
        format: ModelApiFormat::AnthropicMessages,
        auth: EndpointAuthScheme::AnthropicApiKey,
        discovery: ModelDiscoveryMode::Disabled,
        role: None,
        token: None,
        recommended,
    }
}

fn expected() -> Vec<Route> {
    use ChatOutputTokenField::{MaxCompletionTokens, MaxTokens};
    use ModelDiscoveryMode::{Automatic, Disabled};
    let minimax = |catalog| {
        [
            messages(
                catalog,
                "MiniMax Anthropic Messages",
                "https://api.minimax.io/anthropic/v1",
                true,
            ),
            chat(
                catalog,
                "MiniMax Chat Completions",
                "https://api.minimax.io/v1",
                Disabled,
                MaxTokens,
                false,
            ),
        ]
    };
    let minimax_cn = |catalog| {
        [
            messages(
                catalog,
                "MiniMax (China) Anthropic Messages",
                "https://api.minimax.cn/anthropic/v1",
                true,
            ),
            chat(
                catalog,
                "MiniMax (China) Chat Completions",
                "https://api.minimax.cn/v1",
                Disabled,
                MaxTokens,
                false,
            ),
        ]
    };
    let mut routes = vec![
        chat(
            "moonshotai-cn",
            "Moonshot AI (China) Chat Completions",
            "https://api.moonshot.cn/v1",
            Automatic,
            MaxCompletionTokens,
            true,
        ),
        chat(
            "zhipuai",
            "Zhipu AI Chat Completions",
            "https://open.bigmodel.cn/api/paas/v4",
            Disabled,
            MaxTokens,
            true,
        ),
        messages(
            "zhipuai",
            "Zhipu AI Anthropic Messages",
            "https://open.bigmodel.cn/api/anthropic/v1",
            false,
        ),
        chat(
            "zhipuai-coding-plan",
            "Zhipu AI Coding Plan Chat Completions",
            "https://open.bigmodel.cn/api/coding/paas/v4",
            Disabled,
            MaxTokens,
            true,
        ),
        messages(
            "zhipuai-coding-plan",
            "Zhipu AI Coding Plan Anthropic Messages",
            "https://open.bigmodel.cn/api/anthropic/v1",
            false,
        ),
        responses(
            "zhipuai-coding-plan",
            "Zhipu AI Coding Plan Responses",
            "https://open.bigmodel.cn/api/v1",
            Disabled,
            false,
        ),
        chat(
            "zai",
            "Z.AI Chat Completions",
            "https://api.z.ai/api/paas/v4",
            Disabled,
            MaxTokens,
            true,
        ),
        chat(
            "zai-coding-plan",
            "Z.AI Coding Plan Chat Completions",
            "https://api.z.ai/api/coding/paas/v4",
            Disabled,
            MaxTokens,
            true,
        ),
        messages(
            "zai-coding-plan",
            "Z.AI Coding Plan Anthropic Messages",
            "https://api.z.ai/api/anthropic/v1",
            false,
        ),
        responses(
            "zai-coding-plan",
            "Z.AI Coding Plan Responses",
            "https://api.z.ai/api/v1",
            Disabled,
            false,
        ),
        chat(
            "alibaba",
            "Alibaba Chat Completions",
            "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
            Disabled,
            MaxTokens,
            true,
        ),
        messages(
            "alibaba",
            "Alibaba Anthropic Messages",
            "https://dashscope-intl.aliyuncs.com/apps/anthropic/v1",
            false,
        ),
        chat(
            "alibaba-cn",
            "Alibaba (China) Chat Completions",
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            Disabled,
            MaxTokens,
            true,
        ),
        messages(
            "alibaba-cn",
            "Alibaba (China) Anthropic Messages",
            "https://dashscope.aliyuncs.com/apps/anthropic/v1",
            false,
        ),
        chat(
            "alibaba-coding-plan",
            "Alibaba Coding Plan Chat Completions",
            "https://coding-intl.dashscope.aliyuncs.com/v1",
            Disabled,
            MaxTokens,
            true,
        ),
        messages(
            "alibaba-coding-plan",
            "Alibaba Coding Plan Anthropic Messages",
            "https://coding-intl.dashscope.aliyuncs.com/apps/anthropic/v1",
            false,
        ),
        chat(
            "alibaba-coding-plan-cn",
            "Alibaba Coding Plan (China) Chat Completions",
            "https://coding.dashscope.aliyuncs.com/v1",
            Disabled,
            MaxTokens,
            true,
        ),
        messages(
            "alibaba-coding-plan-cn",
            "Alibaba Coding Plan (China) Anthropic Messages",
            "https://coding.dashscope.aliyuncs.com/apps/anthropic/v1",
            false,
        ),
    ];
    routes.extend(minimax("minimax"));
    routes.extend(minimax("minimax-coding-plan"));
    routes.extend(minimax_cn("minimax-cn"));
    routes.extend(minimax_cn("minimax-cn-coding-plan"));
    routes.extend([
        chat(
            "siliconflow",
            "SiliconFlow Chat Completions",
            "https://api.siliconflow.com/v1",
            Automatic,
            MaxTokens,
            true,
        ),
        chat(
            "siliconflow-cn",
            "SiliconFlow (China) Chat Completions",
            "https://api.siliconflow.cn/v1",
            Automatic,
            MaxTokens,
            true,
        ),
        chat(
            "volcengine",
            "Volcengine Ark Chat Completions",
            "https://ark.cn-beijing.volces.com/api/v3",
            Disabled,
            MaxTokens,
            true,
        ),
        chat(
            "volcengine-coding-plan",
            "Volcengine Ark Coding Plan Chat Completions",
            "https://ark.cn-beijing.volces.com/api/coding/v3",
            Disabled,
            MaxTokens,
            true,
        ),
        messages(
            "volcengine-coding-plan",
            "Volcengine Ark Coding Plan Anthropic Messages",
            "https://ark.cn-beijing.volces.com/api/coding/v1",
            false,
        ),
        chat(
            "google",
            "Google Gemini Chat Completions",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            Disabled,
            MaxTokens,
            true,
        ),
        responses("xai", "xAI Responses", "https://api.x.ai/v1", Disabled, true),
        chat("xai", "xAI Chat Completions", "https://api.x.ai/v1", Disabled, MaxTokens, false),
        chat(
            "mistral",
            "Mistral Chat Completions",
            "https://api.mistral.ai/v1",
            Automatic,
            MaxTokens,
            true,
        ),
        chat(
            "groq",
            "Groq Chat Completions",
            "https://api.groq.com/openai/v1",
            Automatic,
            MaxCompletionTokens,
            true,
        ),
        responses("groq", "Groq Responses", "https://api.groq.com/openai/v1", Automatic, false),
        chat(
            "fireworks-ai",
            "Fireworks AI Chat Completions",
            "https://api.fireworks.ai/inference/v1",
            Disabled,
            MaxTokens,
            true,
        ),
        chat(
            "togetherai",
            "Together AI Chat Completions",
            "https://api.together.ai/v1",
            Automatic,
            MaxTokens,
            true,
        ),
        chat(
            "openrouter",
            "OpenRouter Chat Completions",
            "https://openrouter.ai/api/v1",
            Automatic,
            MaxTokens,
            true,
        ),
        responses(
            "openrouter",
            "OpenRouter Responses",
            "https://openrouter.ai/api/v1",
            Automatic,
            false,
        ),
        chat(
            "vercel",
            "Vercel AI Gateway Chat Completions",
            "https://ai-gateway.vercel.sh/v1",
            Automatic,
            MaxTokens,
            true,
        ),
        responses(
            "vercel",
            "Vercel AI Gateway Responses",
            "https://ai-gateway.vercel.sh/v1",
            Automatic,
            false,
        ),
        messages(
            "vercel",
            "Vercel AI Gateway Anthropic Messages",
            "https://ai-gateway.vercel.sh/v1",
            false,
        ),
        Route {
            auth: EndpointAuthScheme::None,
            ..chat(
                "lmstudio",
                "LM Studio Chat Completions",
                "http://127.0.0.1:1234/v1",
                Automatic,
                MaxTokens,
                true,
            )
        },
    ]);
    routes
}

const ADDED: [&str; 26] = [
    "moonshotai-cn",
    "zhipuai",
    "zhipuai-coding-plan",
    "zai",
    "zai-coding-plan",
    "alibaba",
    "alibaba-cn",
    "alibaba-coding-plan",
    "alibaba-coding-plan-cn",
    "minimax",
    "minimax-coding-plan",
    "minimax-cn",
    "minimax-cn-coding-plan",
    "siliconflow",
    "siliconflow-cn",
    "volcengine",
    "volcengine-coding-plan",
    "google",
    "xai",
    "mistral",
    "groq",
    "fireworks-ai",
    "togetherai",
    "openrouter",
    "vercel",
    "lmstudio",
];

fn actual(catalog: &'static str) -> Vec<Route> {
    routes_for_catalog_provider(catalog)
        .into_iter()
        .map(|route| {
            assert!(route.catalog_routes.is_empty() && route.parameters.is_empty(), "{catalog}");
            Route {
                catalog,
                id: Box::leak(route.route_id.into_boxed_str()),
                name: Box::leak(route.display_name.into_boxed_str()),
                base_url: Box::leak(route.base_url.into_boxed_str()),
                format: route.api_format,
                auth: route.auth_scheme,
                discovery: route.discovery_mode,
                role: route.chat_instruction_role,
                token: route.chat_output_token_field,
                recommended: route.recommended,
            }
        })
        .collect()
}

#[test]
fn the_added_providers_routes_are_the_ones_they_document() {
    let routes: Vec<Route> = ADDED.into_iter().flat_map(actual).collect();
    assert_eq!(routes, expected());
}

#[test]
fn each_added_provider_takes_any_of_its_models_by_any_route_and_recommends_one() {
    for catalog in ADDED {
        let profile = profile_for_catalog_provider(catalog).expect("a profile");
        assert_eq!(profile.route_choice, RouteChoice::OneOf, "{catalog}");
        let recommended = actual(catalog).iter().filter(|route| route.recommended).count();
        assert_eq!(recommended, 1, "{catalog}");
    }
}
