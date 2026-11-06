//! A Provider whose models each go by their own route: a model goes by the profile route whose
//! catalog routes include the one its catalog entry names, and the catalog's package and address
//! only select one of the profile's routes. A model whose catalog route the profile does not have
//! is not used automatically. A connection is a route when its address and protocol are the
//! route's, and a route's connection takes only that route's models. A Provider whose routes are
//! alternatives takes any model on any of them.
use std::collections::BTreeMap;

use aura_model_domain::{ModelApiFormat, ProviderCatalogEntry, ProviderCatalogModelRoute};
use aura_provider_profile::{
    ModelRoute, connection_takes_model, route_for_catalog_model, route_of_connection,
};

const GO_OPENAI: &str = "https://opencode.ai/inference/go/openai/v1";
const GO_ANTHROPIC: &str = "https://opencode.ai/inference/go/anthropic/v1";
const GO_GOOGLE: &str = "https://opencode.ai/inference/go/google/v1beta";
const ZEN_OPENAI: &str = "https://opencode.ai/inference/openai/v1";
const ZEN_ANTHROPIC: &str = "https://opencode.ai/inference/anthropic/v1";
const COMPATIBLE: &str = "@ai-sdk/openai-compatible";

/// A catalog Provider as models.dev lists it, without its models.
fn provider(id: &str, npm: Option<&str>, api: Option<&str>) -> ProviderCatalogEntry {
    ProviderCatalogEntry {
        catalog_provider_id: id.to_owned(),
        display_name: id.to_owned(),
        npm_package: npm.map(str::to_owned),
        api_url: api.map(str::to_owned),
        documentation_url: None,
        model_order: Vec::new(),
        models: BTreeMap::new(),
    }
}

fn opencode_go() -> ProviderCatalogEntry {
    provider("opencode-go", Some(COMPATIBLE), Some(GO_OPENAI))
}

fn own(npm: Option<&str>, api: Option<&str>, shape: Option<&str>) -> ProviderCatalogModelRoute {
    ProviderCatalogModelRoute {
        api_url: api.map(str::to_owned),
        npm_package: npm.map(str::to_owned),
        shape: shape.map(str::to_owned),
    }
}

/// The route id a model goes by, `any`, or `unsupported`.
fn goes_by(provider: &ProviderCatalogEntry, route: Option<ProviderCatalogModelRoute>) -> &str {
    match route_for_catalog_model(provider, route.as_ref()) {
        ModelRoute::Any => "any",
        ModelRoute::Route(route) => route.route_id.as_str(),
        ModelRoute::Unsupported => "unsupported",
    }
}

#[test]
fn opencode_go_models_go_by_the_route_their_catalog_entries_name() {
    let go = opencode_go();
    let cases = [
        (None, "chat-completions"),
        (Some(own(Some("@ai-sdk/openai"), Some(GO_OPENAI), None)), "responses"),
        (Some(own(Some("@ai-sdk/anthropic"), Some(GO_ANTHROPIC), None)), "anthropic-messages"),
        // What the model's own route leaves out is its Provider's.
        (Some(own(None, Some(GO_OPENAI), None)), "chat-completions"),
        (Some(own(Some("@ai-sdk/openai"), None, None)), "responses"),
        (Some(own(None, None, None)), "chat-completions"),
        // A protocol Aura does not speak, an address the profile does not have, a package at
        // another route's address, or a shape the route does not name.
        (Some(own(Some("@ai-sdk/google"), Some(GO_GOOGLE), None)), "unsupported"),
        (
            Some(own(Some(COMPATIBLE), Some("https://elsewhere.example.test/v1"), None)),
            "unsupported",
        ),
        (Some(own(Some("@ai-sdk/anthropic"), Some(GO_OPENAI), None)), "unsupported"),
        (Some(own(Some("@ai-sdk/openai"), Some(GO_OPENAI), Some("completions"))), "unsupported"),
    ];
    for (route, expected) in cases {
        assert_eq!(goes_by(&go, route.clone()), expected, "{route:?}");
    }
}

#[test]
fn opencode_models_go_by_the_routes_of_the_pay_as_you_go_addresses() {
    let zen = provider("opencode", Some(COMPATIBLE), Some(ZEN_OPENAI));
    assert_eq!(goes_by(&zen, None), "chat-completions");
    assert_eq!(
        goes_by(&zen, Some(own(Some("@ai-sdk/openai"), Some(ZEN_OPENAI), None))),
        "responses"
    );
    assert_eq!(
        goes_by(&zen, Some(own(Some("@ai-sdk/anthropic"), Some(ZEN_ANTHROPIC), None))),
        "anthropic-messages"
    );
    assert_eq!(
        goes_by(&zen, Some(own(Some("@ai-sdk/mistral"), Some(ZEN_OPENAI), None))),
        "unsupported"
    );
    // OpenCode Go's addresses are not the pay-as-you-go plan's.
    assert_eq!(
        goes_by(&zen, Some(own(Some("@ai-sdk/anthropic"), Some(GO_ANTHROPIC), None))),
        "unsupported"
    );
}

#[test]
fn a_provider_whose_routes_are_alternatives_or_that_has_no_profile_takes_any_model() {
    let deepseek = provider("deepseek", Some(COMPATIBLE), Some("https://api.deepseek.com"));
    let other = provider("provider-a", Some(COMPATIBLE), Some("https://api.example.test/v1"));
    for provider in [&deepseek, &other] {
        assert_eq!(goes_by(provider, None), "any");
        assert_eq!(
            goes_by(provider, Some(own(Some("@ai-sdk/anthropic"), Some(GO_ANTHROPIC), None))),
            "any"
        );
    }
}

#[test]
fn a_connection_is_a_route_when_its_address_and_protocol_are_the_routes() {
    use ModelApiFormat::{AnthropicMessages, OpenAiChatCompletions, OpenAiResponses};
    let cases = [
        ("opencode-go", GO_OPENAI, OpenAiChatCompletions, Some("chat-completions")),
        (
            "opencode-go",
            "https://opencode.ai/inference/go/openai/v1/",
            OpenAiResponses,
            Some("responses"),
        ),
        (
            "opencode-go",
            "https://OpenCode.AI/inference/go/openai/v1",
            OpenAiChatCompletions,
            Some("chat-completions"),
        ),
        ("opencode-go", GO_ANTHROPIC, AnthropicMessages, Some("anthropic-messages")),
        ("opencode-go", GO_ANTHROPIC, OpenAiChatCompletions, None),
        ("opencode-go", "http://opencode.ai/inference/go/openai/v1", OpenAiChatCompletions, None),
        (
            "opencode-go",
            "https://opencode.ai./inference/go/openai/v1",
            OpenAiChatCompletions,
            Some("chat-completions"),
        ),
        (
            "opencode-go",
            "https://opencode.ai:443/inference/go/openai/v1",
            OpenAiChatCompletions,
            Some("chat-completions"),
        ),
        (
            "opencode-go",
            "https://opencode.ai:8443/inference/go/openai/v1",
            OpenAiChatCompletions,
            None,
        ),
        ("opencode-go", "https://opencode.ai/zen/go/v1", OpenAiChatCompletions, None),
        ("opencode", GO_OPENAI, OpenAiChatCompletions, None),
        ("opencode", ZEN_OPENAI, OpenAiChatCompletions, Some("chat-completions")),
        ("deepseek", "https://api.deepseek.com", OpenAiResponses, Some("responses")),
        ("provider-a", GO_OPENAI, OpenAiChatCompletions, None),
        ("", GO_OPENAI, OpenAiChatCompletions, None),
    ];
    for (catalog_provider_id, base_url, api_format, expected) in cases {
        let got = route_of_connection(catalog_provider_id, base_url, api_format)
            .map(|route| route.route_id.as_str());
        assert_eq!(got, expected, "{catalog_provider_id} {base_url} {api_format:?}");
    }
}

#[test]
fn a_routes_connection_takes_only_that_routes_models() {
    use ModelApiFormat::{AnthropicMessages, OpenAiChatCompletions, OpenAiResponses};
    let go = opencode_go();
    let chat_model = None;
    let anthropic_model = Some(own(Some("@ai-sdk/anthropic"), Some(GO_ANTHROPIC), None));
    let google_model = Some(own(Some("@ai-sdk/google"), Some(GO_GOOGLE), None));
    let takes = |route: &Option<ProviderCatalogModelRoute>, base_url: &str, format| {
        connection_takes_model(&go, route.as_ref(), base_url, format)
    };
    assert!(takes(&chat_model, GO_OPENAI, OpenAiChatCompletions));
    assert!(!takes(&anthropic_model, GO_OPENAI, OpenAiChatCompletions));
    assert!(!takes(&google_model, GO_OPENAI, OpenAiChatCompletions));
    assert!(!takes(&chat_model, GO_OPENAI, OpenAiResponses));
    assert!(takes(&anthropic_model, GO_ANTHROPIC, AnthropicMessages));
    assert!(!takes(&chat_model, GO_ANTHROPIC, AnthropicMessages));
    assert!(!takes(&google_model, GO_ANTHROPIC, AnthropicMessages));
    // A connection at an address of the user's own is no route: it takes what the user adds.
    for model in [&chat_model, &anthropic_model, &google_model] {
        assert!(takes(model, "https://opencode.ai/zen/go/v1", OpenAiResponses));
    }
    let deepseek = provider("deepseek", Some(COMPATIBLE), Some("https://api.deepseek.com"));
    assert!(connection_takes_model(
        &deepseek,
        anthropic_model.as_ref(),
        "https://api.deepseek.com",
        OpenAiResponses
    ));
}
