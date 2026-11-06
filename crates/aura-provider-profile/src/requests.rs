use aura_model_domain::ModelApiFormat;
use url::{Host, Url};

use crate::{HostPattern, RequestRules, RequestTarget, builtin_profiles};

/// What a request to `url` in `api_format` carries beyond the protocol: the rules of the first
/// built-in profile whose targets include it, or nothing.
#[must_use]
pub fn request_rules(url: &Url, api_format: ModelApiFormat) -> RequestRules {
    builtin_profiles()
        .iter()
        .flat_map(|profile| &profile.requests)
        .find(|rule| includes(&rule.target, url, api_format))
        .map(|rule| rule.rules.clone())
        .unwrap_or_default()
}

fn includes(target: &RequestTarget, url: &Url, api_format: ModelApiFormat) -> bool {
    target.scheme.as_deref().is_none_or(|scheme| scheme == url.scheme())
        && (target.api_formats.is_empty() || target.api_formats.contains(&api_format))
        && host_matches(&target.host, url)
}

fn host_matches(pattern: &HostPattern, url: &Url) -> bool {
    match (pattern, url.host()) {
        (HostPattern::Loopback, Some(Host::Ipv4(address))) => address.is_loopback(),
        (HostPattern::Loopback, Some(Host::Ipv6(address))) => address.is_loopback(),
        (pattern, Some(Host::Domain(host))) => {
            // The URL parser has lowered the case; a trailing dot names the same host.
            let host = host.strip_suffix('.').unwrap_or(host);
            match pattern {
                HostPattern::Exact(name) => host == name,
                HostPattern::Domain(name) => {
                    host == name
                        || host.strip_suffix(name.as_str()).is_some_and(|rest| rest.ends_with('.'))
                }
                HostPattern::Loopback => host == "localhost",
            }
        }
        _ => false,
    }
}
