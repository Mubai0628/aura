//! The built-in routes are the eleven trusted presets as they were: the same catalog Providers,
//! addresses, protocols, keys, discovery and Chat defaults, in the same order, six recommended.
//! Their routes are alternatives. OpenCode's two plans have one route per protocol at the
//! addresses OpenCode documents, and each of their models goes by its own, as each of Bedrock's
//! does (its routes are in `address_templates.rs`).
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat,
    ModelDiscoveryMode,
};
use aura_provider_profile::{
    CatalogRoute, ProviderRoute, RouteChoice, has_builtin_routes, profile_for_catalog_provider,
    routes_for_catalog_provider,
};

const CATALOG_PROVIDERS: [&str; 6] =
    ["openai", "anthropic", "deepseek", "moonshotai", "kimi-code-plan-cn", "kimi-code-plan-global"];

macro_rules! route {
    ($id:literal, $name:literal, $base:literal, $format:ident, $auth:ident, $discovery:ident, $role:expr, $token:expr, $recommended:literal) => {
        ProviderRoute {
            route_id: $id.to_owned(),
            display_name: $name.to_owned(),
            base_url: $base.to_owned(),
            api_format: ModelApiFormat::$format,
            auth_scheme: EndpointAuthScheme::$auth,
            discovery_mode: ModelDiscoveryMode::$discovery,
            chat_instruction_role: $role,
            chat_output_token_field: $token,
            recommended: $recommended,
            catalog_routes: Vec::new(),
            parameters: Vec::new(),
        }
    };
}

fn expected() -> Vec<(&'static str, ProviderRoute)> {
    use ChatInstructionRole::{Developer, System};
    use ChatOutputTokenField::{MaxCompletionTokens, MaxTokens};
    vec![
        (
            "openai",
            route!(
                "responses",
                "OpenAI Responses",
                "https://api.openai.com/v1",
                OpenAiResponses,
                OpenAiBearerApiKey,
                Automatic,
                None,
                None,
                true
            ),
        ),
        (
            "openai",
            route!(
                "chat-completions",
                "OpenAI Chat Completions",
                "https://api.openai.com/v1",
                OpenAiChatCompletions,
                OpenAiBearerApiKey,
                Automatic,
                Some(Developer),
                Some(MaxCompletionTokens),
                false
            ),
        ),
        (
            "anthropic",
            route!(
                "messages",
                "Anthropic Messages",
                "https://api.anthropic.com/v1",
                AnthropicMessages,
                AnthropicApiKey,
                Automatic,
                None,
                None,
                true
            ),
        ),
        (
            "deepseek",
            route!(
                "responses",
                "DeepSeek Responses",
                "https://api.deepseek.com",
                OpenAiResponses,
                OpenAiBearerApiKey,
                Automatic,
                None,
                None,
                true
            ),
        ),
        (
            "deepseek",
            route!(
                "chat-completions",
                "DeepSeek Chat Completions",
                "https://api.deepseek.com",
                OpenAiChatCompletions,
                OpenAiBearerApiKey,
                Automatic,
                Some(System),
                Some(MaxTokens),
                false
            ),
        ),
        (
            "deepseek",
            route!(
                "anthropic-messages",
                "DeepSeek Anthropic Messages",
                "https://api.deepseek.com/anthropic/v1",
                AnthropicMessages,
                AnthropicApiKey,
                Disabled,
                None,
                None,
                false
            ),
        ),
        (
            "moonshotai",
            route!(
                "chat-completions",
                "Moonshot AI Chat Completions",
                "https://api.moonshot.ai/v1",
                OpenAiChatCompletions,
                OpenAiBearerApiKey,
                Automatic,
                Some(System),
                Some(MaxCompletionTokens),
                true
            ),
        ),
        (
            "kimi-code-plan-cn",
            route!(
                "anthropic-messages",
                "Kimi for Coding Anthropic Messages",
                "https://api.kimi.com/coding/v1",
                AnthropicMessages,
                AnthropicApiKey,
                Disabled,
                None,
                None,
                true
            ),
        ),
        (
            "kimi-code-plan-cn",
            route!(
                "chat-completions",
                "Kimi for Coding Chat Completions",
                "https://api.kimi.com/coding/v1",
                OpenAiChatCompletions,
                OpenAiBearerApiKey,
                Disabled,
                Some(System),
                Some(MaxCompletionTokens),
                false
            ),
        ),
        (
            "kimi-code-plan-global",
            route!(
                "anthropic-messages",
                "Kimi for Coding Anthropic Messages",
                "https://api.kimi.ai/coding/v1",
                AnthropicMessages,
                AnthropicApiKey,
                Disabled,
                None,
                None,
                true
            ),
        ),
        (
            "kimi-code-plan-global",
            route!(
                "chat-completions",
                "Kimi for Coding Chat Completions",
                "https://api.kimi.ai/coding/v1",
                OpenAiChatCompletions,
                OpenAiBearerApiKey,
                Disabled,
                Some(System),
                Some(MaxCompletionTokens),
                false
            ),
        ),
    ]
}

#[test]
fn the_built_in_routes_are_the_eleven_presets_as_they_were() {
    let routes: Vec<(&str, ProviderRoute)> = CATALOG_PROVIDERS
        .iter()
        .flat_map(|id| routes_for_catalog_provider(id).into_iter().map(move |route| (*id, route)))
        .collect();
    assert_eq!(routes, expected());
    assert_eq!(routes.iter().filter(|(_, route)| route.recommended).count(), 6);
}

#[test]
fn only_those_catalog_providers_have_built_in_routes() {
    for id in CATALOG_PROVIDERS.into_iter().chain(OPENCODE).chain(["amazon-bedrock"]) {
        assert!(has_builtin_routes(id), "{id}");
    }
    for id in [
        "kimi-for-coding",
        "opencode-zen",
        "amazon-bedrock-mantle",
        "alibaba-token-plan",
        "ollama-cloud",
        "",
    ] {
        assert!(!has_builtin_routes(id), "{id}");
        assert!(routes_for_catalog_provider(id).is_empty(), "{id}");
    }
}

const OPENCODE: [&str; 2] = ["opencode", "opencode-go"];

/// An OpenCode route at `base`: the catalog's own entry for it names the same address.
fn opencode_route(
    id: &str,
    name: &str,
    base: &str,
    api_format: ModelApiFormat,
    npm: &str,
) -> ProviderRoute {
    let chat = api_format == ModelApiFormat::OpenAiChatCompletions;
    ProviderRoute {
        route_id: id.to_owned(),
        display_name: name.to_owned(),
        base_url: base.to_owned(),
        api_format,
        auth_scheme: if api_format == ModelApiFormat::AnthropicMessages {
            EndpointAuthScheme::AnthropicApiKey
        } else {
            EndpointAuthScheme::OpenAiBearerApiKey
        },
        discovery_mode: ModelDiscoveryMode::Disabled,
        chat_instruction_role: chat.then_some(ChatInstructionRole::System),
        chat_output_token_field: chat.then_some(ChatOutputTokenField::MaxTokens),
        recommended: false,
        catalog_routes: vec![CatalogRoute {
            npm_package: npm.to_owned(),
            api_url: base.to_owned(),
            shape: None,
        }],
        parameters: Vec::new(),
    }
}

fn opencode_routes(plan: &str, inference: &str) -> Vec<ProviderRoute> {
    let openai = format!("{inference}/openai/v1");
    let anthropic = format!("{inference}/anthropic/v1");
    vec![
        opencode_route(
            "chat-completions",
            &format!("{plan} Chat Completions"),
            &openai,
            ModelApiFormat::OpenAiChatCompletions,
            "@ai-sdk/openai-compatible",
        ),
        opencode_route(
            "responses",
            &format!("{plan} Responses"),
            &openai,
            ModelApiFormat::OpenAiResponses,
            "@ai-sdk/openai",
        ),
        opencode_route(
            "anthropic-messages",
            &format!("{plan} Anthropic Messages"),
            &anthropic,
            ModelApiFormat::AnthropicMessages,
            "@ai-sdk/anthropic",
        ),
    ]
}

#[test]
fn opencodes_routes_are_one_per_protocol_at_its_documented_addresses() {
    assert_eq!(
        routes_for_catalog_provider("opencode"),
        opencode_routes("OpenCode", "https://opencode.ai/inference")
    );
    assert_eq!(
        routes_for_catalog_provider("opencode-go"),
        opencode_routes("OpenCode Go", "https://opencode.ai/inference/go")
    );
}

#[test]
fn only_opencodes_and_bedrocks_models_go_by_their_own_routes() {
    for id in OPENCODE.into_iter().chain(["amazon-bedrock"]) {
        let profile = profile_for_catalog_provider(id).expect("a profile");
        assert_eq!(profile.route_choice, RouteChoice::ByModel, "{id}");
    }
    for id in CATALOG_PROVIDERS {
        let profile = profile_for_catalog_provider(id).expect("a profile");
        assert_eq!(profile.route_choice, RouteChoice::OneOf, "{id}");
    }
    assert!(profile_for_catalog_provider("ollama-cloud").is_none());
}
