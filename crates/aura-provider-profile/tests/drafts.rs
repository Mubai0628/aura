//! A connection to a catalog Provider without a built-in profile is drafted from the catalog's
//! package for it, as the package itself reaches the address: Chat Completions with a bearer
//! key, the instructions in the system role and the limit in `max_tokens`
//! (`@ai-sdk/openai-compatible`); Responses with a bearer key (`@ai-sdk/openai`); Anthropic
//! Messages with the key in `x-api-key` (`@ai-sdk/anthropic`). Any other package drafts nothing,
//! however near its name.
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat,
    ModelsDevProtocolSuggestion,
};
use aura_provider_profile::catalog_package_protocol;

#[test]
fn the_three_packages_that_speak_auras_protocols_draft_them() {
    assert_eq!(
        catalog_package_protocol("@ai-sdk/openai-compatible"),
        Some(ModelsDevProtocolSuggestion {
            api_format: ModelApiFormat::OpenAiChatCompletions,
            auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
            chat_instruction_role: Some(ChatInstructionRole::System),
            chat_output_token_field: Some(ChatOutputTokenField::MaxTokens),
        })
    );
    assert_eq!(
        catalog_package_protocol("@ai-sdk/openai"),
        Some(ModelsDevProtocolSuggestion {
            api_format: ModelApiFormat::OpenAiResponses,
            auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
            chat_instruction_role: None,
            chat_output_token_field: None,
        })
    );
    assert_eq!(
        catalog_package_protocol("@ai-sdk/anthropic"),
        Some(ModelsDevProtocolSuggestion {
            api_format: ModelApiFormat::AnthropicMessages,
            auth_scheme: EndpointAuthScheme::AnthropicApiKey,
            chat_instruction_role: None,
            chat_output_token_field: None,
        })
    );
}

#[test]
fn any_other_package_drafts_nothing() {
    for npm in [
        "@openrouter/ai-sdk-provider",
        "@ai-sdk/groq",
        "@ai-sdk/google",
        "@ai-sdk/amazon-bedrock/mantle",
        "@ai-sdk/google-vertex/anthropic",
        "@ai-sdk/openai-compatible/extra",
        "@AI-SDK/OPENAI",
        " @ai-sdk/anthropic",
        "merge-gateway-ai-sdk-provider",
        "",
    ] {
        assert_eq!(catalog_package_protocol(npm), None, "{npm}");
    }
}
