use std::collections::{BTreeMap, BTreeSet};

use aura_model_domain::{
    CatalogDecimal, CatalogPriceCurrency, CatalogPriceUnit, ProviderCatalogEntry,
    ProviderCatalogModelCapabilities, ProviderCatalogModelEntry, ProviderCatalogPrice,
};
use serde::Deserialize;
use serde_json::{Number, Value};

use crate::CatalogError;

#[derive(Deserialize)]
struct RawProvider {
    name: String,
    #[serde(default)]
    npm: Option<String>,
    #[serde(default)]
    api: Option<String>,
    #[serde(default)]
    doc: Option<String>,
    #[serde(default)]
    models: BTreeMap<String, RawModel>,
}

#[derive(Deserialize)]
struct RawModel {
    name: String,
    #[serde(default)]
    family: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    aliases: BTreeSet<String>,
    #[serde(default)]
    limit: RawLimit,
    #[serde(default)]
    attachment: bool,
    #[serde(default)]
    reasoning: bool,
    #[serde(default)]
    reasoning_options: Value,
    #[serde(default)]
    tool_call: bool,
    #[serde(default)]
    interleaved: Value,
    #[serde(default)]
    structured_output: bool,
    #[serde(default)]
    temperature: bool,
    #[serde(default)]
    knowledge: Option<String>,
    #[serde(default)]
    release_date: Option<String>,
    #[serde(default)]
    last_updated: Option<String>,
    #[serde(default)]
    modalities: RawModalities,
    #[serde(default)]
    open_weights: Option<bool>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    cost: Option<RawCost>,
}

#[derive(Default, Deserialize)]
struct RawLimit {
    #[serde(default)]
    context: Option<u64>,
    #[serde(default)]
    input: Option<u64>,
    #[serde(default)]
    output: Option<u64>,
}

#[derive(Default, Deserialize)]
struct RawModalities {
    #[serde(default)]
    input: BTreeSet<String>,
    #[serde(default)]
    output: BTreeSet<String>,
}

#[derive(Deserialize)]
struct RawCost {
    #[serde(default)]
    input: Option<Number>,
    #[serde(default)]
    output: Option<Number>,
    #[serde(default)]
    cache_read: Option<Number>,
    #[serde(default)]
    cache_write: Option<Number>,
}

pub fn parse_models_dev_catalog(
    bytes: &[u8],
) -> Result<BTreeMap<String, ProviderCatalogEntry>, CatalogError> {
    let raw = serde_json::from_slice::<BTreeMap<String, RawProvider>>(bytes)?;
    if raw.is_empty() {
        return Err(CatalogError::Invalid("provider map is empty".to_owned()));
    }
    raw.into_iter()
        .map(|(provider_id, provider)| {
            validate_nonblank("provider ID", &provider_id)?;
            validate_nonblank("provider name", &provider.name)?;
            let models = provider
                .models
                .into_iter()
                .map(|(model_id, model)| {
                    validate_nonblank("model ID", &model_id)?;
                    validate_nonblank("model name", &model.name)?;
                    if model.aliases.iter().any(|alias| alias.trim().is_empty()) {
                        return Err(CatalogError::Invalid(format!(
                            "model {provider_id}/{model_id} contains a blank alias"
                        )));
                    }
                    let reasoning_options = reasoning_options(&model.reasoning_options)?;
                    let price = model.cost.map(provider_price).transpose()?;
                    Ok((
                        model_id.clone(),
                        ProviderCatalogModelEntry {
                            catalog_model_id: model_id,
                            display_name: model.name,
                            family: model.family,
                            description: model.description,
                            aliases: model.aliases,
                            context_window_tokens: usable_token_limit(model.limit.context),
                            input_limit_tokens: usable_token_limit(model.limit.input),
                            output_limit_tokens: usable_token_limit(model.limit.output),
                            input_modalities: model.modalities.input,
                            output_modalities: model.modalities.output,
                            capabilities: ProviderCatalogModelCapabilities {
                                attachment: model.attachment,
                                reasoning: model.reasoning,
                                tool_call: model.tool_call,
                                structured_output: model.structured_output,
                                temperature: model.temperature,
                                interleaved: interleaved_capability(&model.interleaved),
                            },
                            reasoning_options,
                            knowledge_cutoff: model.knowledge,
                            release_date: model.release_date,
                            last_updated: model.last_updated,
                            open_weights: model.open_weights,
                            status: model.status,
                            price,
                        },
                    ))
                })
                .collect::<Result<BTreeMap<_, _>, CatalogError>>()?;
            let model_order = models.keys().cloned().collect();
            Ok((
                provider_id.clone(),
                ProviderCatalogEntry {
                    catalog_provider_id: provider_id,
                    display_name: provider.name,
                    npm_package: provider.npm,
                    api_url: provider.api,
                    documentation_url: provider.doc,
                    model_order,
                    models,
                },
            ))
        })
        .collect()
}

fn usable_token_limit(value: Option<u64>) -> Option<u64> {
    // models.dev uses zero for model families whose token limit is not
    // applicable or not published (for example image and speech models).
    // Aura's executable Profile contract requires non-zero limits, so keep the
    // Catalog entry but represent the unusable sentinel as absent. A Profile
    // created from this entry therefore remains non-executable until the user
    // supplies the required limit instead of poisoning the entire Catalog.
    value.filter(|value| *value > 0)
}

fn reasoning_options(value: &Value) -> Result<BTreeSet<String>, CatalogError> {
    match value {
        Value::Null => Ok(BTreeSet::new()),
        Value::Array(values) => values
            .iter()
            .map(|value| {
                serde_json::to_string(value)
                    .map_err(|error| CatalogError::Invalid(error.to_string()))
            })
            .collect(),
        _ => Err(CatalogError::Invalid(
            "model reasoning_options must be an array when present".to_owned(),
        )),
    }
}

fn interleaved_capability(value: &Value) -> Option<bool> {
    match value {
        Value::Null => None,
        Value::Bool(value) => Some(*value),
        _ => Some(true),
    }
}

fn provider_price(cost: RawCost) -> Result<ProviderCatalogPrice, CatalogError> {
    Ok(ProviderCatalogPrice {
        currency: CatalogPriceCurrency::Usd,
        unit: CatalogPriceUnit::PerMillionTokens,
        input: decimal(cost.input)?,
        output: decimal(cost.output)?,
        cache_read: decimal(cost.cache_read)?,
        cache_write: decimal(cost.cache_write)?,
    })
}

fn decimal(value: Option<Number>) -> Result<Option<CatalogDecimal>, CatalogError> {
    value
        .map(|value| {
            canonical_decimal(&value.to_string())?.parse().map_err(|_| {
                CatalogError::Invalid("catalog price is not a canonical decimal".to_owned())
            })
        })
        .transpose()
}

fn canonical_decimal(value: &str) -> Result<String, CatalogError> {
    if value.starts_with('-') {
        return Err(CatalogError::Invalid("catalog price must be non-negative".to_owned()));
    }
    let (mantissa, exponent) =
        value.split_once(['e', 'E']).map_or(Ok((value, 0_i32)), |(mantissa, exponent)| {
            exponent
                .parse::<i32>()
                .map(|exponent| (mantissa, exponent))
                .map_err(|_| CatalogError::Invalid("catalog price exponent is invalid".to_owned()))
        })?;
    let (whole, fraction) = mantissa.split_once('.').map_or((mantissa, ""), |parts| parts);
    if whole.is_empty() || !whole.bytes().chain(fraction.bytes()).all(|byte| byte.is_ascii_digit())
    {
        return Err(CatalogError::Invalid("catalog price is invalid".to_owned()));
    }
    let mut digits = format!("{whole}{fraction}");
    let decimal_position = i64::try_from(whole.len())
        .map_err(|_| CatalogError::Invalid("catalog price is too long".to_owned()))?
        + i64::from(exponent);
    let expanded = if decimal_position <= 0 {
        let zeroes = usize::try_from(-decimal_position)
            .map_err(|_| CatalogError::Invalid("catalog price is too long".to_owned()))?;
        format!("0.{}{digits}", "0".repeat(zeroes))
    } else {
        let decimal_position = usize::try_from(decimal_position)
            .map_err(|_| CatalogError::Invalid("catalog price is too long".to_owned()))?;
        if decimal_position >= digits.len() {
            digits.push_str(&"0".repeat(decimal_position - digits.len()));
            digits
        } else {
            digits.insert(decimal_position, '.');
            digits
        }
    };
    let (whole, fraction) = expanded.split_once('.').map_or((expanded.as_str(), ""), |parts| parts);
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    let fraction = fraction.trim_end_matches('0');
    Ok(if fraction.is_empty() { whole.to_owned() } else { format!("{whole}.{fraction}") })
}

fn validate_nonblank(label: &str, value: &str) -> Result<(), CatalogError> {
    if value.trim().is_empty() {
        Err(CatalogError::Invalid(format!("{label} must not be blank")))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical_decimal, parse_models_dev_catalog};

    #[test]
    fn parses_the_official_provider_map_shape_and_ignores_future_fields() {
        let providers = parse_models_dev_catalog(
            br#"{
              "openai": {
                "name": "OpenAI",
                "npm": "@ai-sdk/openai",
                "doc": "https://example.test/docs",
                "future_provider_field": true,
                "models": {
                  "gpt-example": {
                    "name": "GPT Example",
                    "description": "Example",
                    "attachment": true,
                    "reasoning": true,
                    "reasoning_options": [{"type":"effort","values":["high"]}],
                    "tool_call": true,
                    "structured_output": true,
                    "temperature": true,
                    "interleaved": {"field":"reasoning_content"},
                    "modalities": {"input":["text","image"],"output":["text"]},
                    "cost": {"input": 2.5, "output": 1e-7},
                    "future_model_field": {"ok": true},
                    "limit": {"context": 400000, "input": 272000, "output": 128000}
                  }
                }
              }
            }"#,
        )
        .expect("official shape");
        let model = &providers["openai"].models["gpt-example"];
        assert_eq!(model.context_window_tokens, Some(400_000));
        assert_eq!(model.input_limit_tokens, Some(272_000));
        assert!(model.capabilities.tool_call);
        assert_eq!(model.capabilities.interleaved, Some(true));
        let output_price = model
            .price
            .as_ref()
            .and_then(|price| price.output.as_ref())
            .expect("the fixture output price must be parsed");
        assert_eq!(output_price.as_str(), "0.0000001");
    }

    #[test]
    fn rejects_empty_catalogs() {
        assert!(parse_models_dev_catalog(br#"{}"#).is_err());
    }

    #[test]
    fn treats_zero_remote_limits_as_missing_without_dropping_the_model() {
        let providers = parse_models_dev_catalog(
            br#"{
              "provider": {
                "name": "Provider",
                "models": {
                  "image-model": {
                    "name": "Image Model",
                    "limit": {"context": 0, "input": 0, "output": 0}
                  },
                  "text-model": {
                    "name": "Text Model",
                    "limit": {"context": 128000, "output": 8192}
                  }
                }
              }
            }"#,
        )
        .expect("zero is a remote missing-value sentinel, not a Catalog-wide failure");

        let image_model = &providers["provider"].models["image-model"];
        assert_eq!(image_model.context_window_tokens, None);
        assert_eq!(image_model.input_limit_tokens, None);
        assert_eq!(image_model.output_limit_tokens, None);

        let text_model = &providers["provider"].models["text-model"];
        assert_eq!(text_model.context_window_tokens, Some(128_000));
        assert_eq!(text_model.output_limit_tokens, Some(8_192));
    }

    #[test]
    fn canonicalizes_json_number_notation_without_float_rounding() {
        assert_eq!(canonical_decimal("1.25").expect("plain decimal must be canonicalized"), "1.25");
        assert_eq!(
            canonical_decimal("1e-7").expect("negative exponent must be canonicalized"),
            "0.0000001"
        );
        assert_eq!(
            canonical_decimal("1.20e2").expect("positive exponent must be canonicalized"),
            "120"
        );
        assert!(canonical_decimal("-1").is_err());
    }
}
