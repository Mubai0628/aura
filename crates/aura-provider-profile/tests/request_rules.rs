//! What a request carries beyond the protocol follows the profile of its host:
//! - OpenAI's API, over https, gets the prompt cache key in the body and the task's state first,
//!   and is asked to keep nothing of a request, a reasoning model's reasoning coming back
//!   encrypted;
//! - a Responses gateway on this computer is treated as OpenAI's cache; other protocols there
//!   get nothing;
//! - OpenCode and the names under it get the conversation header and Aura's name in User-Agent;
//! - Anthropic's own Messages endpoint gets cache breakpoints;
//! - xAI's API, over https, gets the prompt cache key in the body on Responses and the
//!   conversation header on Chat Completions, and nothing on its Messages endpoint;
//! - Vercel's AI Gateway, over https, gets the session header on every protocol;
//! - Amazon Bedrock's endpoints under api.aws, over https, are asked to keep nothing of a
//!   Responses request;
//! - every other host gets nothing.
//!
//! Hosts are compared as the URL parser gives them: in lower case, a trailing dot naming the
//! same host.
use aura_model_domain::ModelApiFormat;
use aura_provider_profile::{RequestRules, RoutingKey, request_rules};
use url::Url;

const FORMATS: [ModelApiFormat; 3] = [
    ModelApiFormat::OpenAiResponses,
    ModelApiFormat::OpenAiChatCompletions,
    ModelApiFormat::AnthropicMessages,
];

fn rules(url: &str, format: ModelApiFormat) -> RequestRules {
    request_rules(&Url::parse(url).expect("a URL"), format)
}

fn openai_cache() -> RequestRules {
    RequestRules {
        routing_key: Some(RoutingKey::PromptPrefixField { name: "prompt_cache_key".to_owned() }),
        aura_user_agent: false,
        state_before_conversation: true,
        cache_breakpoints: false,
        store_off: false,
        encrypted_reasoning: false,
    }
}

fn openai_api() -> RequestRules {
    RequestRules { store_off: true, encrypted_reasoning: true, ..openai_cache() }
}

fn opencode() -> RequestRules {
    RequestRules {
        routing_key: Some(RoutingKey::ConversationHeader { name: "x-opencode-session".to_owned() }),
        aura_user_agent: true,
        state_before_conversation: false,
        cache_breakpoints: false,
        store_off: false,
        encrypted_reasoning: false,
    }
}

#[test]
fn openais_api_gets_the_prompt_cache_key_and_the_state_first_and_keeps_nothing() {
    for url in [
        "https://api.openai.com/v1/responses",
        "https://api.openai.com/v1/chat/completions",
        "https://API.OpenAI.com/v1/responses",
        "https://api.openai.com./v1/responses",
        "https://api.openai.com:443/v1/responses",
    ] {
        for format in FORMATS {
            assert_eq!(rules(url, format), openai_api(), "{url} {format:?}");
        }
    }
    for url in [
        "http://api.openai.com/v1/responses",
        "https://eu.api.openai.com/v1/responses",
        "https://api.openai.com.example.test/v1/responses",
        "https://openai.com/v1/responses",
    ] {
        assert_eq!(rules(url, ModelApiFormat::OpenAiResponses), RequestRules::default(), "{url}");
    }
}

#[test]
fn a_responses_gateway_on_this_computer_is_treated_as_openais_cache() {
    for url in [
        "http://127.0.0.1:8317/v1/responses",
        "http://127.0.0.2:8317/v1/responses",
        "http://localhost:8080/v1/responses",
        "http://LOCALHOST./v1/responses",
        "https://localhost:8443/v1/responses",
        "http://[::1]:9000/v1/responses",
    ] {
        assert_eq!(rules(url, ModelApiFormat::OpenAiResponses), openai_cache(), "{url}");
        for format in [ModelApiFormat::OpenAiChatCompletions, ModelApiFormat::AnthropicMessages] {
            assert_eq!(rules(url, format), RequestRules::default(), "{url} {format:?}");
        }
    }
    for url in [
        "http://10.0.0.2:8317/v1/responses",
        "http://localhost.example.test/v1/responses",
        "http://[::2]:9000/v1/responses",
        "http://my-localhost/v1/responses",
    ] {
        assert_eq!(rules(url, ModelApiFormat::OpenAiResponses), RequestRules::default(), "{url}");
    }
}

#[test]
fn opencode_and_the_names_under_it_get_the_conversation_header_and_auras_name() {
    for url in [
        "https://opencode.ai/zen/go/v1/chat/completions",
        "https://opencode.ai/inference/go/openai/v1/responses",
        "https://opencode.ai/inference/go/anthropic/v1/messages",
        "https://api.opencode.ai/v1/chat/completions",
        "https://OpenCode.AI/zen/v1/chat/completions",
        "https://opencode.ai./zen/go/v1/chat/completions",
    ] {
        for format in FORMATS {
            assert_eq!(rules(url, format), opencode(), "{url} {format:?}");
        }
    }
    for url in [
        "https://opencode.ai.example.test/v1/chat/completions",
        "https://notopencode.ai/v1/chat/completions",
        "https://opencode.dev/v1/chat/completions",
    ] {
        assert_eq!(
            rules(url, ModelApiFormat::OpenAiChatCompletions),
            RequestRules::default(),
            "{url}"
        );
    }
}

#[test]
fn anthropics_own_messages_endpoint_gets_cache_breakpoints() {
    let breakpoints = RequestRules { cache_breakpoints: true, ..RequestRules::default() };
    for url in [
        "https://api.anthropic.com/v1/messages",
        "https://API.Anthropic.com./v1/messages",
        "https://api.anthropic.com:443/v1/messages",
    ] {
        assert_eq!(rules(url, ModelApiFormat::AnthropicMessages), breakpoints, "{url}");
    }
    for url in [
        "https://api.deepseek.com/anthropic/v1/messages",
        "https://api.anthropic.com.example.test/v1/messages",
        "http://api.anthropic.com/v1/messages",
        "https://api.example.test/v1/messages",
    ] {
        assert_eq!(rules(url, ModelApiFormat::AnthropicMessages), RequestRules::default(), "{url}");
    }
    assert_eq!(
        rules(
            "https://api.anthropic.com/v1/chat/completions",
            ModelApiFormat::OpenAiChatCompletions
        ),
        RequestRules::default()
    );
}

fn conversation_header(name: &str) -> RequestRules {
    RequestRules {
        routing_key: Some(RoutingKey::ConversationHeader { name: name.to_owned() }),
        ..RequestRules::default()
    }
}

#[test]
fn xais_api_gets_its_cache_key_by_the_protocols_own_name() {
    let prompt_cache_key = RequestRules {
        routing_key: Some(RoutingKey::PromptPrefixField { name: "prompt_cache_key".to_owned() }),
        ..RequestRules::default()
    };
    for url in ["https://api.x.ai/v1/responses", "https://API.X.AI./v1/responses"] {
        assert_eq!(rules(url, ModelApiFormat::OpenAiResponses), prompt_cache_key, "{url}");
    }
    assert_eq!(
        rules("https://api.x.ai/v1/chat/completions", ModelApiFormat::OpenAiChatCompletions),
        conversation_header("x-grok-conv-id")
    );
    assert_eq!(
        rules("https://api.x.ai/v1/messages", ModelApiFormat::AnthropicMessages),
        RequestRules::default()
    );
    for url in [
        "http://api.x.ai/v1/responses",
        "https://x.ai/v1/responses",
        "https://api.x.ai.example.test/v1/responses",
        "https://eu.api.x.ai/v1/responses",
    ] {
        for format in FORMATS {
            assert_eq!(rules(url, format), RequestRules::default(), "{url} {format:?}");
        }
    }
}

#[test]
fn vercels_ai_gateway_gets_the_session_header() {
    for url in [
        "https://ai-gateway.vercel.sh/v1/chat/completions",
        "https://ai-gateway.vercel.sh/v1/responses",
        "https://AI-Gateway.Vercel.sh./v1/messages",
    ] {
        for format in FORMATS {
            assert_eq!(rules(url, format), conversation_header("x-session-id"), "{url} {format:?}");
        }
    }
    for url in [
        "http://ai-gateway.vercel.sh/v1/chat/completions",
        "https://vercel.sh/v1/chat/completions",
        "https://my-app.vercel.app/v1/chat/completions",
        "https://ai-gateway.vercel.sh.example.test/v1/chat/completions",
    ] {
        for format in FORMATS {
            assert_eq!(rules(url, format), RequestRules::default(), "{url} {format:?}");
        }
    }
}

#[test]
fn bedrock_keeps_nothing_of_a_responses_request() {
    let store_off = RequestRules { store_off: true, ..RequestRules::default() };
    for url in [
        "https://bedrock-mantle.us-east-1.api.aws/v1/responses",
        "https://bedrock-mantle.eu-west-1.api.aws/openai/v1/responses",
        "https://Bedrock-Mantle.us-east-1.API.AWS./v1/responses",
    ] {
        assert_eq!(rules(url, ModelApiFormat::OpenAiResponses), store_off, "{url}");
        for format in [ModelApiFormat::OpenAiChatCompletions, ModelApiFormat::AnthropicMessages] {
            assert_eq!(rules(url, format), RequestRules::default(), "{url} {format:?}");
        }
    }
    for url in [
        "http://bedrock-mantle.us-east-1.api.aws/v1/responses",
        "https://bedrock-mantle.us-east-1.api.aws.example.test/v1/responses",
        "https://notapi.aws/v1/responses",
    ] {
        assert_eq!(rules(url, ModelApiFormat::OpenAiResponses), RequestRules::default(), "{url}");
    }
}

#[test]
fn every_other_host_gets_nothing() {
    for url in [
        "https://api.deepseek.com/responses",
        "https://api.example.test/v1/responses",
        "https://openrouter.ai/api/v1/chat/completions",
        "https://open.bigmodel.cn/api/coding/paas/v4/chat/completions",
        "https://api.groq.com/openai/v1/responses",
    ] {
        for format in FORMATS {
            assert_eq!(rules(url, format), RequestRules::default(), "{url} {format:?}");
        }
    }
}
