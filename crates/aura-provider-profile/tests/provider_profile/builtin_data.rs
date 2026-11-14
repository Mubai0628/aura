//! The built-in profiles are well formed:
//! - profile ids are unique, and a catalog Provider belongs to one profile at most;
//! - route ids are unique within a profile, and every route's base URL, filled with its
//!   parameters' examples, is https with a host and nothing else besides a path, but for a
//!   server on this computer that takes no key, which is plain http at a loopback address;
//!   every other route takes a key;
//! - each parameter of a route's base URL stands there once, as one whole label of the host but
//!   never one of its last two, or as one whole segment of the path, and every `{name}` there is
//!   a parameter;
//! - a route's own requests follow its own profile's rules, never another profile's;
//! - in a profile whose models go by their own routes, each route names the catalog routes it
//!   serves, no two routes serve the same one and none is recommended over the others; in a
//!   profile whose routes are alternatives, no route names any;
//! - host patterns are bare lower-case names;
//! - a routing key never names a header that carries a key or that the protocol or the transport
//!   sets, nor a body field the adapters write.
use std::collections::{BTreeMap, BTreeSet};

use aura_model_domain::{EndpointAuthScheme, ProviderPresetParameterKind};
use aura_provider_profile::{
    HostPattern, ProviderRoute, RequestRules, RouteChoice, RoutingKey, builtin_profiles,
    fill_base_url, match_base_url, parameter_value_is_valid, request_rules,
};
use url::{Host, Url};

/// A route's base URL with each parameter at its example.
fn example_url(route: &ProviderRoute) -> String {
    let examples = route
        .parameters
        .iter()
        .map(|parameter| (parameter.name.clone(), parameter.example.clone()))
        .collect::<BTreeMap<_, _>>();
    fill_base_url(route, &examples).unwrap_or_else(|| panic!("{} fills", route.base_url))
}

const RESERVED_HEADERS: [&str; 12] = [
    "authorization",
    "x-api-key",
    "accept",
    "accept-encoding",
    "content-type",
    "content-length",
    "host",
    "user-agent",
    "anthropic-version",
    "anthropic-workspace-id",
    "openai-organization",
    "openai-project",
];

const ADAPTER_FIELDS: [&str; 21] = [
    "model",
    "input",
    "instructions",
    "messages",
    "system",
    "stream",
    "stream_options",
    "tools",
    "tool_choice",
    "max_output_tokens",
    "max_tokens",
    "max_completion_tokens",
    "temperature",
    "top_p",
    "reasoning",
    "reasoning_effort",
    "thinking",
    "text",
    "response_format",
    "store",
    "include",
];

#[test]
fn profile_ids_and_catalog_providers_are_not_shared() {
    let mut profiles = BTreeSet::new();
    let mut catalog = BTreeSet::new();
    for profile in builtin_profiles() {
        assert!(profiles.insert(profile.profile_id.as_str()), "{}", profile.profile_id);
        for id in &profile.catalog_provider_ids {
            assert!(catalog.insert(id.as_str()), "{id} belongs to two profiles");
        }
    }
}

#[test]
fn routes_are_unique_and_their_base_urls_are_plain_https_or_a_keyless_local_server() {
    for profile in builtin_profiles() {
        let mut ids = BTreeSet::new();
        for route in &profile.routes {
            assert!(
                ids.insert(route.route_id.as_str()),
                "{}/{}",
                profile.profile_id,
                route.route_id
            );
            let url = Url::parse(&example_url(route)).expect("a base URL");
            let keyless = route.auth_scheme == EndpointAuthScheme::None;
            let loopback = matches!(url.host(), Some(Host::Ipv4(address)) if address.is_loopback());
            match url.scheme() {
                "https" => assert!(!keyless, "{} takes a key", route.base_url),
                "http" => assert!(keyless && loopback, "{}", route.base_url),
                other => panic!("{}: {other}", route.base_url),
            }
            assert!(url.host_str().is_some_and(|host| !host.is_empty()), "{}", route.base_url);
            assert!(url.username().is_empty() && url.password().is_none(), "{}", route.base_url);
            assert!(url.query().is_none() && url.fragment().is_none(), "{}", route.base_url);
        }
    }
}

#[test]
fn a_routes_parameters_each_stand_for_one_whole_label_or_segment_of_its_own_address() {
    for profile in builtin_profiles() {
        for route in &profile.routes {
            let name = format!("{}/{}", profile.profile_id, route.route_id);
            let named = route
                .base_url
                .split('{')
                .skip(1)
                .map(|rest| rest.split_once('}').map_or(rest, |(name, _)| name))
                .collect::<Vec<_>>();
            let declared = route
                .parameters
                .iter()
                .map(|parameter| parameter.name.as_str())
                .collect::<Vec<_>>();
            assert_eq!(named, declared, "{name}: each parameter stands once, in order");
            let url = Url::parse(&example_url(route)).expect("a base URL");
            let labels = url.host_str().expect("a host").split('.').collect::<Vec<_>>();
            let segments = url.path().split('/').collect::<Vec<_>>();
            let template_labels = route
                .base_url
                .split_once("://")
                .and_then(|(_, rest)| rest.split('/').next())
                .expect("a host")
                .split('.')
                .collect::<Vec<_>>();
            let template_segments = route
                .base_url
                .split_once("://")
                .and_then(|(_, rest)| rest.split_once('/'))
                .map_or(Vec::new(), |(_, path)| path.split('/').collect());
            for parameter in &route.parameters {
                let placeholder = format!("{{{}}}", parameter.name);
                assert!(
                    !parameter.name.is_empty()
                        && parameter
                            .name
                            .bytes()
                            .all(|byte| byte.is_ascii_lowercase() || byte == b'_'),
                    "{name}: {}",
                    parameter.name
                );
                assert!(!parameter.display_name.trim().is_empty(), "{name}");
                assert!(parameter_value_is_valid(parameter.kind, &parameter.example), "{name}");
                match parameter.kind {
                    ProviderPresetParameterKind::DomainLabel => {
                        let at = template_labels
                            .iter()
                            .position(|label| *label == placeholder)
                            .unwrap_or_else(|| panic!("{name}: {placeholder} is a whole label"));
                        assert!(at + 2 < template_labels.len(), "{name}: the domain stays fixed");
                        assert_eq!(labels[at], parameter.example, "{name}");
                    }
                    ProviderPresetParameterKind::PathSegment => {
                        let at = template_segments
                            .iter()
                            .position(|segment| *segment == placeholder)
                            .unwrap_or_else(|| panic!("{name}: {placeholder} is a whole segment"));
                        assert_eq!(segments[at + 1], parameter.example, "{name}");
                    }
                }
            }
            let examples = route
                .parameters
                .iter()
                .map(|parameter| (parameter.name.clone(), parameter.example.clone()))
                .collect::<BTreeMap<_, _>>();
            assert_eq!(match_base_url(route, url.as_str()), Some(examples), "{name}");
        }
    }
}

#[test]
fn a_routes_requests_follow_its_own_profiles_rules() {
    for profile in builtin_profiles() {
        for route in &profile.routes {
            let url = Url::parse(&example_url(route)).expect("a base URL");
            let got = request_rules(&url, route.api_format);
            assert!(
                got == RequestRules::default()
                    || profile.requests.iter().any(|rule| rule.rules == got),
                "{}/{} gets {got:?}",
                profile.profile_id,
                route.route_id
            );
        }
    }
}

#[test]
fn a_by_model_profiles_routes_each_serve_catalog_routes_of_their_own() {
    for profile in builtin_profiles() {
        let mut served = BTreeSet::new();
        for route in &profile.routes {
            let name = format!("{}/{}", profile.profile_id, route.route_id);
            match profile.route_choice {
                RouteChoice::ByModel => {
                    assert!(!route.catalog_routes.is_empty(), "{name} serves no catalog route");
                    assert!(!route.recommended, "{name}");
                }
                RouteChoice::OneOf => assert!(route.catalog_routes.is_empty(), "{name}"),
            }
            for catalog_route in &route.catalog_routes {
                assert!(
                    served.insert((
                        catalog_route.npm_package.as_str(),
                        catalog_route.api_url.as_str(),
                        catalog_route.shape.as_deref(),
                    )),
                    "{name}: {catalog_route:?} is served twice"
                );
                assert!(!catalog_route.npm_package.trim().is_empty(), "{name}");
                // The catalog writes a value of the user's own as `${NAME}`.
                let api_url = catalog_route.api_url.replace("${AWS_REGION}", "region");
                assert!(!api_url.contains('$'), "{name}: {}", catalog_route.api_url);
                let url = Url::parse(&api_url).expect("a catalog address");
                assert_eq!(url.scheme(), "https", "{name}");
                assert!(url.query().is_none() && url.fragment().is_none(), "{name}");
            }
        }
    }
}

#[test]
fn host_patterns_are_bare_lower_case_names() {
    for profile in builtin_profiles() {
        for rule in &profile.requests {
            if let HostPattern::Exact(name) | HostPattern::Domain(name) = &rule.target.host {
                assert!(name.contains('.') && !name.ends_with('.'), "{name}");
                assert!(
                    name.bytes().all(|byte| byte.is_ascii_lowercase()
                        || byte.is_ascii_digit()
                        || byte == b'-'
                        || byte == b'.'),
                    "{name}"
                );
            }
            if let Some(scheme) = &rule.target.scheme {
                assert!(scheme == "https" || scheme == "http", "{scheme}");
            }
        }
    }
}

#[test]
fn routing_keys_never_take_a_key_header_or_a_field_the_adapters_write() {
    for profile in builtin_profiles() {
        for rule in &profile.requests {
            match &rule.rules.routing_key {
                Some(RoutingKey::ConversationHeader { name }) => {
                    assert!(!RESERVED_HEADERS.contains(&name.as_str()), "{name}");
                    assert!(
                        !name.is_empty()
                            && name.bytes().all(|byte| byte.is_ascii_lowercase()
                                || byte.is_ascii_digit()
                                || byte == b'-'),
                        "{name}"
                    );
                }
                Some(RoutingKey::PromptPrefixField { name }) => {
                    assert!(!ADAPTER_FIELDS.contains(&name.as_str()), "{name}");
                    assert!(
                        !name.is_empty()
                            && name.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'_'),
                        "{name}"
                    );
                }
                None => {}
            }
        }
    }
}
