use aura_domain_common::ContentHash;
use aura_domain_common::jcs::{CanonicalHashV1, canonicalize_json_value, parse_json_strict};
use aura_tool_contract::{
    CompiledToolSchemaV1, ToolResultContractV1, ToolResultErrorCode, ToolSchemaError,
    ToolSchemaValidationReportV1,
};
use serde_json::Value;
use thiserror::Error;

/// A result contract compiled from the exact ToolVersion before Runtime dispatch.
///
/// This value deliberately contains no side-effect or outcome decision. Runtime
/// bytes are only candidate result evidence; the ToolCall supervisor decides the
/// final outcome and aggregate side-effect state independently.
pub enum PreparedToolResultContractV1 {
    Unit,
    Utf8Text,
    StructuredJson { validator: CompiledToolSchemaV1, schema_hash: CanonicalHashV1 },
}

impl PreparedToolResultContractV1 {
    pub fn compile(contract: &ToolResultContractV1) -> Result<Self, ToolSchemaError> {
        match contract {
            ToolResultContractV1::Unit => Ok(Self::Unit),
            ToolResultContractV1::Utf8Text => Ok(Self::Utf8Text),
            ToolResultContractV1::StructuredJson { result_schema } => {
                let validator = CompiledToolSchemaV1::compile(result_schema.canonical_document())?;
                Ok(Self::StructuredJson {
                    validator,
                    schema_hash: result_schema.schema_hash().clone(),
                })
            }
        }
    }

    pub fn validate_candidate(
        &self,
        complete_body: &[u8],
    ) -> Result<ValidatedToolResultBodyV1, ToolResultBodyErrorV1> {
        match self {
            Self::Unit => Ok(ValidatedToolResultBodyV1::Unit),
            Self::Utf8Text => {
                let text = std::str::from_utf8(complete_body)
                    .map_err(|_| ToolResultBodyErrorV1::Utf8Invalid)?
                    .to_owned();
                Ok(ValidatedToolResultBodyV1::Utf8Text {
                    content_hash: ContentHash::sha256(complete_body),
                    text,
                })
            }
            Self::StructuredJson { validator, schema_hash } => {
                let value = parse_json_strict(complete_body)
                    .map_err(|_| ToolResultBodyErrorV1::JsonInvalid)?;
                let report = validator.validate(&value);
                if !report.valid {
                    return Err(ToolResultBodyErrorV1::SchemaValidationFailed { report });
                }
                let canonical_json = canonicalize_json_value(&value)
                    .map_err(|_| ToolResultBodyErrorV1::JsonInvalid)?;
                Ok(ValidatedToolResultBodyV1::StructuredJson {
                    content_hash: ContentHash::sha256(&canonical_json),
                    canonical_json,
                    schema_hash: schema_hash.clone(),
                    value,
                })
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidatedToolResultBodyV1 {
    Unit,
    Utf8Text {
        text: String,
        content_hash: ContentHash,
    },
    StructuredJson {
        value: Value,
        canonical_json: Vec<u8>,
        content_hash: ContentHash,
        schema_hash: CanonicalHashV1,
    },
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ToolResultBodyErrorV1 {
    #[error("tool result is not valid UTF-8")]
    Utf8Invalid,
    #[error("tool result is not exactly one valid canonicalizable JSON value")]
    JsonInvalid,
    #[error("tool result does not satisfy the frozen result schema")]
    SchemaValidationFailed { report: ToolSchemaValidationReportV1 },
}

impl ToolResultBodyErrorV1 {
    #[must_use]
    pub const fn error_code(&self) -> ToolResultErrorCode {
        match self {
            Self::Utf8Invalid => ToolResultErrorCode::Utf8Invalid,
            Self::JsonInvalid => ToolResultErrorCode::JsonInvalid,
            Self::SchemaValidationFailed { .. } => ToolResultErrorCode::SchemaValidationFailed,
        }
    }
}

#[cfg(test)]
mod tests {
    use aura_tool_contract::{ToolResultContractV1, ToolStructuredResultSchemaV1};
    use serde_json::json;

    use super::*;

    #[test]
    fn utf8_text_preserves_complete_bytes_and_rejects_invalid_utf8() {
        let contract = PreparedToolResultContractV1::compile(&ToolResultContractV1::Utf8Text)
            .expect("compile text contract");
        assert_eq!(
            contract.validate_candidate(b"line one\r\nline two").expect("valid text"),
            ValidatedToolResultBodyV1::Utf8Text {
                text: "line one\r\nline two".to_owned(),
                content_hash: ContentHash::sha256(b"line one\r\nline two"),
            }
        );
        let error = contract.validate_candidate(&[0xff]).expect_err("invalid UTF-8");
        assert_eq!(error.error_code(), ToolResultErrorCode::Utf8Invalid);
    }

    #[test]
    fn structured_json_canonicalizes_then_hashes_and_validates_exact_schema() {
        let result_schema = ToolStructuredResultSchemaV1::new(json!({
            "type": "object",
            "required": ["count", "name"],
            "additionalProperties": false,
            "properties": {
                "count": {"type": "integer"},
                "name": {"type": "string"}
            }
        }))
        .expect("result schema");
        let expected_schema_hash = result_schema.schema_hash().clone();
        let contract =
            PreparedToolResultContractV1::compile(&ToolResultContractV1::StructuredJson {
                result_schema,
            })
            .expect("compile structured contract");
        let validated = contract
            .validate_candidate(br#"{ "name": "Aura", "count": 2 }"#)
            .expect("valid structured result");
        assert_eq!(
            validated,
            ValidatedToolResultBodyV1::StructuredJson {
                value: json!({"count": 2, "name": "Aura"}),
                canonical_json: br#"{"count":2,"name":"Aura"}"#.to_vec(),
                content_hash: ContentHash::sha256(br#"{"count":2,"name":"Aura"}"#),
                schema_hash: expected_schema_hash,
            }
        );
    }

    #[test]
    fn structured_json_rejects_duplicates_extra_values_and_schema_mismatch() {
        let result_schema = ToolStructuredResultSchemaV1::new(json!({"type": "integer"}))
            .expect("integer result schema");
        let contract =
            PreparedToolResultContractV1::compile(&ToolResultContractV1::StructuredJson {
                result_schema,
            })
            .expect("compile structured contract");

        for bytes in [br#"{"value":1,"value":2}"#.as_slice(), b"1 2".as_slice()] {
            let error = contract.validate_candidate(bytes).expect_err("invalid JSON evidence");
            assert_eq!(error.error_code(), ToolResultErrorCode::JsonInvalid);
        }

        let error =
            contract.validate_candidate(br#""not an integer""#).expect_err("schema mismatch");
        assert_eq!(error.error_code(), ToolResultErrorCode::SchemaValidationFailed);
        let ToolResultBodyErrorV1::SchemaValidationFailed { report } = error else {
            panic!("expected schema validation report");
        };
        assert!(!report.valid);
        assert_eq!(report.total_error_count, 1);
    }

    #[test]
    fn unit_contract_never_creates_model_visible_result_body() {
        let contract = PreparedToolResultContractV1::compile(&ToolResultContractV1::Unit)
            .expect("compile unit contract");
        assert_eq!(
            contract.validate_candidate(&[]).expect("unit result"),
            ValidatedToolResultBodyV1::Unit
        );
    }
}
