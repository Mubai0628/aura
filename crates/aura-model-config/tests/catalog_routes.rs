//! A Provider whose models each go by their own route keeps each model on the connection of its
//! route: a catalog model is added only to that connection, and discovery adds a new model only
//! there, a model the catalog does not list going by the Provider's own route. A model whose
//! route Aura does not speak is not added to any of the routes. A connection at an address of the
//! user's own, a model the user adds by hand and a model already on a connection are left as they
//! are; a Provider whose routes are alternatives takes any model on any of them. A route whose
//! address takes a value of the user's own, such as Bedrock's region, is the route at any value.
use std::collections::BTreeSet;

use aura_domain_common::{ContentHash, EndpointId, UtcTimestamp};
use aura_model_config::{
    ModelConfigError, add_manual_model, create_model_from_catalog, merge_discovery_results,
};
use aura_model_domain::{
    AddManualModelInput, ModelApiFormat, ModelRegistryConfig, ProviderCatalogSnapshot,
};

mod support;

const GO_OPENAI: &str = "https://opencode.ai/inference/go/openai/v1";
const GO_ANTHROPIC: &str = "https://opencode.ai/inference/go/anthropic/v1";
const GO_GOOGLE: &str = "https://opencode.ai/inference/go/google/v1beta";
const OWN_ADDRESS: &str = "https://opencode.ai/zen/go/v1";

/// OpenCode Go with a model on each kind of route: its own Chat route, the Anthropic route, and
/// Google's, which Aura does not speak.
fn catalog() -> ProviderCatalogSnapshot {
    support::provider_catalog(
        "opencode-go",
        Some("@ai-sdk/openai-compatible"),
        Some(GO_OPENAI),
        vec![
            support::catalog_model("glm-5.3", None),
            support::catalog_model_at("minimax-m3", "@ai-sdk/anthropic", GO_ANTHROPIC),
            support::catalog_model_at("gemini-4-argon", "@ai-sdk/google", GO_GOOGLE),
        ],
    )
}

/// OpenCode Go's three routes and a connection at an address of the user's own, in this order.
fn registry() -> (ModelRegistryConfig, [EndpointId; 4]) {
    let (registry, ids) = support::registry(
        "opencode-go",
        &[
            (GO_OPENAI, ModelApiFormat::OpenAiChatCompletions),
            (GO_OPENAI, ModelApiFormat::OpenAiResponses),
            (GO_ANTHROPIC, ModelApiFormat::AnthropicMessages),
            (OWN_ADDRESS, ModelApiFormat::OpenAiResponses),
        ],
    );
    (registry, ids.try_into().expect("four connections"))
}

fn add(
    registry: &mut ModelRegistryConfig,
    catalog: &ProviderCatalogSnapshot,
    endpoint: EndpointId,
    id: &str,
) -> Result<(), ModelConfigError> {
    let provider = catalog.providers.keys().next().expect("one Provider").clone();
    let model = &catalog.providers[&provider].models[id];
    let hash = ContentHash::blake3(&serde_json::to_vec(model).expect("model JSON"));
    create_model_from_catalog(
        registry,
        endpoint,
        catalog,
        &provider,
        id,
        &hash,
        UtcTimestamp::now(),
    )
    .map(|_| ())
}

fn names_on(registry: &ModelRegistryConfig, endpoint: EndpointId) -> BTreeSet<&str> {
    registry.endpoints[&endpoint]
        .model_order
        .iter()
        .map(|id| registry.models[id].provider_model_name.as_str())
        .collect()
}

#[test]
fn a_catalog_model_is_added_only_to_the_connection_of_its_route() {
    let catalog = catalog();
    let (mut registry, [chat, responses, anthropic, own]) = registry();

    add(&mut registry, &catalog, chat, "glm-5.3").expect("its own route");
    let refused = add(&mut registry, &catalog, responses, "glm-5.3");
    assert!(
        matches!(&refused, Err(ModelConfigError::CatalogModelOnAnotherRoute { catalog_model_id, route_id, .. })
            if catalog_model_id == "glm-5.3" && route_id == "chat-completions"),
        "{refused:?}"
    );
    let refused = add(&mut registry, &catalog, chat, "minimax-m3");
    assert!(
        matches!(&refused, Err(ModelConfigError::CatalogModelOnAnotherRoute { route_id, .. })
            if route_id == "anthropic-messages"),
        "{refused:?}"
    );
    add(&mut registry, &catalog, anthropic, "minimax-m3").expect("its own route");
    for endpoint in [chat, responses, anthropic] {
        let refused = add(&mut registry, &catalog, endpoint, "gemini-4-argon");
        assert!(
            matches!(&refused, Err(ModelConfigError::CatalogModelRouteUnsupported { catalog_model_id, .. })
                if catalog_model_id == "gemini-4-argon"),
            "{refused:?}"
        );
    }
    // An address of the user's own is no route of the profile: it takes what the user adds.
    for id in ["glm-5.3", "minimax-m3", "gemini-4-argon"] {
        add(&mut registry, &catalog, own, id).expect("the user's own address");
    }

    assert_eq!(names_on(&registry, chat), BTreeSet::from(["glm-5.3"]));
    assert!(names_on(&registry, responses).is_empty());
    assert_eq!(names_on(&registry, anthropic), BTreeSet::from(["minimax-m3"]));
    assert_eq!(names_on(&registry, own).len(), 3);
}

#[test]
fn discovery_adds_a_new_model_only_to_the_connection_of_its_route() {
    let catalog = catalog();
    let (mut registry, [chat, responses, anthropic, own]) = registry();
    let names =
        ["glm-5.3", "minimax-m3", "gemini-4-argon", "not-in-the-catalog"].map(str::to_owned);
    for endpoint in [chat, responses, anthropic, own] {
        merge_discovery_results(
            &mut registry,
            endpoint,
            &names,
            Some(&catalog),
            UtcTimestamp::now(),
        )
        .expect("merge");
    }
    assert_eq!(names_on(&registry, chat), BTreeSet::from(["glm-5.3", "not-in-the-catalog"]));
    assert!(names_on(&registry, responses).is_empty());
    assert_eq!(names_on(&registry, anthropic), BTreeSet::from(["minimax-m3"]));
    assert_eq!(names_on(&registry, own).len(), 4);
}

#[test]
fn a_model_the_user_put_on_another_route_stays_there() {
    let catalog = catalog();
    let (mut registry, [_, responses, _, _]) = registry();
    add_manual_model(
        &mut registry,
        AddManualModelInput {
            endpoint_id: responses,
            provider_model_name: "glm-5.3".to_owned(),
            display_name: None,
            notes: None,
            context_window_tokens: None,
        },
        UtcTimestamp::now(),
    )
    .expect("a model added by hand goes where the user puts it");
    let merged = merge_discovery_results(
        &mut registry,
        responses,
        &["glm-5.3".to_owned()],
        Some(&catalog),
        UtcTimestamp::now(),
    )
    .expect("merge");
    assert_eq!(merged.returned_profiles.len(), 1);
    assert!(!merged.returned_profiles[0].created);
    assert_eq!(names_on(&registry, responses), BTreeSet::from(["glm-5.3"]));
}

#[test]
fn a_provider_whose_routes_are_alternatives_takes_any_model_on_any_of_them() {
    let catalog = support::provider_catalog(
        "deepseek",
        Some("@ai-sdk/openai-compatible"),
        Some("https://api.deepseek.com"),
        vec![support::catalog_model_at(
            "deepseek-v4-pro",
            "@ai-sdk/anthropic",
            "https://api.deepseek.com/anthropic/v1",
        )],
    );
    let (mut registry, ids) = support::registry(
        "deepseek",
        &[
            ("https://api.deepseek.com", ModelApiFormat::OpenAiResponses),
            ("https://api.deepseek.com", ModelApiFormat::OpenAiChatCompletions),
        ],
    );
    for endpoint in ids {
        add(&mut registry, &catalog, endpoint, "deepseek-v4-pro").expect("any route");
    }
}

#[test]
fn a_bedrock_model_goes_to_the_connection_of_its_mantle_prefix_in_any_region() {
    let mantle = |id: &str, api: &str| {
        let mut model = support::catalog_model_at(id, "@ai-sdk/amazon-bedrock/mantle", api);
        if let Some(route) = model.route.as_mut() {
            route.shape = Some("responses".to_owned());
        }
        model
    };
    let catalog = support::provider_catalog(
        "amazon-bedrock",
        Some("@ai-sdk/amazon-bedrock"),
        None,
        vec![
            mantle("openai.gpt-oss-120b", "https://bedrock-mantle.${AWS_REGION}.api.aws/v1"),
            mantle("openai.gpt-6-luna", "https://bedrock-mantle.${AWS_REGION}.api.aws/openai/v1"),
            support::catalog_model("anthropic.claude-haiku-5-5", None),
        ],
    );
    let (mut registry, ids) = support::registry(
        "amazon-bedrock",
        &[
            ("https://bedrock-mantle.eu-central-1.api.aws/v1", ModelApiFormat::OpenAiResponses),
            (
                "https://bedrock-mantle.eu-central-1.api.aws/openai/v1",
                ModelApiFormat::OpenAiResponses,
            ),
        ],
    );
    let [v1, openai]: [EndpointId; 2] = ids.try_into().expect("two connections");

    add(&mut registry, &catalog, v1, "openai.gpt-oss-120b").expect("its own prefix");
    add(&mut registry, &catalog, openai, "openai.gpt-6-luna").expect("its own prefix");
    let refused = add(&mut registry, &catalog, v1, "openai.gpt-6-luna");
    assert!(
        matches!(&refused, Err(ModelConfigError::CatalogModelOnAnotherRoute { route_id, .. })
            if route_id == "mantle-openai-v1"),
        "{refused:?}"
    );
    // Bedrock's own protocol, which Aura does not speak.
    for endpoint in [v1, openai] {
        let refused = add(&mut registry, &catalog, endpoint, "anthropic.claude-haiku-5-5");
        assert!(
            matches!(&refused, Err(ModelConfigError::CatalogModelRouteUnsupported { .. })),
            "{refused:?}"
        );
    }
    assert_eq!(names_on(&registry, v1), BTreeSet::from(["openai.gpt-oss-120b"]));
    assert_eq!(names_on(&registry, openai), BTreeSet::from(["openai.gpt-6-luna"]));
}
