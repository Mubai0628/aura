//! The profiles as the catalog page shows them.
use aura_model_domain::{
    ProviderCatalogConnectionDefaultsView, ProviderCatalogEntry, ProviderCatalogModelRoute,
    ProviderCatalogModelRouteView, ProviderPresetParameterView, ProviderPresetVariantView,
};

use crate::{ModelRoute, RouteChoice, profile_for_catalog_provider, route_for_catalog_model};

/// The preset variants a connection to the catalog Provider `catalog_provider_id` starts from.
#[must_use]
pub fn preset_defaults(catalog_provider_id: &str) -> Option<ProviderCatalogConnectionDefaultsView> {
    let profile = profile_for_catalog_provider(catalog_provider_id)?;
    let variants = profile
        .routes
        .iter()
        .map(|route| ProviderPresetVariantView {
            variant_id: route.route_id.clone(),
            display_name: route.display_name.clone(),
            base_url: route.base_url.clone(),
            api_format: route.api_format,
            auth_scheme: route.auth_scheme,
            discovery_mode: route.discovery_mode.clone(),
            chat_instruction_role: route.chat_instruction_role,
            chat_output_token_field: route.chat_output_token_field,
            recommended: route.recommended,
            base_url_parameters: route
                .parameters
                .iter()
                .map(|parameter| ProviderPresetParameterView {
                    name: parameter.name.clone(),
                    display_name: parameter.display_name.clone(),
                    kind: parameter.kind,
                    example: parameter.example.clone(),
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    (!variants.is_empty()).then_some(ProviderCatalogConnectionDefaultsView::AuraPreset {
        variants,
        by_model: profile.route_choice == RouteChoice::ByModel,
    })
}

/// Which preset variant the model of `provider` whose own catalog route is `model_route` goes by.
#[must_use]
pub fn model_route_view(
    provider: &ProviderCatalogEntry,
    model_route: Option<&ProviderCatalogModelRoute>,
) -> ProviderCatalogModelRouteView {
    match route_for_catalog_model(provider, model_route) {
        ModelRoute::Any => ProviderCatalogModelRouteView::Any,
        ModelRoute::Route(route) => {
            ProviderCatalogModelRouteView::Variant { variant_id: route.route_id.clone() }
        }
        ModelRoute::Unsupported => ProviderCatalogModelRouteView::Unsupported,
    }
}
