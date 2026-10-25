//! Compatibility identities for the independent security and terminal streams.
use crate::ProjectionKey;
use aura_domain_common::{RuntimeInstanceId, SessionId};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(untagged)]
pub enum ProjectionCompatibilityKey {
    Projection(ProjectionKey),
    Auxiliary(AuxiliaryProjectionKey),
}

impl ProjectionCompatibilityKey {
    pub const fn runtime_instance_id(&self) -> RuntimeInstanceId {
        match self {
            Self::Projection(key) => key.runtime_instance_id,
            Self::Auxiliary(key) => key.runtime_instance_id(),
        }
    }
}
impl From<ProjectionKey> for ProjectionCompatibilityKey {
    fn from(key: ProjectionKey) -> Self {
        Self::Projection(key)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "projection_kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AuxiliaryProjectionKey {
    SessionPermission {
        runtime_instance_id: RuntimeInstanceId,
        projection_scope: CompatibilitySessionScope,
    },
    PendingApproval {
        runtime_instance_id: RuntimeInstanceId,
        projection_scope: CompatibilityGlobalScope,
    },
    SecurityRuntime {
        runtime_instance_id: RuntimeInstanceId,
        projection_scope: CompatibilityGlobalScope,
    },
    TerminalState {
        runtime_instance_id: RuntimeInstanceId,
        projection_scope: CompatibilityTerminalScope,
    },
}

impl AuxiliaryProjectionKey {
    const fn runtime_instance_id(&self) -> RuntimeInstanceId {
        match self {
            Self::SessionPermission { runtime_instance_id, .. }
            | Self::PendingApproval { runtime_instance_id, .. }
            | Self::SecurityRuntime { runtime_instance_id, .. }
            | Self::TerminalState { runtime_instance_id, .. } => *runtime_instance_id,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompatibilityGlobalScope {
    Global,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompatibilitySessionScope {
    Session { session_id: SessionId },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum CompatibilityTerminalScope {
    Terminal {
        session_id: SessionId,
        #[serde(deserialize_with = "terminal_id")]
        #[schemars(schema_with = "terminal_id_schema")]
        terminal_id: String,
    },
}

fn terminal_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    let suffix = value.strip_prefix("term_").unwrap_or_default();
    if suffix.is_empty()
        || suffix.len() > 128
        || !suffix.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
    {
        return Err(D::Error::custom("invalid terminal compatibility identity"));
    }
    Ok(value)
}
fn terminal_id_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({"type": "string", "pattern": "^term_[A-Za-z0-9_-]{1,128}$"})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auxiliary_keys_roundtrip_only_their_exact_scope_without_payload() {
        let runtime = RuntimeInstanceId::new();
        let session = SessionId::new();
        for (kind, scope) in [
            ("session_permission", serde_json::json!({"type":"session","session_id":session})),
            ("pending_approval", serde_json::json!({"type":"global"})),
            ("security_runtime", serde_json::json!({"type":"global"})),
            (
                "terminal_state",
                serde_json::json!({"type":"terminal","session_id":session,"terminal_id":"term_fixture"}),
            ),
        ] {
            let wire = serde_json::json!({"runtime_instance_id":runtime,"projection_kind":kind,"projection_scope":scope});
            let key: ProjectionCompatibilityKey =
                serde_json::from_value(wire.clone()).expect("scoped metadata");
            assert_eq!(serde_json::to_value(key).expect("wire"), wire);
            for field in ["payload", "private_data"] {
                let mut invalid = wire.clone();
                invalid[field] = serde_json::json!("must never cross metadata");
                assert!(serde_json::from_value::<ProjectionCompatibilityKey>(invalid).is_err());
            }
            let mut wrong = wire;
            wrong["projection_scope"] =
                serde_json::json!({"type":"workspace","workspace_id":"bad"});
            assert!(serde_json::from_value::<ProjectionCompatibilityKey>(wrong).is_err());
        }
    }
}
