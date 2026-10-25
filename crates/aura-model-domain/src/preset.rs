use crate::{
    ChatInstructionRole, ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat,
    ModelDiscoveryMode,
};

/// Aura-owned execution configuration for one trusted Provider variant.
///
/// These records are compiled into the signed Aura component. Remote catalog
/// data can describe Providers and Models but cannot change these fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProviderPresetV1 {
    pub catalog_provider_id: &'static str,
    pub variant_id: &'static str,
    pub display_name: &'static str,
    pub base_url: &'static str,
    pub api_format: ModelApiFormat,
    pub auth_scheme: EndpointAuthScheme,
    pub discovery_mode: ModelDiscoveryMode,
    pub chat_instruction_role: Option<ChatInstructionRole>,
    pub chat_output_token_field: Option<ChatOutputTokenField>,
    pub recommended: bool,
}

pub const PROVIDER_PRESET_REGISTRY_VERSION: u32 = 1;

#[must_use]
pub fn provider_preset_registry_v1() -> Vec<ProviderPresetV1> {
    macro_rules! preset {
        ($provider:literal, $variant:literal, $name:literal, $base:literal, $format:expr, $auth:expr, $discovery:expr, $role:expr, $token:expr, $recommended:literal $(,)?) => {
            ProviderPresetV1 {
                catalog_provider_id: $provider,
                variant_id: $variant,
                display_name: $name,
                base_url: $base,
                api_format: $format,
                auth_scheme: $auth,
                discovery_mode: $discovery,
                chat_instruction_role: $role,
                chat_output_token_field: $token,
                recommended: $recommended,
            }
        };
    }
    vec![
        preset!(
            "openai",
            "responses",
            "OpenAI Responses",
            "https://api.openai.com/v1",
            ModelApiFormat::OpenAiResponses,
            EndpointAuthScheme::OpenAiBearerApiKey,
            ModelDiscoveryMode::Automatic,
            None,
            None,
            true,
        ),
        preset!(
            "openai",
            "chat-completions",
            "OpenAI Chat Completions",
            "https://api.openai.com/v1",
            ModelApiFormat::OpenAiChatCompletions,
            EndpointAuthScheme::OpenAiBearerApiKey,
            ModelDiscoveryMode::Automatic,
            Some(ChatInstructionRole::Developer),
            Some(ChatOutputTokenField::MaxCompletionTokens),
            false,
        ),
        preset!(
            "anthropic",
            "messages",
            "Anthropic Messages",
            "https://api.anthropic.com/v1",
            ModelApiFormat::AnthropicMessages,
            EndpointAuthScheme::AnthropicApiKey,
            ModelDiscoveryMode::Automatic,
            None,
            None,
            true,
        ),
        preset!(
            "deepseek",
            "responses",
            "DeepSeek Responses",
            "https://api.deepseek.com",
            ModelApiFormat::OpenAiResponses,
            EndpointAuthScheme::OpenAiBearerApiKey,
            ModelDiscoveryMode::Automatic,
            None,
            None,
            true,
        ),
        preset!(
            "deepseek",
            "chat-completions",
            "DeepSeek Chat Completions",
            "https://api.deepseek.com",
            ModelApiFormat::OpenAiChatCompletions,
            EndpointAuthScheme::OpenAiBearerApiKey,
            ModelDiscoveryMode::Automatic,
            Some(ChatInstructionRole::System),
            Some(ChatOutputTokenField::MaxTokens),
            false,
        ),
        preset!(
            "deepseek",
            "anthropic-messages",
            "DeepSeek Anthropic Messages",
            "https://api.deepseek.com/anthropic/v1",
            ModelApiFormat::AnthropicMessages,
            EndpointAuthScheme::AnthropicApiKey,
            ModelDiscoveryMode::Disabled,
            None,
            None,
            false,
        ),
        preset!(
            "moonshotai",
            "chat-completions",
            "Moonshot AI Chat Completions",
            "https://api.moonshot.ai/v1",
            ModelApiFormat::OpenAiChatCompletions,
            EndpointAuthScheme::OpenAiBearerApiKey,
            ModelDiscoveryMode::Automatic,
            Some(ChatInstructionRole::System),
            Some(ChatOutputTokenField::MaxCompletionTokens),
            true,
        ),
        preset!(
            "kimi-for-coding",
            "anthropic-messages",
            "Kimi for Coding Anthropic Messages",
            "https://api.kimi.com/coding/v1",
            ModelApiFormat::AnthropicMessages,
            EndpointAuthScheme::AnthropicApiKey,
            ModelDiscoveryMode::Disabled,
            None,
            None,
            true,
        ),
        preset!(
            "kimi-for-coding",
            "chat-completions",
            "Kimi for Coding Chat Completions",
            "https://api.kimi.com/coding/v1",
            ModelApiFormat::OpenAiChatCompletions,
            EndpointAuthScheme::OpenAiBearerApiKey,
            ModelDiscoveryMode::Disabled,
            Some(ChatInstructionRole::System),
            Some(ChatOutputTokenField::MaxCompletionTokens),
            false,
        ),
    ]
}

#[must_use]
pub fn has_trusted_provider_preset(catalog_provider_id: &str) -> bool {
    provider_preset_registry_v1()
        .iter()
        .any(|preset| preset.catalog_provider_id == catalog_provider_id)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use crate::{
        ChatInstructionRole, ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat,
        ModelDiscoveryMode,
    };

    use super::{ProviderPresetV1, provider_preset_registry_v1};

    #[test]
    fn registry_contains_the_frozen_variants_without_duplicates() {
        let registry = provider_preset_registry_v1();
        assert_eq!(registry.len(), 9);
        let keys = registry
            .iter()
            .map(|preset| (preset.catalog_provider_id, preset.variant_id))
            .collect::<BTreeSet<_>>();
        assert_eq!(keys.len(), registry.len());
        assert_eq!(registry.iter().filter(|preset| preset.recommended).count(), 5);

        let expected = [
            ProviderPresetV1 {
                catalog_provider_id: "openai",
                variant_id: "responses",
                display_name: "OpenAI Responses",
                base_url: "https://api.openai.com/v1",
                api_format: ModelApiFormat::OpenAiResponses,
                auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
                discovery_mode: ModelDiscoveryMode::Automatic,
                chat_instruction_role: None,
                chat_output_token_field: None,
                recommended: true,
            },
            ProviderPresetV1 {
                catalog_provider_id: "openai",
                variant_id: "chat-completions",
                display_name: "OpenAI Chat Completions",
                base_url: "https://api.openai.com/v1",
                api_format: ModelApiFormat::OpenAiChatCompletions,
                auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
                discovery_mode: ModelDiscoveryMode::Automatic,
                chat_instruction_role: Some(ChatInstructionRole::Developer),
                chat_output_token_field: Some(ChatOutputTokenField::MaxCompletionTokens),
                recommended: false,
            },
            ProviderPresetV1 {
                catalog_provider_id: "anthropic",
                variant_id: "messages",
                display_name: "Anthropic Messages",
                base_url: "https://api.anthropic.com/v1",
                api_format: ModelApiFormat::AnthropicMessages,
                auth_scheme: EndpointAuthScheme::AnthropicApiKey,
                discovery_mode: ModelDiscoveryMode::Automatic,
                chat_instruction_role: None,
                chat_output_token_field: None,
                recommended: true,
            },
            ProviderPresetV1 {
                catalog_provider_id: "deepseek",
                variant_id: "responses",
                display_name: "DeepSeek Responses",
                base_url: "https://api.deepseek.com",
                api_format: ModelApiFormat::OpenAiResponses,
                auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
                discovery_mode: ModelDiscoveryMode::Automatic,
                chat_instruction_role: None,
                chat_output_token_field: None,
                recommended: true,
            },
            ProviderPresetV1 {
                catalog_provider_id: "deepseek",
                variant_id: "chat-completions",
                display_name: "DeepSeek Chat Completions",
                base_url: "https://api.deepseek.com",
                api_format: ModelApiFormat::OpenAiChatCompletions,
                auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
                discovery_mode: ModelDiscoveryMode::Automatic,
                chat_instruction_role: Some(ChatInstructionRole::System),
                chat_output_token_field: Some(ChatOutputTokenField::MaxTokens),
                recommended: false,
            },
            ProviderPresetV1 {
                catalog_provider_id: "deepseek",
                variant_id: "anthropic-messages",
                display_name: "DeepSeek Anthropic Messages",
                base_url: "https://api.deepseek.com/anthropic/v1",
                api_format: ModelApiFormat::AnthropicMessages,
                auth_scheme: EndpointAuthScheme::AnthropicApiKey,
                discovery_mode: ModelDiscoveryMode::Disabled,
                chat_instruction_role: None,
                chat_output_token_field: None,
                recommended: false,
            },
            ProviderPresetV1 {
                catalog_provider_id: "moonshotai",
                variant_id: "chat-completions",
                display_name: "Moonshot AI Chat Completions",
                base_url: "https://api.moonshot.ai/v1",
                api_format: ModelApiFormat::OpenAiChatCompletions,
                auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
                discovery_mode: ModelDiscoveryMode::Automatic,
                chat_instruction_role: Some(ChatInstructionRole::System),
                chat_output_token_field: Some(ChatOutputTokenField::MaxCompletionTokens),
                recommended: true,
            },
            ProviderPresetV1 {
                catalog_provider_id: "kimi-for-coding",
                variant_id: "anthropic-messages",
                display_name: "Kimi for Coding Anthropic Messages",
                base_url: "https://api.kimi.com/coding/v1",
                api_format: ModelApiFormat::AnthropicMessages,
                auth_scheme: EndpointAuthScheme::AnthropicApiKey,
                discovery_mode: ModelDiscoveryMode::Disabled,
                chat_instruction_role: None,
                chat_output_token_field: None,
                recommended: true,
            },
            ProviderPresetV1 {
                catalog_provider_id: "kimi-for-coding",
                variant_id: "chat-completions",
                display_name: "Kimi for Coding Chat Completions",
                base_url: "https://api.kimi.com/coding/v1",
                api_format: ModelApiFormat::OpenAiChatCompletions,
                auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
                discovery_mode: ModelDiscoveryMode::Disabled,
                chat_instruction_role: Some(ChatInstructionRole::System),
                chat_output_token_field: Some(ChatOutputTokenField::MaxCompletionTokens),
                recommended: false,
            },
        ];
        assert_eq!(registry, expected);
    }
}
