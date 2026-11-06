//! A route's base URL may take values of the user's own, each standing for one whole label of
//! the host or one whole segment of the path:
//! - a value that is not one whole label, or one whole segment, fills nothing;
//! - a connection is the route when its base URL is the template with valid values, the host's
//!   case, a default port, a trailing dot of the host and a trailing slash not counting, and the
//!   values come back;
//! - no address of another host, path, scheme or port is the route, whatever the values;
//! - a template that names a value twice, or not at all, is no connection's route;
//! - Bedrock's routes are the two path prefixes of its Mantle endpoint, at the region the user
//!   gives, and each Mantle model of the catalog goes by the prefix its entry names.
use std::collections::BTreeMap;

use aura_model_domain::{
    EndpointAuthScheme, ModelApiFormat, ModelDiscoveryMode, ProviderCatalogEntry,
    ProviderCatalogModelRoute, ProviderPresetParameterKind,
};
use aura_provider_profile::{
    ModelRoute, ProviderRoute, RouteParameter, fill_base_url, match_base_url,
    parameter_value_is_valid, route_for_catalog_model, route_of_connection,
    routes_for_catalog_provider,
};

use ProviderPresetParameterKind::{DomainLabel, PathSegment};

const MANTLE: &str = "@ai-sdk/amazon-bedrock/mantle";

fn parameter(name: &str, kind: ProviderPresetParameterKind, example: &str) -> RouteParameter {
    RouteParameter {
        name: name.to_owned(),
        display_name: name.to_owned(),
        kind,
        example: example.to_owned(),
    }
}

/// A gateway whose address takes an account as a host label and a gateway as a path segment.
fn gateway() -> ProviderRoute {
    ProviderRoute {
        route_id: "gateway".to_owned(),
        display_name: "Gateway".to_owned(),
        base_url: "https://{account}.gateway.example.test/v1/{gateway}/openai".to_owned(),
        api_format: ModelApiFormat::OpenAiResponses,
        auth_scheme: EndpointAuthScheme::OpenAiBearerApiKey,
        discovery_mode: ModelDiscoveryMode::Disabled,
        chat_instruction_role: None,
        chat_output_token_field: None,
        recommended: false,
        catalog_routes: Vec::new(),
        parameters: vec![
            parameter("account", DomainLabel, "acme"),
            parameter("gateway", PathSegment, "main"),
        ],
    }
}

fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
}

#[test]
fn a_value_stands_for_one_whole_label_or_one_whole_segment() {
    let long_label = "a".repeat(63);
    for value in ["us-east-1", "a", "x1", "0", "xn--fiq228c", long_label.as_str()] {
        assert!(parameter_value_is_valid(DomainLabel, value), "{value}");
    }
    let too_long_label = "a".repeat(64);
    for value in [
        "",
        "-a",
        "a-",
        "a.b",
        "a/b",
        "a@b",
        "a:1",
        "US-EAST-1",
        "a b",
        "a%2e",
        "a_b",
        "évian",
        too_long_label.as_str(),
    ] {
        assert!(!parameter_value_is_valid(DomainLabel, value), "{value:?}");
    }
    let long_segment = "a".repeat(128);
    for value in ["main", "A_b.c~-1", "...a", long_segment.as_str()] {
        assert!(parameter_value_is_valid(PathSegment, value), "{value}");
    }
    let too_long_segment = "a".repeat(129);
    for value in [
        "",
        ".",
        "..",
        "a/b",
        "a?b",
        "a#b",
        "a@b",
        "a%2Fb",
        "a b",
        "a\\b",
        too_long_segment.as_str(),
    ] {
        assert!(!parameter_value_is_valid(PathSegment, value), "{value:?}");
    }
}

#[test]
fn a_template_filled_with_valid_values_is_its_routes_connection_and_gives_them_back() {
    let route = gateway();
    let given = values(&[("account", "acme"), ("gateway", "main")]);
    assert_eq!(
        fill_base_url(&route, &given).as_deref(),
        Some("https://acme.gateway.example.test/v1/main/openai")
    );
    for address in [
        "https://acme.gateway.example.test/v1/main/openai",
        "https://ACME.Gateway.Example.Test./v1/main/openai/",
        "https://acme.gateway.example.test:443/v1/main/openai",
    ] {
        assert_eq!(match_base_url(&route, address), Some(given.clone()), "{address}");
    }
    for missing_or_invalid in [
        values(&[("account", "acme")]),
        values(&[("account", "evil.test"), ("gateway", "main")]),
        values(&[("account", "acme"), ("gateway", "a/b")]),
        values(&[("account", ""), ("gateway", "main")]),
    ] {
        assert_eq!(fill_base_url(&route, &missing_or_invalid), None, "{missing_or_invalid:?}");
    }
}

#[test]
fn no_address_of_another_host_path_scheme_or_port_is_the_route() {
    let route = gateway();
    for address in [
        "https://evil.test/v1/main/openai",
        "https://acme.evil.gateway.example.test/v1/main/openai",
        "https://acme.gateway.example.test.evil.test/v1/main/openai",
        "https://gateway.example.test/v1/main/openai",
        "https://a_b.gateway.example.test/v1/main/openai",
        "https://acme.gateway.example.test/v1/main/extra/openai",
        "https://acme.gateway.example.test/v1/main/openai/extra",
        "https://acme.gateway.example.test/v1/openai",
        "https://acme.gateway.example.test/v2/main/openai",
        "https://acme.gateway.example.test/v1/a%2Fb/openai",
        "https://acme.gateway.example.test/v1/main/openai?account=evil",
        "https://acme.gateway.example.test/v1/main/openai#account=evil",
        "https://user@acme.gateway.example.test/v1/main/openai",
        "http://acme.gateway.example.test/v1/main/openai",
        // The template's port under another scheme.
        "http://acme.gateway.example.test:443/v1/main/openai",
        "https://acme.gateway.example.test:8443/v1/main/openai",
        "not a url",
    ] {
        assert_eq!(match_base_url(&route, address), None, "{address}");
    }
}

#[test]
fn a_template_that_names_a_value_twice_or_not_at_all_is_no_connections_route() {
    let mut twice = gateway();
    twice.base_url = "https://{account}.gateway.example.test/v1/{gateway}/{gateway}".to_owned();
    for address in [
        "https://acme.gateway.example.test/v1/main/main",
        "https://acme.gateway.example.test/v1/main/openai",
    ] {
        assert_eq!(match_base_url(&twice, address), None, "{address}");
    }
    let mut not_at_all = gateway();
    not_at_all.base_url = "https://{account}.gateway.example.test/v1/main/openai".to_owned();
    assert_eq!(
        match_base_url(&not_at_all, "https://acme.gateway.example.test/v1/main/openai"),
        None
    );
}

#[test]
fn bedrocks_routes_are_mantles_two_path_prefixes_at_the_region_the_user_gives() {
    let routes = routes_for_catalog_provider("amazon-bedrock");
    let summary = routes
        .iter()
        .map(|route| {
            (
                route.route_id.as_str(),
                route.display_name.as_str(),
                route.base_url.as_str(),
                route.api_format,
                route.auth_scheme,
                route.discovery_mode.clone(),
                route.parameters.clone(),
            )
        })
        .collect::<Vec<_>>();
    let region = vec![RouteParameter {
        name: "region".to_owned(),
        display_name: "区域".to_owned(),
        kind: DomainLabel,
        example: "us-east-1".to_owned(),
    }];
    assert_eq!(
        summary,
        vec![
            (
                "mantle-v1",
                "Amazon Bedrock Mantle /v1",
                "https://bedrock-mantle.{region}.api.aws/v1",
                ModelApiFormat::OpenAiResponses,
                EndpointAuthScheme::OpenAiBearerApiKey,
                ModelDiscoveryMode::Disabled,
                region.clone(),
            ),
            (
                "mantle-openai-v1",
                "Amazon Bedrock Mantle /openai/v1",
                "https://bedrock-mantle.{region}.api.aws/openai/v1",
                ModelApiFormat::OpenAiResponses,
                EndpointAuthScheme::OpenAiBearerApiKey,
                ModelDiscoveryMode::Disabled,
                region,
            ),
        ]
    );

    let route_of = |address: &str, format: ModelApiFormat| {
        route_of_connection("amazon-bedrock", address, format).map(|route| route.route_id.as_str())
    };
    let responses = ModelApiFormat::OpenAiResponses;
    assert_eq!(
        route_of("https://bedrock-mantle.eu-central-1.api.aws/v1", responses),
        Some("mantle-v1")
    );
    assert_eq!(
        route_of("https://bedrock-mantle.eu-central-1.api.aws/openai/v1/", responses),
        Some("mantle-openai-v1")
    );
    assert_eq!(
        match_base_url(&routes[0], "https://bedrock-mantle.ap-northeast-1.api.aws/v1"),
        Some(values(&[("region", "ap-northeast-1")]))
    );
    for (address, format) in [
        ("https://bedrock-mantle.eu-central-1.api.aws/v1", ModelApiFormat::OpenAiChatCompletions),
        ("https://bedrock-mantle.evil.test.api.aws/v1", responses),
        ("https://bedrock-mantle.us-east-1.api.aws.evil.test/v1", responses),
        ("https://bedrock-runtime.us-east-1.amazonaws.com/openai/v1", responses),
        ("https://bedrock-mantle.{region}.api.aws/v1", responses),
    ] {
        assert_eq!(route_of(address, format), None, "{address} {format:?}");
    }
}

#[test]
fn a_bedrock_model_goes_by_the_mantle_prefix_its_catalog_entry_names() {
    let bedrock = ProviderCatalogEntry {
        catalog_provider_id: "amazon-bedrock".to_owned(),
        display_name: "Amazon Bedrock".to_owned(),
        npm_package: Some("@ai-sdk/amazon-bedrock".to_owned()),
        api_url: None,
        documentation_url: None,
        model_order: Vec::new(),
        models: BTreeMap::new(),
    };
    let own = |npm: &str, api: &str, shape: Option<&str>| ProviderCatalogModelRoute {
        api_url: Some(api.to_owned()),
        npm_package: Some(npm.to_owned()),
        shape: shape.map(str::to_owned),
    };
    let goes_by = |route: Option<ProviderCatalogModelRoute>| match route_for_catalog_model(
        &bedrock,
        route.as_ref(),
    ) {
        ModelRoute::Any => "any".to_owned(),
        ModelRoute::Route(route) => route.route_id.clone(),
        ModelRoute::Unsupported => "unsupported".to_owned(),
    };
    let cases = [
        (
            Some(own(
                MANTLE,
                "https://bedrock-mantle.${AWS_REGION}.api.aws/openai/v1",
                Some("responses"),
            )),
            "mantle-openai-v1",
        ),
        (
            Some(own(MANTLE, "https://bedrock-mantle.${AWS_REGION}.api.aws/v1", Some("responses"))),
            "mantle-v1",
        ),
        (
            Some(own(
                MANTLE,
                "https://bedrock-mantle.${AWS_REGION}.api.aws/v1/",
                Some("responses"),
            )),
            "mantle-v1",
        ),
        // Bedrock's own protocol, which most of its models go by.
        (None, "unsupported"),
        (Some(own(MANTLE, "https://bedrock-mantle.${AWS_REGION}.api.aws/v1", None)), "unsupported"),
        (
            Some(own(
                MANTLE,
                "https://bedrock-mantle.${AWS_REGION}.api.aws/v1",
                Some("completions"),
            )),
            "unsupported",
        ),
        (
            Some(own(
                MANTLE,
                "https://bedrock-mantle.${AWS_DEFAULT_REGION}.api.aws/v1",
                Some("responses"),
            )),
            "unsupported",
        ),
        // The catalog's own value is compared as written, though a URL parser would take it.
        (
            Some(own(MANTLE, "https://bedrock-mantle.${aws_region}.api.aws/v1", Some("responses"))),
            "unsupported",
        ),
        (
            Some(own(
                "@ai-sdk/openai",
                "https://bedrock-mantle.${AWS_REGION}.api.aws/v1",
                Some("responses"),
            )),
            "unsupported",
        ),
    ];
    for (route, expected) in cases {
        assert_eq!(goes_by(route.clone()), expected, "{route:?}");
    }
}
