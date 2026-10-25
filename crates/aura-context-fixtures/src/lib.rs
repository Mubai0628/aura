#![forbid(unsafe_code)]

use std::str::FromStr;

use aura_context_domain::{
    ContextSourceRef, ModelEgressAuthorizationBasis, ModelEgressSourceKind,
    PromptInspectorTaskLineageView,
};
use aura_context_runtime::ContextFragment;
use aura_domain_common::CanonicalSemVer;
use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{Revision, TaskLineageId};
use aura_domain_common::{ToolId, ToolInstanceId, ToolRegistrySnapshotId};
use aura_security_contract::NetworkDataClass;
use aura_tool_contract::{
    EffectiveToolBindingSnapshot, ToolExposureV1, ToolInputSchemaV1, ToolRegistrySnapshotRef,
    ToolSchemaSetSnapshotV1, ToolSchemaSnapshotV1, ToolSourceKindV1, ToolVersion,
};
use serde_json::{Value, json};
use thiserror::Error;

mod p06;
mod performance;

pub use p06::*;
pub use performance::*;

#[derive(Clone, Debug)]
pub struct P08TaskLineageFixture {
    pub view: PromptInspectorTaskLineageView,
    pub context_fragment: ContextFragment,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ContextFixtureError {
    #[error("context.fixture.static_identity_invalid")]
    StaticIdentityInvalid,
    #[error("context.fixture.tool_version_invalid")]
    ToolVersionInvalid,
    #[error("context.fixture.schema_serialization_failed")]
    SchemaSerializationFailed,
    #[error("context.fixture.preparation_failed")]
    PreparationFailed,
    #[error("context.fixture.preparation_did_not_produce_a_snapshot")]
    PreparationDidNotProduceSnapshot,
}

pub fn empty_p07_context_tool_schema_set() -> Result<ToolSchemaSetSnapshotV1, ContextFixtureError> {
    let registry_snapshot_ref = ToolRegistrySnapshotRef {
        tool_registry_snapshot_id: ToolRegistrySnapshotId::from_str(
            "trs_01ARZ3NDEKTSV4RRFFQ69G5FAW",
        )
        .map_err(|_| ContextFixtureError::StaticIdentityInvalid)?,
        registry_revision: Revision::INITIAL,
        content_hash: CanonicalHashV1::sha256(b"p07-empty-context-fixture-registry"),
    };
    ToolSchemaSetSnapshotV1::new(registry_snapshot_ref, Vec::new())
        .map_err(|_| ContextFixtureError::SchemaSerializationFailed)
}

pub fn p07_context_tool_schema_set()
-> Result<(ToolSchemaSetSnapshotV1, Vec<EffectiveToolBindingSnapshot>), ContextFixtureError> {
    let definitions = [
        (
            "tol_01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "context.status",
            "Read the current immutable Context status for this Session and AgentRun.",
            scoped_schema(json!({})),
        ),
        (
            "tol_01ARZ3NDEKTSV4RRFFQ69G5FAW",
            "context.search",
            "Search model-visible indexed Context using Natural or ExactPhrase mode.",
            scoped_schema(json!({
                "query": {"type": "string", "minLength": 1},
                "mode": {"enum": ["natural", "exact_phrase"]},
                "filters": {"type": "object"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 50},
                "cursor": {"type": ["string", "null"]}
            })),
        ),
        (
            "tol_01ARZ3NDEKTSV4RRFFQ69G5FAX",
            "context.read_events",
            "Read model-visible Event facts with Atomic Group expansion.",
            scoped_schema(json!({
                "selectors": {"type": "array", "minItems": 1, "maxItems": 32},
                "character_limit": {"type": "integer", "minimum": 1, "maximum": 262144},
                "cursor": {"type": ["string", "null"]}
            })),
        ),
        (
            "tol_01ARZ3NDEKTSV4RRFFQ69G5FAY",
            "context.read_tool_result",
            "Read the exact model-visible Tool Result and optional original model Tool input.",
            scoped_schema(json!({
                "tool_call_id": {"type": "string", "pattern": "^tcl_[0-9A-HJKMNP-TV-Z]{26}$"},
                "include_input": {"type": "boolean"},
                "cursor": {"type": ["string", "null"]}
            })),
        ),
        (
            "tol_01ARZ3NDEKTSV4RRFFQ69G5FAZ",
            "context.read_compaction",
            "Read a validated historical Context compaction and its source anchors.",
            scoped_schema(json!({
                "source_context_snapshot_id": {
                    "type": "string",
                    "pattern": "^cps_[0-9A-HJKMNP-TV-Z]{26}$"
                },
                "sequence": {"type": "integer", "minimum": 1}
            })),
        ),
    ];
    let semantic_version =
        CanonicalSemVer::new("1.0.0").map_err(|_| ContextFixtureError::ToolVersionInvalid)?;
    let mut tools = definitions
        .into_iter()
        .map(|(id, name, description, input_schema)| {
            let tool_id =
                ToolId::from_str(id).map_err(|_| ContextFixtureError::StaticIdentityInvalid)?;
            let tool_version = ToolVersion::new(Some(semantic_version.clone()), Revision::INITIAL)
                .map_err(|_| ContextFixtureError::ToolVersionInvalid)?;
            ToolSchemaSnapshotV1::new(
                tool_id,
                tool_version,
                name.parse().map_err(|_| ContextFixtureError::SchemaSerializationFailed)?,
                description.to_owned(),
                ToolInputSchemaV1::new(input_schema)
                    .map_err(|_| ContextFixtureError::SchemaSerializationFailed)?,
                true,
            )
            .map_err(|_| ContextFixtureError::SchemaSerializationFailed)
        })
        .collect::<Result<Vec<_>, ContextFixtureError>>()?;
    tools.sort_by_key(|tool| tool.tool_id);
    let registry_snapshot_ref = ToolRegistrySnapshotRef {
        tool_registry_snapshot_id: ToolRegistrySnapshotId::from_str(
            "trs_01ARZ3NDEKTSV4RRFFQ69G5FAV",
        )
        .map_err(|_| ContextFixtureError::StaticIdentityInvalid)?,
        registry_revision: Revision::INITIAL,
        content_hash: CanonicalHashV1::sha256(b"p07-context-fixture-registry"),
    };
    let bindings = tools
        .iter()
        .enumerate()
        .map(|(index, tool)| EffectiveToolBindingSnapshot {
            tool_id: tool.tool_id,
            tool_version: tool.tool_version.clone(),
            tool_instance_id: ToolInstanceId::from_bytes(
                [u8::try_from(index + 1).unwrap_or(1); 16],
            ),
            source_kind: ToolSourceKindV1::BuiltIn,
            exposure: ToolExposureV1::Core,
            definition_fingerprint: CanonicalHashV1::sha256(
                tool.tool_schema_snapshot_hash.hex().as_bytes(),
            ),
            source_contract_fingerprint:
                aura_tool_contract::ToolSourceContractFingerprintV1::BuiltIn {
                    signed_builtin_tool_contract_fingerprint:
                        aura_tool_contract::SignedBuiltinToolContractFingerprintV1::new(
                            CanonicalHashV1::sha256(b"p07-context-fixture-source"),
                        ),
                },
        })
        .collect::<Vec<_>>();
    let schema_set = ToolSchemaSetSnapshotV1::new(registry_snapshot_ref, tools)
        .map_err(|_| ContextFixtureError::SchemaSerializationFailed)?;
    Ok((schema_set, bindings))
}

pub fn p08_active_task_lineage_fixture() -> Result<P08TaskLineageFixture, ContextFixtureError> {
    let lineage_id = TaskLineageId::from_str("lin_01ARZ3NDEKTSV4RRFFQ69G5FAV")
        .map_err(|_| ContextFixtureError::StaticIdentityInvalid)?;
    let revision = Revision::new(7);
    let content = concat!(
        "TaskLineage state: active\n",
        "Objective: preserve the frozen P06 Context invariants.\n",
        "Completion gate: source_recheck_required\n"
    )
    .to_owned();
    Ok(P08TaskLineageFixture {
        view: PromptInspectorTaskLineageView {
            lineage_id,
            revision,
            state: "active".to_owned(),
            source_count: 2,
            completion_policy: "all_sources_satisfied".to_owned(),
            completion_gate: "source_recheck_required".to_owned(),
            source_recheck_required: true,
            preview_characters: u64::try_from(content.chars().count())
                .map_err(|_| ContextFixtureError::SchemaSerializationFailed)?,
            preview_estimated_tokens: u64::try_from(content.len())
                .map_err(|_| ContextFixtureError::SchemaSerializationFailed)?,
        },
        context_fragment: ContextFragment {
            source_ref: ContextSourceRef::TaskLineage { lineage_id, revision },
            source_kind: ModelEgressSourceKind::TaskLineage,
            data_class: NetworkDataClass::WorkspaceMetadata,
            authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content,
        },
    })
}

fn scoped_schema(extra_properties: Value) -> Value {
    let mut properties = json!({
        "session_id": {"type": "string", "pattern": "^ses_[0-9A-HJKMNP-TV-Z]{26}$"},
        "agent_run_id": {"type": "string", "pattern": "^agt_[0-9A-HJKMNP-TV-Z]{26}$"}
    });
    if let (Some(properties), Some(extra)) =
        (properties.as_object_mut(), extra_properties.as_object())
    {
        properties.extend(extra.iter().map(|(key, value)| (key.clone(), value.clone())));
    }
    let required = properties
        .as_object()
        .map(|properties| properties.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p07_context_tool_fixtures_are_deterministic_strict_and_sorted() {
        let first = p07_context_tool_schema_set().expect("fixtures");
        let second = p07_context_tool_schema_set().expect("fixtures");
        assert_eq!(first, second);
        assert_eq!(first.0.tools.len(), 5);
        assert!(first.0.tools.windows(2).all(|pair| pair[0].tool_id < pair[1].tool_id));
        assert!(first.0.tools.iter().all(|tool| tool.strict));
        assert_eq!(first.0.tools.len(), first.1.len());
    }

    #[test]
    fn p08_fixture_is_read_only_model_visible_lineage_input() {
        let fixture = p08_active_task_lineage_fixture().expect("fixture");
        assert!(fixture.view.source_recheck_required);
        assert_eq!(fixture.context_fragment.source_kind, ModelEgressSourceKind::TaskLineage);
        assert_eq!(
            fixture.context_fragment.authorization_basis,
            ModelEgressAuthorizationBasis::ModelChannelPolicy
        );
        let ContextSourceRef::TaskLineage { lineage_id, revision } =
            fixture.context_fragment.source_ref
        else {
            panic!("fixture source must remain a TaskLineage reference");
        };
        assert_eq!(lineage_id, fixture.view.lineage_id);
        assert_eq!(revision, fixture.view.revision);
    }
}
