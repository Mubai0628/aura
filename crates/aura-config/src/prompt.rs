use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ConfigModule, ConfigValidationError, ModuleConfigDocument};

pub const CUSTOM_SYSTEM_ADDENDUM_FIELD: &str = "custom_system_addendum";

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PromptSettingsSnapshot {
    pub revision: u64,
    pub custom_system_addendum: Option<String>,
}

pub(crate) fn validate_prompt_document(
    document: &ModuleConfigDocument,
) -> Result<(), ConfigValidationError> {
    let module = ConfigModule::Prompt;

    for key in document.values.keys() {
        if key != CUSTOM_SYSTEM_ADDENDUM_FIELD {
            return Err(ConfigValidationError::UnknownModuleField { module, field: key.clone() });
        }
    }

    let Some(value) = document.values.get(CUSTOM_SYSTEM_ADDENDUM_FIELD) else {
        return Ok(());
    };
    let Value::String(value) = value else {
        return Err(ConfigValidationError::InvalidModuleFieldType {
            module,
            field: CUSTOM_SYSTEM_ADDENDUM_FIELD.to_owned(),
            expected: "string",
        });
    };
    validate_addendum(value)
}

pub(crate) fn prompt_snapshot_from_document(
    document: &ModuleConfigDocument,
) -> Result<PromptSettingsSnapshot, ConfigValidationError> {
    validate_prompt_document(document)?;
    let custom_system_addendum = document
        .values
        .get(CUSTOM_SYSTEM_ADDENDUM_FIELD)
        .and_then(Value::as_str)
        .map(normalize_line_endings)
        .filter(|value| !value.chars().all(char::is_whitespace));
    Ok(PromptSettingsSnapshot { revision: document.revision, custom_system_addendum })
}

fn validate_addendum(value: &str) -> Result<(), ConfigValidationError> {
    for character in value.chars() {
        let allowed_control = matches!(character, '\n' | '\r' | '\t');
        if (character <= '\u{001f}' && !allowed_control) || character == '\u{007f}' {
            return Err(ConfigValidationError::InvalidPromptControlCharacter {
                code_point: u32::from(character),
            });
        }
    }
    Ok(())
}

fn normalize_line_endings(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn document(value: Option<&str>) -> ModuleConfigDocument {
        let mut document = ModuleConfigDocument::empty();
        if let Some(value) = value {
            document.values.insert(CUSTOM_SYSTEM_ADDENDUM_FIELD.to_owned(), json!(value));
        }
        document
    }

    #[test]
    fn snapshot_normalizes_line_endings_without_trimming_non_empty_content() {
        let snapshot = prompt_snapshot_from_document(&document(Some("  alpha\r\nbeta\rgamma  ")))
            .expect("valid prompt settings");
        assert_eq!(snapshot.custom_system_addendum.as_deref(), Some("  alpha\nbeta\ngamma  "));
    }

    #[test]
    fn whitespace_only_addendum_is_absent() {
        let snapshot = prompt_snapshot_from_document(&document(Some(" \t\r\n")))
            .expect("valid prompt settings");
        assert_eq!(snapshot.custom_system_addendum, None);
    }

    #[test]
    fn rejects_forbidden_control_characters_but_allows_cr_lf_and_tab() {
        validate_addendum("a\r\n\tb").expect("allowed controls");
        for invalid in ['\0', '\u{0008}', '\u{001f}', '\u{007f}'] {
            assert!(validate_addendum(&format!("a{invalid}b")).is_err());
        }
    }
}
