//! Base URLs with parameters. A route's base URL may take values of the user's own, such as an
//! account or a region, each standing for one whole label of the host or one whole segment of
//! the path, so that no value can move a request, or the key it carries, to another host or path.
use std::collections::BTreeMap;

use aura_model_domain::ProviderPresetParameterKind;
use url::Url;

use crate::ProviderRoute;

const LONGEST_LABEL: usize = 63;
const LONGEST_SEGMENT: usize = 128;

/// Whether `value` may stand for a parameter of `kind`.
#[must_use]
pub fn parameter_value_is_valid(kind: ProviderPresetParameterKind, value: &str) -> bool {
    match kind {
        ProviderPresetParameterKind::DomainLabel => {
            (1..=LONGEST_LABEL).contains(&value.len())
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
                && !value.starts_with('-')
                && !value.ends_with('-')
        }
        ProviderPresetParameterKind::PathSegment => {
            (1..=LONGEST_SEGMENT).contains(&value.len())
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
                })
                && value != "."
                && value != ".."
        }
    }
}

/// `route`'s base URL with each parameter replaced by its value; `None` when a value is missing
/// or may not stand for its parameter.
#[must_use]
pub fn fill_base_url(route: &ProviderRoute, values: &BTreeMap<String, String>) -> Option<String> {
    let mut base_url = route.base_url.clone();
    for parameter in &route.parameters {
        let value = values.get(&parameter.name)?;
        if !parameter_value_is_valid(parameter.kind, value) {
            return None;
        }
        base_url = base_url.replace(&format!("{{{}}}", parameter.name), value);
    }
    Some(base_url)
}

/// The values with which `route`'s base URL is `base_url`, the host's case, a default port, a
/// trailing dot of the host and a trailing slash of the path not counting; `None` when no values
/// make it so. A base URL with a query, a fragment or a user name is never a route's, nor is any
/// the route's whose template names a parameter twice or not at all.
#[must_use]
pub fn match_base_url(route: &ProviderRoute, base_url: &str) -> Option<BTreeMap<String, String>> {
    // Each parameter stands for a whole label or segment, so a stand-in valid in either place
    // marks where it is once the template is parsed as any URL is.
    let mut template = route.base_url.clone();
    for (index, parameter) in route.parameters.iter().enumerate() {
        template = template.replace(&format!("{{{}}}", parameter.name), &stand_in(index));
    }
    let template = Url::parse(&template).ok()?;
    let own = Url::parse(base_url).ok()?;
    if own.scheme() != template.scheme()
        || own.port_or_known_default() != template.port_or_known_default()
        || !own.username().is_empty()
        || own.password().is_some()
        || own.query().is_some()
        || own.fragment().is_some()
    {
        return None;
    }
    let mut values = BTreeMap::new();
    let parts = [(labels(&template)?, labels(&own)?), (segments(&template), segments(&own))];
    for (expected, actual) in parts {
        if expected.len() != actual.len() {
            return None;
        }
        for (expected, actual) in expected.iter().zip(&actual) {
            match route
                .parameters
                .iter()
                .enumerate()
                .find(|(index, _)| *expected == stand_in(*index))
            {
                Some((_, parameter)) => {
                    if !parameter_value_is_valid(parameter.kind, actual)
                        || values.insert(parameter.name.clone(), actual.clone()).is_some()
                    {
                        return None;
                    }
                }
                None if expected == actual => {}
                None => return None,
            }
        }
    }
    (values.len() == route.parameters.len()).then_some(values)
}

fn stand_in(index: usize) -> String {
    format!("aura-parameter-{index}")
}

/// The host's labels, in lower case as the parser gives them, a trailing dot not counting.
fn labels(url: &Url) -> Option<Vec<String>> {
    let host = url.host_str()?;
    Some(host.strip_suffix('.').unwrap_or(host).split('.').map(str::to_owned).collect())
}

/// The path's segments, a trailing slash not counting.
fn segments(url: &Url) -> Vec<String> {
    url.path().trim_end_matches('/').split('/').map(str::to_owned).collect()
}
