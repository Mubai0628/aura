use aura_domain_common::Revision;
use aura_tool_contract::{
    DEFAULT_MAX_CONCURRENT_TOOL_CALLS, DEFAULT_MAX_DYNAMIC_TOOL_SCHEMAS, DEFAULT_MAX_TOOL_RETRIES,
    DEFAULT_TOOL_TIMEOUT_MS, ToolSettingsSnapshotV1,
};
use serde_json::Value;

use crate::{ConfigModule, ConfigValidationError, ModuleConfigDocument};

pub const MAX_DYNAMIC_TOOL_SCHEMAS_FIELD: &str = "max_dynamic_tool_schemas";
pub const MAX_CONCURRENT_TOOL_CALLS_FIELD: &str = "max_concurrent_tool_calls";
pub const TOOL_DEFAULT_TIMEOUT_MS_FIELD: &str = "default_timeout_ms";
pub const MAX_RETRIES_FIELD: &str = "max_retries";
const WORKSPACE_FILES_FIELD: &str = "workspace_files";
pub const FIXED_TOOL_IDS_FIELD: &str = "fixed_tool_ids";
/// The revision of Core's recommended fixed tools a saved list took in; absent is the
/// first. What was recommended later joins the list until
/// its user saves it again.
pub const FIXED_TOOL_RECOMMENDATIONS_FIELD: &str = "fixed_tool_recommendations";
/// Which service web.search and web.fetch use; absent is automatic.
pub const WEB_SERVICE_FIELD: &str = "web_service";

/// The stored web service choice.
pub fn web_service_from_document(
    document: &ModuleConfigDocument,
) -> Result<aura_tool_contract::WebServiceSettingV1, ConfigValidationError> {
    let Some(value) = document.values.get(WEB_SERVICE_FIELD) else {
        return Ok(aura_tool_contract::WebServiceSettingV1::Automatic);
    };
    serde_json::from_value(value.clone()).map_err(|_| {
        ConfigValidationError::Semantic(
            "web_service must name automatic, local or a deepseek connection".into(),
        )
    })
}

pub fn fixed_tool_ids_from_document(
    document: &ModuleConfigDocument,
) -> Result<Option<Vec<aura_domain_common::ToolId>>, ConfigValidationError> {
    let Some(value) = document.values.get(FIXED_TOOL_IDS_FIELD) else { return Ok(None) };
    let ids: Vec<aura_domain_common::ToolId> =
        serde_json::from_value(value.clone()).map_err(|_| {
            ConfigValidationError::Semantic("fixed_tool_ids must contain Tool IDs".into())
        })?;
    if !ids.windows(2).all(|p| p[0] < p[1]) {
        return Err(ConfigValidationError::Semantic(
            "fixed_tool_ids must be sorted and unique".into(),
        ));
    }
    Ok(Some(ids))
}

/// The recommendations revision a saved list took in.
pub fn fixed_tool_recommendations_from_document(
    document: &ModuleConfigDocument,
) -> Result<u32, ConfigValidationError> {
    let Some(value) = document.values.get(FIXED_TOOL_RECOMMENDATIONS_FIELD) else { return Ok(1) };
    value.as_u64().and_then(|v| u32::try_from(v).ok()).filter(|v| *v >= 1).ok_or_else(|| {
        ConfigValidationError::Semantic(
            "fixed_tool_recommendations must be a revision number from 1".into(),
        )
    })
}

pub const DEFAULT_TOOL_POLICY_FIELDS: [(&str, u64); 4] = [
    (MAX_DYNAMIC_TOOL_SCHEMAS_FIELD, DEFAULT_MAX_DYNAMIC_TOOL_SCHEMAS as u64),
    (MAX_CONCURRENT_TOOL_CALLS_FIELD, DEFAULT_MAX_CONCURRENT_TOOL_CALLS as u64),
    (TOOL_DEFAULT_TIMEOUT_MS_FIELD, DEFAULT_TOOL_TIMEOUT_MS),
    (MAX_RETRIES_FIELD, DEFAULT_MAX_TOOL_RETRIES as u64),
];

pub(crate) fn validate_tools_document(
    document: &ModuleConfigDocument,
) -> Result<(), ConfigValidationError> {
    for key in document.values.keys() {
        if !matches!(
            key.as_str(),
            WORKSPACE_FILES_FIELD
                | MAX_DYNAMIC_TOOL_SCHEMAS_FIELD
                | MAX_CONCURRENT_TOOL_CALLS_FIELD
                | TOOL_DEFAULT_TIMEOUT_MS_FIELD
                | MAX_RETRIES_FIELD
                | FIXED_TOOL_IDS_FIELD
                | FIXED_TOOL_RECOMMENDATIONS_FIELD
                | WEB_SERVICE_FIELD
                | crate::NETWORK_PROXY_FIELD
        ) {
            return Err(ConfigValidationError::UnknownModuleField {
                module: ConfigModule::Tools,
                field: key.clone(),
            });
        }
    }
    fixed_tool_ids_from_document(document)?;
    fixed_tool_recommendations_from_document(document)?;
    web_service_from_document(document)?;
    tool_settings_snapshot_from_document(document).map(|_| ())
}

pub fn tool_settings_snapshot_from_document(
    document: &ModuleConfigDocument,
) -> Result<ToolSettingsSnapshotV1, ConfigValidationError> {
    let max_dynamic_tool_schemas =
        read_u32(document, MAX_DYNAMIC_TOOL_SCHEMAS_FIELD, DEFAULT_MAX_DYNAMIC_TOOL_SCHEMAS)?;
    let max_concurrent_tool_calls =
        read_u32(document, MAX_CONCURRENT_TOOL_CALLS_FIELD, DEFAULT_MAX_CONCURRENT_TOOL_CALLS)?;
    let default_timeout_ms =
        read_u64(document, TOOL_DEFAULT_TIMEOUT_MS_FIELD, DEFAULT_TOOL_TIMEOUT_MS)?;
    let max_retries = read_u8(document, MAX_RETRIES_FIELD, DEFAULT_MAX_TOOL_RETRIES)?;
    ToolSettingsSnapshotV1::new(
        Revision::new(document.revision),
        max_dynamic_tool_schemas,
        max_concurrent_tool_calls,
        default_timeout_ms,
        max_retries,
    )
    .and_then(|snapshot| {
        snapshot.with_network_proxy(
            crate::network_proxy_state_from_document(document).effective_proxy(),
        )
    })
    .map_err(|error| ConfigValidationError::Semantic(error.to_string()))
}

fn read_u32(
    document: &ModuleConfigDocument,
    field: &str,
    default: u32,
) -> Result<u32, ConfigValidationError> {
    let value = read_u64(document, field, u64::from(default))?;
    u32::try_from(value).map_err(|_| invalid_unsigned_integer(field))
}

fn read_u8(
    document: &ModuleConfigDocument,
    field: &str,
    default: u8,
) -> Result<u8, ConfigValidationError> {
    let value = read_u64(document, field, u64::from(default))?;
    u8::try_from(value).map_err(|_| invalid_unsigned_integer(field))
}

fn read_u64(
    document: &ModuleConfigDocument,
    field: &str,
    default: u64,
) -> Result<u64, ConfigValidationError> {
    match document.values.get(field) {
        None => Ok(default),
        Some(Value::Number(value)) => value.as_u64().ok_or_else(|| invalid_unsigned_integer(field)),
        Some(_) => Err(invalid_unsigned_integer(field)),
    }
}

fn invalid_unsigned_integer(field: &str) -> ConfigValidationError {
    ConfigValidationError::InvalidModuleFieldType {
        module: ConfigModule::Tools,
        field: field.to_owned(),
        expected: "unsigned integer",
    }
}

#[cfg(test)]
mod tests {
    use aura_tool_contract::{
        DEFAULT_MAX_CONCURRENT_TOOL_CALLS, DEFAULT_MAX_DYNAMIC_TOOL_SCHEMAS,
        DEFAULT_MAX_TOOL_RETRIES, DEFAULT_TOOL_TIMEOUT_MS,
    };
    use serde_json::json;

    use super::*;

    #[test]
    fn missing_policy_fields_use_the_frozen_defaults_without_touching_workspace_files() {
        let mut document = ModuleConfigDocument::empty();
        document.values.insert("workspace_files".to_owned(), json!({"read": {}}));

        validate_tools_document(&document).expect("tools document");
        let snapshot = tool_settings_snapshot_from_document(&document).expect("snapshot");

        assert_eq!(snapshot.max_dynamic_tool_schemas, DEFAULT_MAX_DYNAMIC_TOOL_SCHEMAS);
        assert_eq!(snapshot.max_concurrent_tool_calls, DEFAULT_MAX_CONCURRENT_TOOL_CALLS);
        assert_eq!(snapshot.default_timeout_ms, DEFAULT_TOOL_TIMEOUT_MS);
        assert_eq!(snapshot.max_retries, DEFAULT_MAX_TOOL_RETRIES);
    }

    #[test]
    fn policy_fields_are_typed_bounded_and_closed() {
        let mut document = ModuleConfigDocument::empty();
        document.revision = 7;
        document.values.extend([
            (MAX_DYNAMIC_TOOL_SCHEMAS_FIELD.to_owned(), json!(25)),
            (MAX_CONCURRENT_TOOL_CALLS_FIELD.to_owned(), json!(8)),
            (TOOL_DEFAULT_TIMEOUT_MS_FIELD.to_owned(), json!(30_000)),
            (MAX_RETRIES_FIELD.to_owned(), json!(2)),
        ]);
        let snapshot = tool_settings_snapshot_from_document(&document).expect("snapshot");
        assert_eq!(snapshot.revision, Revision::new(7));
        assert_eq!(snapshot.max_dynamic_tool_schemas, 25);
        assert_eq!(snapshot.max_concurrent_tool_calls, 8);
        assert_eq!(snapshot.default_timeout_ms, 30_000);
        assert_eq!(snapshot.max_retries, 2);

        document.values.insert(MAX_RETRIES_FIELD.to_owned(), json!(4));
        assert!(tool_settings_snapshot_from_document(&document).is_err());
        document.values.remove(MAX_RETRIES_FIELD);
        document.values.insert("package_trust".to_owned(), json!([]));
        assert!(validate_tools_document(&document).is_err());
    }

    /// A saved list says which recommendations it took in; one saved before says nothing.
    #[test]
    fn a_saved_list_names_the_recommendations_it_took_in() {
        let mut document = ModuleConfigDocument::empty();
        assert_eq!(fixed_tool_recommendations_from_document(&document).expect("absent"), 1);
        document.values.insert(FIXED_TOOL_RECOMMENDATIONS_FIELD.to_owned(), json!(2));
        validate_tools_document(&document).expect("a known field");
        assert_eq!(fixed_tool_recommendations_from_document(&document).expect("saved"), 2);
        for bad in [json!(0), json!(-1), json!("2"), json!(1.5)] {
            document.values.insert(FIXED_TOOL_RECOMMENDATIONS_FIELD.to_owned(), bad);
            assert!(validate_tools_document(&document).is_err());
        }
    }
}
