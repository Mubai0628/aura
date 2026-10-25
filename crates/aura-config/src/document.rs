use std::collections::BTreeMap;

use aura_domain_common::CredentialRef;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{ConfigFormat, ConfigModule};

pub const CONFIG_SCHEMA_VERSION: u32 = 1;
const SECRET_KEYS: &[&str] = &[
    "access_token",
    "api_key",
    "api_secret",
    "apikey",
    "authorization",
    "bearer_token",
    "client_secret",
    "cookie",
    "credential_blob",
    "credential_value",
    "id_token",
    "oauth_refresh_token",
    "oauth_token",
    "password",
    "private_key",
    "refresh_token",
    "secret",
    "secret_key",
    "session_token",
    "set_cookie",
    "token",
];

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct ModuleConfigDocument {
    pub schema_version: u32,
    pub revision: u64,
    #[serde(flatten)]
    pub values: BTreeMap<String, Value>,
}

impl ModuleConfigDocument {
    #[must_use]
    pub const fn empty() -> Self {
        Self { schema_version: CONFIG_SCHEMA_VERSION, revision: 1, values: BTreeMap::new() }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ConfigValidationError {
    #[error("unsupported config schema version {actual}; expected {expected}")]
    UnsupportedSchema { actual: u32, expected: u32 },
    #[error("config revision must be at least 1")]
    InvalidRevision,
    #[error("secret-bearing field is forbidden in config: {path}")]
    SecretField { path: String },
    #[error("invalid CredentialRef at {path}: {message}")]
    InvalidCredentialRef { path: String, message: String },
    #[error("module semantic validation failed: {0}")]
    Semantic(String),
    #[error("unknown field {field} in {module:?} configuration")]
    UnknownModuleField { module: ConfigModule, field: String },
    #[error("field {field} in {module:?} configuration must be a {expected}")]
    InvalidModuleFieldType { module: ConfigModule, field: String, expected: &'static str },
    #[error("custom system addendum contains forbidden control character U+{code_point:04X}")]
    InvalidPromptControlCharacter { code_point: u32 },
}

pub trait ModuleConfigValidator: Send + Sync {
    fn validate(
        &self,
        module: ConfigModule,
        document: &ModuleConfigDocument,
    ) -> Result<(), ConfigValidationError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct BaseModuleConfigValidator;

impl ModuleConfigValidator for BaseModuleConfigValidator {
    fn validate(
        &self,
        _module: ConfigModule,
        document: &ModuleConfigDocument,
    ) -> Result<(), ConfigValidationError> {
        validate_base_document(document)
    }
}

pub fn validate_base_document(
    document: &ModuleConfigDocument,
) -> Result<(), ConfigValidationError> {
    if document.schema_version != CONFIG_SCHEMA_VERSION {
        return Err(ConfigValidationError::UnsupportedSchema {
            actual: document.schema_version,
            expected: CONFIG_SCHEMA_VERSION,
        });
    }
    if document.revision == 0 {
        return Err(ConfigValidationError::InvalidRevision);
    }
    for (key, value) in &document.values {
        validate_named_value(key, value, key)?;
    }
    Ok(())
}

pub fn parse_document(
    module: ConfigModule,
    bytes: &[u8],
) -> Result<ModuleConfigDocument, DocumentCodecError> {
    let document = match module.format() {
        ConfigFormat::Json => serde_json::from_slice(bytes).map_err(DocumentCodecError::Json)?,
        ConfigFormat::Toml => {
            let text = std::str::from_utf8(bytes).map_err(DocumentCodecError::Utf8)?;
            let normalized;
            let text = if module == ConfigModule::Prompt && text.contains('\r') {
                normalized = text.replace("\r\n", "\n").replace('\r', "\n");
                normalized.as_str()
            } else {
                text
            };
            toml::from_str(text).map_err(DocumentCodecError::TomlDecode)?
        }
    };
    Ok(document)
}

pub fn encode_document(
    module: ConfigModule,
    document: &ModuleConfigDocument,
) -> Result<Vec<u8>, DocumentCodecError> {
    match module.format() {
        ConfigFormat::Json => {
            let mut bytes =
                serde_json::to_vec_pretty(document).map_err(DocumentCodecError::JsonEncode)?;
            bytes.push(b'\n');
            Ok(bytes)
        }
        ConfigFormat::Toml => {
            let mut text =
                toml::to_string_pretty(document).map_err(DocumentCodecError::TomlEncode)?;
            if !text.ends_with('\n') {
                text.push('\n');
            }
            Ok(text.into_bytes())
        }
    }
}

#[derive(Debug, Error)]
pub enum DocumentCodecError {
    #[error("config is not valid UTF-8: {0}")]
    Utf8(#[source] std::str::Utf8Error),
    #[error("config JSON is invalid: {0}")]
    Json(#[source] serde_json::Error),
    #[error("failed to encode config JSON: {0}")]
    JsonEncode(#[source] serde_json::Error),
    #[error("config TOML is invalid: {0}")]
    TomlDecode(#[source] toml::de::Error),
    #[error("failed to encode config TOML: {0}")]
    TomlEncode(#[source] toml::ser::Error),
}

fn validate_value(value: &Value, path: &str) -> Result<(), ConfigValidationError> {
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                let child_path = format!("{path}.{key}");
                validate_named_value(key, child, &child_path)?;
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                validate_value(child, &format!("{path}[{index}]"))?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_named_value(key: &str, value: &Value, path: &str) -> Result<(), ConfigValidationError> {
    let normalized = normalize_key(key);
    if is_secret_key(&normalized) {
        return Err(ConfigValidationError::SecretField { path: path.to_owned() });
    }
    if normalized == "credential_ref" || normalized.ends_with("_credential_ref") {
        if value.is_null() {
            return Ok(());
        }
        let reference =
            value.as_str().ok_or_else(|| ConfigValidationError::InvalidCredentialRef {
                path: path.to_owned(),
                message: "CredentialRef must be a string".to_owned(),
            })?;
        reference.parse::<CredentialRef>().map_err(|error| {
            ConfigValidationError::InvalidCredentialRef {
                path: path.to_owned(),
                message: error.to_string(),
            }
        })?;
        Ok(())
    } else {
        validate_value(value, path)
    }
}

fn is_secret_key(key: &str) -> bool {
    SECRET_KEYS.contains(&key)
}

pub(crate) fn document_contains_secret_fields(document: &ModuleConfigDocument) -> bool {
    document.values.iter().any(|(key, value)| named_value_contains_secret(key, value))
}

pub(crate) fn contains_secret_field_marker(bytes: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return true;
    };
    text.lines().any(line_contains_secret_field_marker)
}

fn named_value_contains_secret(key: &str, value: &Value) -> bool {
    if is_secret_key(&normalize_key(key)) {
        return true;
    }
    match value {
        Value::Object(object) => {
            object.iter().any(|(child_key, child)| named_value_contains_secret(child_key, child))
        }
        Value::Array(values) => values.iter().any(|child| named_value_contains_secret("", child)),
        _ => false,
    }
}

fn line_contains_secret_field_marker(line: &str) -> bool {
    for quote in ['"', '\''] {
        let mut remainder = line;
        while let Some(start) = remainder.find(quote) {
            let after_start = &remainder[start + quote.len_utf8()..];
            let Some(end) = after_start.find(quote) else {
                break;
            };
            let key = &after_start[..end];
            let after_key = after_start[end + quote.len_utf8()..].trim_start();
            if matches!(after_key.as_bytes().first(), Some(b':' | b'='))
                && is_secret_key(&normalize_key(key))
            {
                return true;
            }
            remainder = &after_start[end + quote.len_utf8()..];
        }
    }
    for delimiter in [':', '='] {
        if let Some((left, _)) = line.split_once(delimiter) {
            let key = left.rsplit('.').next().unwrap_or(left);
            if is_secret_key(&normalize_key(key)) {
                return true;
            }
        }
    }
    false
}

fn normalize_key(value: &str) -> String {
    let mut normalized = String::with_capacity(value.len());
    let mut separator = false;
    for character in value.chars() {
        if character.is_ascii_alphanumeric() {
            normalized.push(character.to_ascii_lowercase());
            separator = false;
        } else if !separator && !normalized.is_empty() {
            normalized.push('_');
            separator = true;
        }
    }
    while normalized.ends_with('_') {
        normalized.pop();
    }
    normalized
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{ModuleConfigDocument, contains_secret_field_marker, validate_base_document};

    #[test]
    fn rejects_secrets_but_accepts_credential_references() {
        let mut document = ModuleConfigDocument::empty();
        document.values.insert(
            "endpoint".to_owned(),
            json!({ "credential_ref": "wincred://aura/model/main" }),
        );
        validate_base_document(&document).expect("CredentialRef is safe");
        document.values.insert("optional".to_owned(), json!({ "credential_ref": null }));
        validate_base_document(&document).expect("an absent optional CredentialRef is safe");
        document.values.insert("unsafe".to_owned(), json!({ "api_key": "do-not-persist" }));
        assert!(validate_base_document(&document).is_err());
        document.values.remove("unsafe");
        document
            .values
            .insert("still-unsafe".to_owned(), json!({ "Client.Secret": "do-not-persist" }));
        assert!(validate_base_document(&document).is_err());
        assert!(contains_secret_field_marker(
            br#"{"schema_version":1,"revision":2,"api_key":"secret"}"#
        ));
        assert!(!contains_secret_field_marker(
            br#"{"schema_version":1,"revision":2,"max_tokens":128}"#
        ));
    }
}
