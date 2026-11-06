use aura_model_domain::{ModelApiFormat, ProviderCatalogEntry, ProviderCatalogModelRoute};
use url::Url;

use crate::{ProviderProfile, ProviderRoute, RouteChoice, builtin_profiles, match_base_url};

/// The routes the built-in profiles offer for the catalog Provider `catalog_provider_id`.
#[must_use]
pub fn routes_for_catalog_provider(catalog_provider_id: &str) -> Vec<ProviderRoute> {
    builtin_profiles()
        .iter()
        .filter(|profile| profile.catalog_provider_ids.iter().any(|id| id == catalog_provider_id))
        .flat_map(|profile| profile.routes.iter().cloned())
        .collect()
}

/// Whether a built-in profile offers a route for the catalog Provider `catalog_provider_id`.
#[must_use]
pub fn has_builtin_routes(catalog_provider_id: &str) -> bool {
    !routes_for_catalog_provider(catalog_provider_id).is_empty()
}

/// The built-in profile for the catalog Provider `catalog_provider_id`.
#[must_use]
pub fn profile_for_catalog_provider(catalog_provider_id: &str) -> Option<&'static ProviderProfile> {
    builtin_profiles()
        .iter()
        .find(|profile| profile.catalog_provider_ids.iter().any(|id| id == catalog_provider_id))
}

/// Which route a catalog model goes by.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelRoute<'a> {
    /// Any connection of its Provider takes it.
    Any,
    /// Only a connection of this route takes it.
    Route(&'a ProviderRoute),
    /// No route of the profile serves the catalog route it names.
    Unsupported,
}

/// The route the model whose own catalog route is `model_route` goes by: a model that names
/// none, or leaves a part out, is served where its Provider's other models are.
#[must_use]
pub fn route_for_catalog_model(
    provider: &ProviderCatalogEntry,
    model_route: Option<&ProviderCatalogModelRoute>,
) -> ModelRoute<'static> {
    let Some(profile) = profile_for_catalog_provider(&provider.catalog_provider_id) else {
        return ModelRoute::Any;
    };
    if profile.route_choice == RouteChoice::OneOf {
        return ModelRoute::Any;
    }
    let npm = model_route
        .and_then(|route| route.npm_package.as_deref())
        .or(provider.npm_package.as_deref());
    let api =
        model_route.and_then(|route| route.api_url.as_deref()).or(provider.api_url.as_deref());
    let shape = model_route.and_then(|route| route.shape.as_deref());
    let (Some(npm), Some(api)) = (npm, api.and_then(catalog_address)) else {
        return ModelRoute::Unsupported;
    };
    profile
        .routes
        .iter()
        .find(|route| {
            route.catalog_routes.iter().any(|served| {
                served.npm_package == npm
                    && catalog_address(&served.api_url).as_ref() == Some(&api)
                    && served.shape.as_deref() == shape
            })
        })
        .map_or(ModelRoute::Unsupported, ModelRoute::Route)
}

/// The route of the profile for `catalog_provider_id` that a connection with this address and
/// protocol is: its base URL, with any values of the user's own the route's address takes.
#[must_use]
pub fn route_of_connection(
    catalog_provider_id: &str,
    base_url: &str,
    api_format: ModelApiFormat,
) -> Option<&'static ProviderRoute> {
    let profile = profile_for_catalog_provider(catalog_provider_id)?;
    profile
        .routes
        .iter()
        .find(|route| route.api_format == api_format && match_base_url(route, base_url).is_some())
}

/// Whether a connection of `provider` with this address and protocol takes the catalog model
/// whose own route is `model_route`.
#[must_use]
pub fn connection_takes_model(
    provider: &ProviderCatalogEntry,
    model_route: Option<&ProviderCatalogModelRoute>,
    base_url: &str,
    api_format: ModelApiFormat,
) -> bool {
    let Some(connection) = route_of_connection(&provider.catalog_provider_id, base_url, api_format)
    else {
        // An address of the user's own is no route of the profile.
        return true;
    };
    match route_for_catalog_model(provider, model_route) {
        ModelRoute::Any => true,
        ModelRoute::Route(route) => route.route_id == connection.route_id,
        ModelRoute::Unsupported => false,
    }
}

/// A catalog address as the catalog's routes are compared: as a base URL, the parser giving the
/// host in lower case and leaving a default port out, neither a trailing dot of the host nor a
/// trailing slash of the path counting; one that holds the catalog's own `${NAME}` for a value
/// of the user's own as written, but for a trailing slash.
fn catalog_address(value: &str) -> Option<String> {
    if value.contains("${") {
        return Some(value.trim_end_matches('/').to_owned());
    }
    let url = Url::parse(value).ok()?;
    let host = url.host_str()?;
    let host = host.strip_suffix('.').unwrap_or(host);
    let port = url.port().map(|port| format!(":{port}")).unwrap_or_default();
    Some(format!("{}://{host}{port}{}", url.scheme(), url.path().trim_end_matches('/')))
}
