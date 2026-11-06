//! The model makers' own APIs, and Kimi For Coding, the coding plan Moonshot AI sells.
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, ModelApiFormat, ModelDiscoveryMode,
};

use super::shape::{
    alternatives, chat, conversation_header, https_host, openai_prompt_cache, profile, recommended,
    responses, route,
};
use crate::{HostPattern, ProviderProfile, RequestRule, RequestRules, RequestTarget, RoutingKey};

/// OpenAI keeps a Responses request and its reply for 30 days unless asked not to; Aura sends
/// the whole conversation each step, so nothing is kept, and a reasoning model's reasoning comes
/// back encrypted to go back with the next step.
pub(super) fn openai() -> ProviderProfile {
    profile(
        "openai",
        "openai",
        vec![
            route!(
                "responses",
                "OpenAI Responses",
                "https://api.openai.com/v1",
                OpenAiResponses,
                OpenAiBearerApiKey,
                Automatic,
                None,
                None,
                true,
            ),
            route!(
                "chat-completions",
                "OpenAI Chat Completions",
                "https://api.openai.com/v1",
                OpenAiChatCompletions,
                OpenAiBearerApiKey,
                Automatic,
                Some(ChatInstructionRole::Developer),
                Some(ChatOutputTokenField::MaxCompletionTokens),
                false,
            ),
        ],
        vec![RequestRule {
            target: RequestTarget {
                scheme: Some("https".to_owned()),
                host: HostPattern::Exact("api.openai.com".to_owned()),
                api_formats: Vec::new(),
            },
            rules: RequestRules {
                store_off: true,
                encrypted_reasoning: true,
                ..openai_prompt_cache()
            },
        }],
    )
}

/// Anthropic's own endpoint takes explicit, default-TTL cache breakpoints; compatible Messages
/// endpoints may reject Anthropic's cache extensions.
pub(super) fn anthropic() -> ProviderProfile {
    profile(
        "anthropic",
        "anthropic",
        vec![route!(
            "messages",
            "Anthropic Messages",
            "https://api.anthropic.com/v1",
            AnthropicMessages,
            AnthropicApiKey,
            Automatic,
            None,
            None,
            true,
        )],
        vec![RequestRule {
            target: RequestTarget {
                scheme: Some("https".to_owned()),
                host: HostPattern::Exact("api.anthropic.com".to_owned()),
                api_formats: vec![ModelApiFormat::AnthropicMessages],
            },
            rules: RequestRules { cache_breakpoints: true, ..RequestRules::default() },
        }],
    )
}

/// DeepSeek's cache keeps nearly every step with the task's state after the conversation and no
/// key, and its endpoints may not know OpenAI's fields, so its requests carry nothing more.
pub(super) fn deepseek() -> ProviderProfile {
    profile(
        "deepseek",
        "deepseek",
        vec![
            route!(
                "responses",
                "DeepSeek Responses",
                "https://api.deepseek.com",
                OpenAiResponses,
                OpenAiBearerApiKey,
                Automatic,
                None,
                None,
                true,
            ),
            route!(
                "chat-completions",
                "DeepSeek Chat Completions",
                "https://api.deepseek.com",
                OpenAiChatCompletions,
                OpenAiBearerApiKey,
                Automatic,
                Some(ChatInstructionRole::System),
                Some(ChatOutputTokenField::MaxTokens),
                false,
            ),
            route!(
                "anthropic-messages",
                "DeepSeek Anthropic Messages",
                "https://api.deepseek.com/anthropic/v1",
                AnthropicMessages,
                AnthropicApiKey,
                Disabled,
                None,
                None,
                false,
            ),
        ],
        Vec::new(),
    )
}

pub(super) fn moonshot() -> ProviderProfile {
    profile(
        "moonshot",
        "moonshotai",
        vec![route!(
            "chat-completions",
            "Moonshot AI Chat Completions",
            "https://api.moonshot.ai/v1",
            OpenAiChatCompletions,
            OpenAiBearerApiKey,
            Automatic,
            Some(ChatInstructionRole::System),
            Some(ChatOutputTokenField::MaxCompletionTokens),
            true,
        )],
        Vec::new(),
    )
}

/// Moonshot AI's China site, reached as its overseas one is.
pub(super) fn moonshot_cn() -> ProviderProfile {
    alternatives(
        "moonshot-cn",
        &["moonshotai-cn"],
        vec![recommended(chat(
            "Moonshot AI (China)",
            "https://api.moonshot.cn/v1",
            ModelDiscoveryMode::Automatic,
            ChatOutputTokenField::MaxCompletionTokens,
        ))],
    )
}

/// Kimi For Coding: the China site (kimi.com) and the overseas one (kimi.ai), as Kimi's own
/// documentation gives their addresses.
pub(super) fn kimi_for_coding(
    profile_id: &str,
    catalog_provider_id: &str,
    base_url: &str,
) -> ProviderProfile {
    profile(
        profile_id,
        catalog_provider_id,
        vec![
            route!(
                "anthropic-messages",
                "Kimi for Coding Anthropic Messages",
                base_url,
                AnthropicMessages,
                AnthropicApiKey,
                Disabled,
                None,
                None,
                true,
            ),
            route!(
                "chat-completions",
                "Kimi for Coding Chat Completions",
                base_url,
                OpenAiChatCompletions,
                OpenAiBearerApiKey,
                Disabled,
                Some(ChatInstructionRole::System),
                Some(ChatOutputTokenField::MaxCompletionTokens),
                false,
            ),
        ],
        Vec::new(),
    )
}

/// Google's OpenAI-compatible endpoint for the Gemini API. Discovery is off: its documentation
/// does not give the form its models list names the models in, so the catalog gives them.
pub(super) fn google() -> ProviderProfile {
    alternatives(
        "google",
        &["google"],
        vec![recommended(chat(
            "Google Gemini",
            "https://generativelanguage.googleapis.com/v1beta/openai",
            ModelDiscoveryMode::Disabled,
            ChatOutputTokenField::MaxTokens,
        ))],
    )
}

/// xAI recommends its Responses API over Chat Completions; its Messages endpoint is withdrawn.
/// It sends a conversation's requests to the same cache by `prompt_cache_key` in a Responses
/// body and by the `x-grok-conv-id` header with Chat Completions. Discovery is off: its API
/// reference gives the models' names in its console and documentation.
pub(super) fn xai() -> ProviderProfile {
    let base_url = "https://api.x.ai/v1";
    ProviderProfile {
        requests: vec![
            https_host(
                "api.x.ai",
                vec![ModelApiFormat::OpenAiResponses],
                RequestRules {
                    routing_key: Some(RoutingKey::PromptPrefixField {
                        name: "prompt_cache_key".to_owned(),
                    }),
                    ..RequestRules::default()
                },
            ),
            https_host(
                "api.x.ai",
                vec![ModelApiFormat::OpenAiChatCompletions],
                conversation_header("x-grok-conv-id"),
            ),
        ],
        ..alternatives(
            "xai",
            &["xai"],
            vec![
                recommended(responses("xAI", base_url, ModelDiscoveryMode::Disabled)),
                chat(
                    "xAI",
                    base_url,
                    ModelDiscoveryMode::Disabled,
                    ChatOutputTokenField::MaxTokens,
                ),
            ],
        )
    }
}

pub(super) fn mistral() -> ProviderProfile {
    alternatives(
        "mistral",
        &["mistral"],
        vec![recommended(chat(
            "Mistral",
            "https://api.mistral.ai/v1",
            ModelDiscoveryMode::Automatic,
            ChatOutputTokenField::MaxTokens,
        ))],
    )
}
