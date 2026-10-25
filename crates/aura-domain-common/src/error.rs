use std::{collections::BTreeMap, fmt, str::FromStr};

use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_json::Value;
use thiserror::Error;

use crate::{CausationId, ContractVersion, CorrelationId};

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("error code must be 1-128 lowercase ASCII characters grouped by dots")]
pub struct ErrorCodeError;

#[derive(Clone, Debug, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd)]
pub struct ErrorCode(#[schemars(with = "String")] String);

impl ErrorCode {
    pub fn new(value: impl Into<String>) -> Result<Self, ErrorCodeError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 128
            && value.split('.').all(|segment| {
                !segment.is_empty()
                    && segment.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || matches!(byte, b'_' | b'-')
                    })
            });

        valid.then_some(Self(value)).ok_or(ErrorCodeError)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for ErrorCode {
    type Err = ErrorCodeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl Serialize for ErrorCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ErrorCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum NormalizedIssueCodeError {
    #[error("normalized issue code does not use ErrorCode wire syntax")]
    InvalidSyntax,
    #[error("normalized issue code is outside the v1 stable registry")]
    Unregistered,
}

/// A Renderer-safe, stable issue code from Aura's v1 normalized registry.
///
/// This is deliberately a distinct domain type. Raw provider, Win32, DNS, or
/// TLS diagnostics must not be converted into it or embedded in its suffix.
#[derive(Clone, Debug, Eq, Hash, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct NormalizedIssueCode(ErrorCode);

impl NormalizedIssueCode {
    const NAMESPACES: [&'static str; 10] = [
        "credential.",
        "configuration.",
        "network.",
        "tls.",
        "authentication.",
        "discovery.",
        "provider.",
        "transport.",
        "persistence.",
        "compatibility.",
    ];
    const EXACT_CODES: [&'static str; 8] = [
        "artifact.unavailable",
        "execution.isolation_reduced",
        "external.unclassified",
        "internal.unexpected",
        "tool.cleanup.incomplete",
        "tool.side_effect.unverified",
        "tool.outcome.unsettled",
        // The installed package changed (disable, revoke, uninstall or a new
        // version) while this call ran; it finished on the content it started with.
        "tool.package.changed_during_call",
    ];

    pub fn new(value: impl Into<String>) -> Result<Self, NormalizedIssueCodeError> {
        let code = ErrorCode::new(value).map_err(|_| NormalizedIssueCodeError::InvalidSyntax)?;
        let value = code.as_str();
        let registered = Self::EXACT_CODES.contains(&value)
            || Self::NAMESPACES.iter().any(|namespace| {
                value.strip_prefix(namespace).is_some_and(|suffix| !suffix.is_empty())
            });
        registered.then_some(Self(code)).ok_or(NormalizedIssueCodeError::Unregistered)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Display for NormalizedIssueCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for NormalizedIssueCode {
    type Err = NormalizedIssueCodeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl<'de> Deserialize<'de> for NormalizedIssueCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(D::Error::custom)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCategory {
    Validation,
    Compatibility,
    NotFound,
    Conflict,
    Permission,
    Unavailable,
    Cancelled,
    Transport,
    Protocol,
    Internal,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorEnvelope {
    pub contract_version: ContractVersion,
    pub code: ErrorCode,
    pub category: ErrorCategory,
    pub message: String,
    pub retryable: bool,
    pub correlation_id: CorrelationId,
    pub causation_id: Option<CausationId>,
    pub details: BTreeMap<String, Value>,
}

impl ErrorEnvelope {
    #[must_use]
    pub fn new(
        code: ErrorCode,
        category: ErrorCategory,
        message: impl Into<String>,
        retryable: bool,
        correlation_id: CorrelationId,
        causation_id: Option<CausationId>,
    ) -> Self {
        Self {
            contract_version: ContractVersion::CURRENT,
            code,
            category,
            message: message.into(),
            retryable,
            correlation_id,
            causation_id,
            details: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_detail(mut self, key: impl Into<String>, value: Value) -> Self {
        self.details.insert(key.into(), value);
        self
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use serde_json::json;

    use super::{NormalizedIssueCode, NormalizedIssueCodeError};

    #[test]
    fn normalized_issue_code_enforces_the_v1_registry() {
        for value in [
            "credential.binding_mismatch",
            "configuration.endpoint.invalid",
            "network.timeout",
            "tls.certificate.invalid",
            "authentication.rejected",
            "discovery.request_failed",
            "provider.rate_limited",
            "transport.cancelled",
            "persistence.reconciliation_required",
            "compatibility.output_token_field_alias_not_persisted",
            "artifact.unavailable",
            "execution.isolation_reduced",
            "external.unclassified",
            "internal.unexpected",
        ] {
            let code = NormalizedIssueCode::from_str(value).expect("registered code");
            assert_eq!(serde_json::to_value(&code).expect("wire"), json!(value));
            assert_eq!(
                serde_json::from_value::<NormalizedIssueCode>(json!(value))
                    .expect("wire round trip"),
                code
            );
        }

        for value in [
            "credential",
            "credential.",
            "configuration",
            "artifact.expired",
            "external.other",
            "unknown.failure",
            "Credential.failure",
            "network..failure",
        ] {
            assert!(NormalizedIssueCode::from_str(value).is_err(), "unexpectedly accepted {value}");
        }
        assert_eq!(
            NormalizedIssueCode::from_str("valid_but_unregistered"),
            Err(NormalizedIssueCodeError::Unregistered)
        );
        assert_eq!(
            NormalizedIssueCode::from_str(&format!("credential.{}", "a".repeat(128))),
            Err(NormalizedIssueCodeError::InvalidSyntax)
        );
    }
}
