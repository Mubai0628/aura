use anyhow::{Result, ensure};
use aura_model_domain::ModelRequestDefaults;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApiFormat {
    #[default]
    OpenAiChatCompletions,
    OpenAiResponses,
}
impl ApiFormat {
    pub fn suffix(self) -> &'static str {
        match self {
            Self::OpenAiChatCompletions => "/chat/completions",
            Self::OpenAiResponses => "/responses",
        }
    }
    pub fn output_field(self) -> &'static str {
        match self {
            Self::OpenAiChatCompletions => "max_tokens",
            Self::OpenAiResponses => "max_output_tokens",
        }
    }
}

/// Non-secret request settings mirrored into the isolated Core endpoint/profile.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RequestSettings {
    pub api_format: ApiFormat,
    pub request_defaults: ModelRequestDefaults,
    pub context_window_tokens: u64,
    pub historical_snapshot_max_tokens: u64,
    pub chat_output_token_field: Option<String>,
    pub chat_instruction_role: Option<String>,
    pub openai_organization_id: Option<String>,
    pub openai_project_id: Option<String>,
    pub max_parallel_model_requests: u32,
}
impl Default for RequestSettings {
    fn default() -> Self {
        Self {
            api_format: ApiFormat::OpenAiChatCompletions,
            request_defaults: ModelRequestDefaults {
                max_output_tokens: Some(8192),
                ..Default::default()
            },
            context_window_tokens: 256000,
            historical_snapshot_max_tokens: 1024,
            chat_output_token_field: None,
            chat_instruction_role: None,
            openai_organization_id: None,
            openai_project_id: None,
            max_parallel_model_requests: 1,
        }
    }
}
impl RequestSettings {
    pub fn read(profile: &Value, endpoint: &Value) -> Result<Self> {
        let request_defaults = serde_json::from_value(profile["request_defaults"].clone())?;
        let api_format = serde_json::from_value(endpoint["api_format"].clone())?;
        // Learned compatibility transforms cannot be silently dropped in a new endpoint.
        ensure!(
            endpoint["compatibility"].is_null(),
            "eval.owner_compatibility_requires_explicit_mirroring"
        );
        let settings = Self {
            api_format,
            request_defaults,
            context_window_tokens: profile
                .pointer("/context/runtime_effective_window_tokens")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow::anyhow!("eval.owner_context_missing"))?,
            historical_snapshot_max_tokens: profile
                .pointer("/context/historical_snapshot_max_tokens")
                .and_then(Value::as_u64)
                .ok_or_else(|| anyhow::anyhow!("eval.owner_history_limit_missing"))?,
            chat_output_token_field: optional_string(endpoint, "chat_output_token_field")?,
            chat_instruction_role: optional_string(endpoint, "chat_instruction_role")?,
            openai_organization_id: optional_string(endpoint, "openai_organization_id")?,
            openai_project_id: optional_string(endpoint, "openai_project_id")?,
            max_parallel_model_requests: u32::try_from(
                endpoint["max_parallel_model_requests"].as_u64().unwrap_or(1),
            )?,
        };
        ensure!(
            settings.context_window_tokens > settings.output_cap()?
                && settings.historical_snapshot_max_tokens > 0
                && settings.max_parallel_model_requests > 0,
            "eval.owner_settings_invalid"
        );
        Ok(settings)
    }
    pub fn output_cap(&self) -> Result<u64> {
        self.request_defaults
            .max_output_tokens
            .filter(|v| *v > 0)
            .ok_or_else(|| anyhow::anyhow!("eval.owner_output_limit_missing"))
    }
}
fn optional_string(value: &Value, key: &str) -> Result<Option<String>> {
    match value.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(v)) => Ok(Some(v.clone())),
        _ => anyhow::bail!("eval.owner_optional_setting_invalid"),
    }
}
