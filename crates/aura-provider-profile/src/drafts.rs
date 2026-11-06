//! What a connection to a catalog Provider without a built-in profile is drafted from beside its
//! address: the catalog's package for the Provider, which names the protocol it speaks.
use aura_model_domain::{
    ChatInstructionRole, ChatOutputTokenField, EndpointAuthScheme, ModelApiFormat,
    ModelsDevProtocolSuggestion,
};

/// The protocol the catalog package `npm` speaks, the key it sends and, for Chat Completions,
/// the defaults it sends with; `None` for a package that speaks none Aura speaks. The package
/// sends `max_tokens`, and a system message as such.
#[must_use]
pub fn catalog_package_protocol(npm: &str) -> Option<ModelsDevProtocolSuggestion> {
    let (api_format, auth_scheme, chat) = match npm {
        "@ai-sdk/openai-compatible" => (
            ModelApiFormat::OpenAiChatCompletions,
            EndpointAuthScheme::OpenAiBearerApiKey,
            Some((ChatInstructionRole::System, ChatOutputTokenField::MaxTokens)),
        ),
        "@ai-sdk/openai" => {
            (ModelApiFormat::OpenAiResponses, EndpointAuthScheme::OpenAiBearerApiKey, None)
        }
        "@ai-sdk/anthropic" => {
            (ModelApiFormat::AnthropicMessages, EndpointAuthScheme::AnthropicApiKey, None)
        }
        _ => return None,
    };
    Some(ModelsDevProtocolSuggestion {
        api_format,
        auth_scheme,
        chat_instruction_role: chat.map(|(role, _)| role),
        chat_output_token_field: chat.map(|(_, field)| field),
    })
}
