//! What a models.dev model entry says beyond its limits and capabilities: its type, its own
//! route, its reasoning field, its price tiers and its modes. Each is read on its own: a
//! malformed one is left out, and the rest of the catalog is still read.
use std::collections::BTreeMap;

use aura_model_domain::{
    ProviderCatalogModelMode, ProviderCatalogModelRoute, ProviderCatalogPrice,
    ProviderCatalogPriceTier, SPEED_MODE_BODY_FIELDS, SPEED_MODE_HEADERS,
};
use serde_json::{Map, Value};

use crate::parser::{RawCost, provider_price};

fn text(value: &Value) -> Option<String> {
    value.as_str().filter(|text| !text.trim().is_empty()).map(str::to_owned)
}

pub(crate) fn model_type(value: &Value) -> Option<String> {
    text(value)
}

pub(crate) fn route(value: &Value) -> Option<ProviderCatalogModelRoute> {
    let fields = value.as_object()?;
    let route = ProviderCatalogModelRoute {
        api_url: fields.get("api").and_then(text),
        npm_package: fields.get("npm").and_then(text),
        shape: fields.get("shape").and_then(text),
    };
    (route.api_url.is_some() || route.npm_package.is_some() || route.shape.is_some())
        .then_some(route)
}

/// `interleaved: {"field": "reasoning_content"}` names the field; `interleaved: true` does not.
pub(crate) fn reasoning_field(value: &Value) -> Option<String> {
    value.as_object()?.get("field").and_then(text)
}

fn price(value: &Value) -> Option<ProviderCatalogPrice> {
    let cost = serde_json::from_value::<RawCost>(value.clone()).ok()?;
    provider_price(cost).ok()
}

/// The `context` tiers of `cost.tiers`, by ascending length; one tier per length.
pub(crate) fn price_tiers(value: &Value) -> Vec<ProviderCatalogPriceTier> {
    let mut tiers: Vec<ProviderCatalogPriceTier> = value
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let tier = item.get("tier")?;
                    if tier.get("type")?.as_str()? != "context" {
                        return None;
                    }
                    let from_context_tokens =
                        tier.get("size")?.as_u64().filter(|size| *size > 0)?;
                    Some(ProviderCatalogPriceTier { from_context_tokens, price: price(item)? })
                })
                .collect()
        })
        .unwrap_or_default();
    tiers.sort_by_key(|tier| tier.from_context_tokens);
    tiers.dedup_by_key(|tier| tier.from_context_tokens);
    tiers
}

/// String values of `fields`, when every field is one of `allowed`.
fn listed(fields: Option<&Value>, allowed: &[&str]) -> Option<BTreeMap<String, String>> {
    let Some(fields) = fields else {
        return Some(BTreeMap::new());
    };
    fields
        .as_object()?
        .iter()
        .map(|(name, value)| {
            allowed.contains(&name.as_str()).then(|| Some((name.clone(), text(value)?))).flatten()
        })
        .collect()
}

fn mode(fields: &Map<String, Value>) -> Option<ProviderCatalogModelMode> {
    let provider = fields.get("provider")?.as_object()?;
    if provider.keys().any(|key| key != "body" && key != "headers") {
        return None;
    }
    let body = listed(provider.get("body"), &SPEED_MODE_BODY_FIELDS)?;
    let headers = listed(provider.get("headers"), &SPEED_MODE_HEADERS)?;
    if body.is_empty() && headers.is_empty() {
        return None;
    }
    let price = match fields.get("cost") {
        Some(cost) => Some(price(cost)?),
        None => None,
    };
    Some(ProviderCatalogModelMode { price, body, headers })
}

/// The modes of `experimental.modes` that change only listed body fields and headers.
pub(crate) fn modes(experimental: &Value) -> BTreeMap<String, ProviderCatalogModelMode> {
    experimental
        .get("modes")
        .and_then(Value::as_object)
        .map(|modes| {
            modes
                .iter()
                .filter(|(name, _)| !name.trim().is_empty())
                .filter_map(|(name, fields)| Some((name.clone(), mode(fields.as_object()?)?)))
                .collect()
        })
        .unwrap_or_default()
}
