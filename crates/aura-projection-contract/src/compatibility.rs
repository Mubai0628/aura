//! Main-owned compatibility metadata never carries an invalid projection's payload.
use crate::ProjectionCompatibilityKey;
use aura_domain_common::{Revision, RuntimeInstanceId};
use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize, de::Error as _};

pub const PROJECTION_COMPATIBILITY_VERSION: u16 = 1;

#[derive(Clone, Debug, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectionCompatibilitySnapshotV1 {
    pub contract_version: u16,
    pub runtime_instance_id: RuntimeInstanceId,
    pub revision: Revision,
    pub paused_projections: Vec<ProjectionCompatibilityKey>,
}

impl<'de> Deserialize<'de> for ProjectionCompatibilitySnapshotV1 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            contract_version: u16,
            runtime_instance_id: RuntimeInstanceId,
            revision: Revision,
            paused_projections: Vec<ProjectionCompatibilityKey>,
        }
        let wire = Wire::deserialize(deserializer)?;
        if wire.contract_version != PROJECTION_COMPATIBILITY_VERSION {
            return Err(D::Error::custom("unsupported projection compatibility version"));
        }
        let mut keys = std::collections::BTreeSet::new();
        for key in &wire.paused_projections {
            if key.runtime_instance_id() != wire.runtime_instance_id || !keys.insert(key.clone()) {
                return Err(D::Error::custom("projection compatibility scope is inconsistent"));
            }
        }
        Ok(Self {
            contract_version: wire.contract_version,
            runtime_instance_id: wire.runtime_instance_id,
            revision: wire.revision,
            paused_projections: wire.paused_projections,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ProjectionKey, ProjectionKind, ProjectionScope};

    #[test]
    fn compatibility_metadata_roundtrips_and_rejects_other_runtime_or_payload() {
        let runtime = RuntimeInstanceId::new();
        let snapshot = ProjectionCompatibilitySnapshotV1 {
            contract_version: PROJECTION_COMPATIBILITY_VERSION,
            runtime_instance_id: runtime,
            revision: Revision::INITIAL,
            paused_projections: vec![
                ProjectionKey {
                    runtime_instance_id: runtime,
                    projection_kind: ProjectionKind::ToolSettings,
                    projection_scope: ProjectionScope::Global,
                }
                .into(),
            ],
        };
        let wire = serde_json::to_value(&snapshot).expect("compatibility metadata wire");
        assert_eq!(
            serde_json::from_value::<ProjectionCompatibilitySnapshotV1>(wire.clone())
                .expect("versioned metadata roundtrip"),
            snapshot
        );
        let mut foreign = wire.clone();
        foreign["runtime_instance_id"] =
            serde_json::to_value(RuntimeInstanceId::new()).expect("unrelated runtime identity");
        let mut unsupported = wire.clone();
        unsupported["contract_version"] = serde_json::json!(2);
        let mut duplicate = wire;
        let second_key = duplicate["paused_projections"][0].clone();
        duplicate["paused_projections"]
            .as_array_mut()
            .expect("metadata key array")
            .push(second_key);
        for invalid in [foreign, unsupported, duplicate] {
            assert!(serde_json::from_value::<ProjectionCompatibilitySnapshotV1>(invalid).is_err());
        }
        let mut invalid = serde_json::to_value(snapshot).expect("wire");
        invalid["payload"] = serde_json::json!({"forbidden":"not in metadata"});
        assert!(serde_json::from_value::<ProjectionCompatibilitySnapshotV1>(invalid).is_err());
    }
}
