use aura_domain_common::{CanonicalSemVer, Revision};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};
use thiserror::Error;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
#[error("tool definition revision must start at 1")]
pub struct ToolVersionError;

#[derive(Clone, Debug, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolVersion {
    pub semantic_version: Option<CanonicalSemVer>,
    pub definition_revision: Revision,
}

impl ToolVersion {
    pub fn new(
        semantic_version: Option<CanonicalSemVer>,
        definition_revision: Revision,
    ) -> Result<Self, ToolVersionError> {
        let version = Self { semantic_version, definition_revision };
        version.validate()?;
        Ok(version)
    }

    pub fn validate(&self) -> Result<(), ToolVersionError> {
        if self.definition_revision.value() == 0 {
            return Err(ToolVersionError);
        }
        Ok(())
    }
}

impl<'de> Deserialize<'de> for ToolVersion {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct WireVersion {
            semantic_version: Option<CanonicalSemVer>,
            definition_revision: Revision,
        }

        let wire = WireVersion::deserialize(deserializer)?;
        Self::new(wire.semantic_version, wire.definition_revision).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {

    use aura_domain_common::Revision;
    use serde_json::json;

    use super::ToolVersion;

    #[test]
    fn tool_version_serializes_null_and_rejects_zero_or_unknown_fields() {
        let version = ToolVersion::new(None, Revision::INITIAL).expect("initial version");
        assert_eq!(
            serde_json::to_value(version).expect("serialize version"),
            json!({"semantic_version": null, "definition_revision": "1"})
        );
        assert!(
            serde_json::from_value::<ToolVersion>(
                json!({"semantic_version": null, "definition_revision": "0"})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<ToolVersion>(json!({
                "semantic_version": "1.2.3",
                "definition_revision": "1",
                "enabled": true
            }))
            .is_err()
        );
    }
}
