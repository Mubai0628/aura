use super::RequestSettings;
use anyhow::{Result, ensure};
use aura_domain_common::CredentialRef;
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct OwnerSource {
    pub models_path: PathBuf,
    pub database_path: PathBuf,
    pub model_profile_id: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OwnerTarget {
    pub source: OwnerSource,
    pub endpoint_id: String,
    pub model: String,
    pub source_api_format: String,
    pub endpoint_base_url: String,
    pub request_url: String,
    pub credential_ref: CredentialRef,
    pub settings: RequestSettings,
}

impl OwnerSource {
    /// Reads configuration and reference metadata only, without opening a Store or WinCred.
    pub fn read(&self) -> Result<OwnerTarget> {
        let config: Value = serde_json::from_slice(&std::fs::read(&self.models_path)?)?;
        let profile = config["models"]
            .get(&self.model_profile_id)
            .ok_or_else(|| anyhow::anyhow!("eval.owner_model_missing"))?;
        ensure!(profile["enabled"] == true, "eval.owner_model_disabled");
        let model = field(profile, "provider_model_name")?;
        ensure!(
            model.to_ascii_lowercase().starts_with("deepseek"),
            "eval.owner_model_not_deepseek"
        );
        let endpoint_id = field(profile, "endpoint_id")?;
        let endpoint = config["endpoints"]
            .get(endpoint_id)
            .ok_or_else(|| anyhow::anyhow!("eval.owner_endpoint_missing"))?;
        ensure!(
            endpoint["auth_scheme"] == "open_ai_bearer_api_key",
            "eval.owner_auth_scheme_unsupported"
        );
        let base = field(endpoint, "base_url")?;
        let url = reqwest::Url::parse(base)?;
        ensure!(
            url.scheme() == "https"
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "eval.owner_endpoint_unsafe"
        );
        let db =
            Connection::open_with_flags(&self.database_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let credential:Option<String>=db.query_row(
            "SELECT credential_ref FROM credential_records WHERE endpoint_id=?1 AND credential_slot='api-key' AND state='configured'",
            [endpoint_id],|r|r.get(0),
        ).optional()?;
        let credential_ref = credential
            .ok_or_else(|| anyhow::anyhow!("eval.owner_credential_not_configured"))?
            .parse()?;
        let settings = RequestSettings::read(profile, endpoint)?;
        Ok(OwnerTarget {
            source: self.clone(),
            endpoint_id: endpoint_id.into(),
            model: model.into(),
            source_api_format: field(endpoint, "api_format")?.into(),
            endpoint_base_url: base.into(),
            request_url: format!("{}{}", base.trim_end_matches('/'), settings.api_format.suffix()),
            credential_ref,
            settings,
        })
    }
}
impl OwnerTarget {
    pub fn verify_current(&self) -> Result<()> {
        ensure!(self.source.read()? == *self, "eval.owner_target_changed_reestimate_required");
        Ok(())
    }
}
fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| anyhow::anyhow!("eval.owner_metadata_incomplete"))
}
