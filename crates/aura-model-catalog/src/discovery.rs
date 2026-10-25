use std::collections::BTreeSet;

use aura_model_domain::ModelApiFormat;
use aura_network_control::ConfigurationTransportPolicyV1;
use serde_json::{Map, Value};
use url::Url;

use crate::CatalogError;

const MAX_PROVIDER_MODEL_ID_BYTES: usize = 512;
const ANTHROPIC_DEFAULT_DISCOVERY_LIMIT: u16 = 1_000;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModelDiscoveryPage {
    pub model_ids: Vec<String>,
    pub has_more: bool,
    pub last_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelDiscoveryPageAction {
    Continue { next_url: Url },
    Complete { model_ids: Vec<String> },
}

pub struct ModelDiscoveryAccumulator {
    api_format: ModelApiFormat,
    base_url: Url,
    pages: u32,
    aggregate_decompressed_bytes: u64,
    model_ids: Vec<String>,
    seen_model_ids: BTreeSet<String>,
    seen_cursors: BTreeSet<String>,
}

impl ModelDiscoveryAccumulator {
    pub fn new(api_format: ModelApiFormat, base_url: Url) -> Result<Self, CatalogError> {
        validate_initial_discovery_url(api_format, &base_url)?;
        Ok(Self {
            api_format,
            base_url,
            pages: 0,
            aggregate_decompressed_bytes: 0,
            model_ids: Vec::new(),
            seen_model_ids: BTreeSet::new(),
            seen_cursors: BTreeSet::new(),
        })
    }

    pub fn initial_url(&self) -> Result<Url, CatalogError> {
        discovery_page_url(self.api_format, &self.base_url, None)
    }

    #[must_use]
    pub fn collected_model_ids(&self) -> Vec<String> {
        self.model_ids.clone()
    }

    pub fn accept_page(
        &mut self,
        raw: &[u8],
        page: ModelDiscoveryPage,
    ) -> Result<ModelDiscoveryPageAction, CatalogError> {
        let limits = ConfigurationTransportPolicyV1::MODEL_DISCOVERY_OPERATION;
        self.pages = self.pages.checked_add(1).ok_or(CatalogError::DiscoveryLimitExceeded)?;
        self.aggregate_decompressed_bytes = self
            .aggregate_decompressed_bytes
            .checked_add(u64::try_from(raw.len()).unwrap_or(u64::MAX))
            .ok_or(CatalogError::DiscoveryLimitExceeded)?;
        if self.pages > u32::from(limits.max_pages)
            || self.aggregate_decompressed_bytes > limits.max_aggregate_decompressed_bytes
        {
            return Err(CatalogError::DiscoveryLimitExceeded);
        }
        let next_model_count = self
            .model_ids
            .len()
            .checked_add(page.model_ids.len())
            .ok_or(CatalogError::DiscoveryLimitExceeded)?;
        if next_model_count > usize::try_from(limits.max_models).unwrap_or(usize::MAX) {
            return Err(CatalogError::DiscoveryLimitExceeded);
        }
        for model_id in page.model_ids {
            if !self.seen_model_ids.insert(model_id.clone()) {
                return Err(CatalogError::DiscoveryInvalidResponse(
                    "provider model IDs must be unique across the complete discovery operation"
                        .to_owned(),
                ));
            }
            self.model_ids.push(model_id);
        }
        if !page.has_more {
            return Ok(ModelDiscoveryPageAction::Complete {
                model_ids: std::mem::take(&mut self.model_ids),
            });
        }
        if !matches!(self.api_format, ModelApiFormat::AnthropicMessages) {
            return Err(CatalogError::DiscoveryInvalidResponse(
                "OpenAI-compatible model discovery must not paginate".to_owned(),
            ));
        }
        let cursor = page.last_id.filter(|value| !value.is_empty()).ok_or_else(|| {
            CatalogError::DiscoveryInvalidResponse(
                "an Anthropic page with has_more=true requires a non-empty last_id".to_owned(),
            )
        })?;
        if !self.seen_cursors.insert(cursor.clone()) {
            return Err(CatalogError::DiscoveryInvalidResponse(
                "Anthropic discovery cursor did not advance".to_owned(),
            ));
        }
        if self.pages >= u32::from(limits.max_pages) {
            return Err(CatalogError::DiscoveryLimitExceeded);
        }
        Ok(ModelDiscoveryPageAction::Continue {
            next_url: discovery_page_url(self.api_format, &self.base_url, Some(&cursor))?,
        })
    }
}

pub fn parse_model_discovery_page(
    api_format: ModelApiFormat,
    raw: &[u8],
) -> Result<ModelDiscoveryPage, CatalogError> {
    let value: Value = serde_json::from_slice(raw)
        .map_err(|error| CatalogError::DiscoveryInvalidResponse(error.to_string()))?;
    let object = value.as_object().ok_or_else(|| {
        CatalogError::DiscoveryInvalidResponse(
            "discovery response root must be an object".to_owned(),
        )
    })?;
    match api_format {
        ModelApiFormat::OpenAiResponses | ModelApiFormat::OpenAiChatCompletions => {
            parse_openai_page(object)
        }
        ModelApiFormat::AnthropicMessages => parse_anthropic_page(object),
    }
}

pub fn discovery_page_url(
    api_format: ModelApiFormat,
    base_url: &Url,
    after_id: Option<&str>,
) -> Result<Url, CatalogError> {
    validate_initial_discovery_url(api_format, base_url)?;
    if !matches!(api_format, ModelApiFormat::AnthropicMessages) {
        if after_id.is_some() {
            return Err(CatalogError::DiscoveryInvalidResponse(
                "OpenAI-compatible model discovery does not support a page cursor".to_owned(),
            ));
        }
        return Ok(base_url.clone());
    }
    let mut pairs = base_url
        .query_pairs()
        .map(|(name, value)| (name.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    if !pairs.iter().any(|(name, _)| name == "limit") {
        pairs.push(("limit".to_owned(), ANTHROPIC_DEFAULT_DISCOVERY_LIMIT.to_string()));
    }
    if let Some(cursor) = after_id {
        if cursor.is_empty() {
            return Err(CatalogError::DiscoveryInvalidResponse(
                "Anthropic discovery cursor must not be empty".to_owned(),
            ));
        }
        pairs.push(("after_id".to_owned(), cursor.to_owned()));
    }
    let mut next = base_url.clone();
    next.set_query(None);
    if !pairs.is_empty() {
        let mut query = next.query_pairs_mut();
        query.clear();
        query.extend_pairs(pairs);
    }
    Ok(next)
}

fn parse_openai_page(object: &Map<String, Value>) -> Result<ModelDiscoveryPage, CatalogError> {
    if let Some(kind) = object.get("object")
        && kind.as_str() != Some("list")
    {
        return Err(CatalogError::DiscoveryInvalidResponse(
            "OpenAI discovery object must equal list when present".to_owned(),
        ));
    }
    let entries = required_array(object, "data")?;
    let model_ids = parse_model_entries(entries, |entry| {
        if let Some(kind) = entry.get("object")
            && kind.as_str() != Some("model")
        {
            return Err(CatalogError::DiscoveryInvalidResponse(
                "OpenAI discovery item object must equal model when present".to_owned(),
            ));
        }
        Ok(())
    })?;
    Ok(ModelDiscoveryPage { model_ids, has_more: false, last_id: None })
}

fn parse_anthropic_page(object: &Map<String, Value>) -> Result<ModelDiscoveryPage, CatalogError> {
    let entries = required_array(object, "data")?;
    let has_more = object.get("has_more").and_then(Value::as_bool).ok_or_else(|| {
        CatalogError::DiscoveryInvalidResponse(
            "Anthropic discovery has_more must be a boolean".to_owned(),
        )
    })?;
    let last_id = match object.get("last_id") {
        None | Some(Value::Null) => None,
        Some(Value::String(value)) => Some(value.clone()),
        Some(_) => {
            return Err(CatalogError::DiscoveryInvalidResponse(
                "Anthropic discovery last_id must be a string or null".to_owned(),
            ));
        }
    };
    if has_more && last_id.as_ref().is_none_or(String::is_empty) {
        return Err(CatalogError::DiscoveryInvalidResponse(
            "Anthropic discovery has_more=true requires a non-empty last_id".to_owned(),
        ));
    }
    let model_ids = parse_model_entries(entries, |_| Ok(()))?;
    Ok(ModelDiscoveryPage { model_ids, has_more, last_id })
}

fn required_array<'a>(
    object: &'a Map<String, Value>,
    name: &str,
) -> Result<&'a Vec<Value>, CatalogError> {
    object.get(name).and_then(Value::as_array).ok_or_else(|| {
        CatalogError::DiscoveryInvalidResponse(format!(
            "discovery response field {name} must be an array"
        ))
    })
}

fn parse_model_entries(
    entries: &[Value],
    validate_item: impl Fn(&Map<String, Value>) -> Result<(), CatalogError>,
) -> Result<Vec<String>, CatalogError> {
    let maximum = ConfigurationTransportPolicyV1::MODEL_DISCOVERY_OPERATION.max_models;
    if entries.len() > usize::try_from(maximum).unwrap_or(usize::MAX) {
        return Err(CatalogError::DiscoveryLimitExceeded);
    }
    let mut model_ids = Vec::with_capacity(entries.len());
    let mut seen = BTreeSet::new();
    for entry in entries {
        let object = entry.as_object().ok_or_else(|| {
            CatalogError::DiscoveryInvalidResponse(
                "discovery data entries must be objects".to_owned(),
            )
        })?;
        validate_item(object)?;
        let model_id = object.get("id").and_then(Value::as_str).ok_or_else(|| {
            CatalogError::DiscoveryInvalidResponse(
                "discovery model objects must contain a string id".to_owned(),
            )
        })?;
        validate_model_id(model_id)?;
        if !seen.insert(model_id.to_owned()) {
            return Err(CatalogError::DiscoveryInvalidResponse(
                "provider model IDs must be unique within a discovery page".to_owned(),
            ));
        }
        model_ids.push(model_id.to_owned());
    }
    Ok(model_ids)
}

fn validate_model_id(model_id: &str) -> Result<(), CatalogError> {
    if model_id.is_empty()
        || model_id.len() > MAX_PROVIDER_MODEL_ID_BYTES
        || model_id.chars().any(|character| character <= '\u{001f}' || character == '\u{007f}')
    {
        return Err(CatalogError::DiscoveryInvalidResponse(
            "provider model id violates the frozen length or control-character contract".to_owned(),
        ));
    }
    Ok(())
}

fn validate_initial_discovery_url(
    api_format: ModelApiFormat,
    url: &Url,
) -> Result<(), CatalogError> {
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(CatalogError::InvalidDiscoveryRequest);
    }
    if !matches!(api_format, ModelApiFormat::AnthropicMessages) {
        return Ok(());
    }
    let pairs = url.query_pairs().collect::<Vec<_>>();
    if pairs.iter().any(|(name, _)| matches!(name.as_ref(), "before_id" | "after_id")) {
        return Err(CatalogError::InvalidDiscoveryRequest);
    }
    let limits = pairs.iter().filter(|(name, _)| name == "limit").collect::<Vec<_>>();
    if limits.len() > 1 {
        return Err(CatalogError::InvalidDiscoveryRequest);
    }
    if let Some((_, value)) = limits.first()
        && value.parse::<u16>().ok().filter(|limit| (1..=1_000).contains(limit)).is_none()
    {
        return Err(CatalogError::InvalidDiscoveryRequest);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use aura_model_domain::ModelApiFormat;
    use url::Url;

    use super::{
        ModelDiscoveryAccumulator, ModelDiscoveryPageAction, discovery_page_url,
        parse_model_discovery_page,
    };

    #[test]
    fn openai_discovery_accepts_only_the_frozen_list_shape() {
        let page = parse_model_discovery_page(
            ModelApiFormat::OpenAiResponses,
            br#"{"object":"list","data":[{"id":"Model-A","object":"model","extra":1}]}"#,
        )
        .expect("frozen OpenAI page");
        assert_eq!(page.model_ids, ["Model-A"]);
        for invalid in [
            br#"["model"]"#.as_slice(),
            br#"{"models":[{"id":"model"}]}"#,
            br#"{"data":["model"]}"#,
            br#"{"object":"models","data":[]}"#,
            br#"{"data":[{"id":"same"},{"id":"same"}]}"#,
            b"{\"data\":[{\"id\":\"bad\\u0001id\"}]}".as_slice(),
        ] {
            assert!(
                parse_model_discovery_page(ModelApiFormat::OpenAiChatCompletions, invalid).is_err()
            );
        }
    }

    #[test]
    fn anthropic_pagination_preserves_query_and_rejects_cursor_loops() {
        let base = Url::parse("https://api.example.test/v1/models?region=us").expect("base URL");
        let mut accumulator =
            ModelDiscoveryAccumulator::new(ModelApiFormat::AnthropicMessages, base)
                .expect("pagination");
        assert_eq!(
            accumulator.initial_url().expect("initial URL").as_str(),
            "https://api.example.test/v1/models?region=us&limit=1000"
        );
        let first_raw = br#"{"data":[{"id":"a"}],"has_more":true,"last_id":"a"}"#;
        let first = parse_model_discovery_page(ModelApiFormat::AnthropicMessages, first_raw)
            .expect("first page");
        let ModelDiscoveryPageAction::Continue { next_url } =
            accumulator.accept_page(first_raw, first).expect("continue")
        else {
            panic!("first page must continue")
        };
        assert_eq!(
            next_url.as_str(),
            "https://api.example.test/v1/models?region=us&limit=1000&after_id=a"
        );
        let loop_raw = br#"{"data":[{"id":"b"}],"has_more":true,"last_id":"a"}"#;
        let loop_page = parse_model_discovery_page(ModelApiFormat::AnthropicMessages, loop_raw)
            .expect("loop page shape");
        assert!(accumulator.accept_page(loop_raw, loop_page).is_err());
    }

    #[test]
    fn anthropic_custom_query_rejects_user_cursor_and_invalid_limit() {
        for url in [
            "https://api.example.test/models?after_id=x",
            "https://api.example.test/models?before_id=x",
            "https://api.example.test/models?limit=0",
            "https://api.example.test/models?limit=1&limit=2",
        ] {
            let parsed = Url::parse(url).expect("URL");
            assert!(discovery_page_url(ModelApiFormat::AnthropicMessages, &parsed, None).is_err());
        }
    }
}
