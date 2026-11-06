//! Provider profiles: how Aura reaches each Provider it knows - the routes a connection can start
//! from, and what requests to the Provider's hosts carry beyond the protocol.
//!
//! The profiles are compiled into the signed program. Catalog data describes Providers and models
//! but cannot change a profile, and the transport and the protocol adapters name no Provider's
//! host: they do what the profile of a request's host asks. What a Provider's error reply means
//! is one table here too, shared by every adapter.
mod builtin;
mod drafts;
mod errors;
mod profile;
mod requests;
mod routes;
mod template;
mod views;

pub use builtin::builtin_profiles;
pub use drafts::catalog_package_protocol;
#[doc(hidden)]
pub use errors::all_phrases;
pub use errors::{
    AccountSetting, ProviderErrorClass, ProviderErrorReply, account_setting,
    classify_provider_error,
};
pub use profile::{
    CatalogRoute, HostPattern, ProviderProfile, ProviderRoute, RequestRule, RequestRules,
    RequestTarget, RouteChoice, RouteParameter, RoutingKey,
};
pub use requests::request_rules;
pub use routes::{
    ModelRoute, connection_takes_model, has_builtin_routes, profile_for_catalog_provider,
    route_for_catalog_model, route_of_connection, routes_for_catalog_provider,
};
pub use template::{fill_base_url, match_base_url, parameter_value_is_valid};
pub use views::{model_route_view, preset_defaults};
