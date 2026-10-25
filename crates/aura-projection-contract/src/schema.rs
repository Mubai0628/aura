use aura_domain_common::{
    EndpointId, ModelAvailabilityTestId, ModelDiscoveryRunId, ModelProfileId, MutationJournalId,
    Revision, RuntimeInstanceId, SessionId, WorkspaceId,
};
use aura_domain_common::{
    ToolBatchId, ToolCallId, ToolConformanceRunId, ToolDraftId, ToolExecutionAttemptId, ToolId,
    ToolPackageId,
};
use schemars::{Schema, SchemaGenerator};
use serde_json::{Map, Value, json};

#[derive(Clone, Copy)]
enum ProjectionRouteEnvelope {
    Key,
    Subscription,
    SnapshotRequest,
    Unsubscribe,
}

pub(crate) fn projection_key_schema(generator: &mut SchemaGenerator) -> Schema {
    projection_route_schema(generator, ProjectionRouteEnvelope::Key)
}

pub(crate) fn projection_subscription_schema(generator: &mut SchemaGenerator) -> Schema {
    projection_route_schema(generator, ProjectionRouteEnvelope::Subscription)
}

pub(crate) fn projection_snapshot_request_schema(generator: &mut SchemaGenerator) -> Schema {
    projection_route_schema(generator, ProjectionRouteEnvelope::SnapshotRequest)
}

pub(crate) fn projection_unsubscribe_request_schema(generator: &mut SchemaGenerator) -> Schema {
    projection_route_schema(generator, ProjectionRouteEnvelope::Unsubscribe)
}

pub(crate) fn global_scope_schema(_generator: &mut SchemaGenerator) -> Schema {
    schemars::json_schema!({
        "type": "object",
        "properties": {
            "type": {"const": "global"}
        },
        "required": ["type"],
        "additionalProperties": false
    })
}

pub(crate) fn endpoint_scope_schema(generator: &mut SchemaGenerator) -> Schema {
    scoped_projection_schema("endpoint", "endpoint_id", generator.subschema_for::<EndpointId>())
}

pub(crate) fn model_profile_scope_schema(generator: &mut SchemaGenerator) -> Schema {
    scoped_projection_schema(
        "model_profile",
        "model_profile_id",
        generator.subschema_for::<ModelProfileId>(),
    )
}

pub(crate) fn session_scope_schema(generator: &mut SchemaGenerator) -> Schema {
    scoped_projection_schema("session", "session_id", generator.subschema_for::<SessionId>())
}

pub(crate) fn workspace_scope_schema(generator: &mut SchemaGenerator) -> Schema {
    scoped_projection_schema("workspace", "workspace_id", generator.subschema_for::<WorkspaceId>())
}

pub(crate) fn tool_package_scope_schema(generator: &mut SchemaGenerator) -> Schema {
    scoped_projection_schema(
        "tool_package",
        "tool_package_id",
        generator.subschema_for::<ToolPackageId>(),
    )
}

pub(crate) fn tool_definition_scope_schema(generator: &mut SchemaGenerator) -> Schema {
    scoped_projection_schema("tool_definition", "tool_id", generator.subschema_for::<ToolId>())
}

pub(crate) fn tool_draft_scope_schema(generator: &mut SchemaGenerator) -> Schema {
    scoped_projection_schema(
        "tool_draft",
        "tool_draft_id",
        generator.subschema_for::<ToolDraftId>(),
    )
}

pub(crate) fn tool_conformance_run_scope_schema(generator: &mut SchemaGenerator) -> Schema {
    scoped_projection_schema(
        "tool_conformance_run",
        "tool_conformance_run_id",
        generator.subschema_for::<ToolConformanceRunId>(),
    )
}

pub(crate) fn tool_batch_scope_schema(generator: &mut SchemaGenerator) -> Schema {
    scoped_projection_schema(
        "tool_batch",
        "tool_batch_id",
        generator.subschema_for::<ToolBatchId>(),
    )
}

pub(crate) fn tool_call_scope_schema(generator: &mut SchemaGenerator) -> Schema {
    scoped_projection_schema("tool_call", "tool_call_id", generator.subschema_for::<ToolCallId>())
}

pub(crate) fn tool_execution_attempt_scope_schema(generator: &mut SchemaGenerator) -> Schema {
    scoped_projection_schema(
        "tool_execution_attempt",
        "tool_execution_attempt_id",
        generator.subschema_for::<ToolExecutionAttemptId>(),
    )
}

pub(crate) fn tool_long_operation_scope_schema(generator: &mut SchemaGenerator) -> Schema {
    scoped_projection_schema(
        "tool_long_operation",
        "journal_id",
        generator.subschema_for::<MutationJournalId>(),
    )
}

pub(crate) fn model_availability_test_scope_schema(generator: &mut SchemaGenerator) -> Schema {
    scoped_projection_schema(
        "model_availability_test",
        "model_availability_test_id",
        generator.subschema_for::<ModelAvailabilityTestId>(),
    )
}

pub(crate) fn model_discovery_scope_schema(generator: &mut SchemaGenerator) -> Schema {
    scoped_projection_schema(
        "model_discovery",
        "model_discovery_run_id",
        generator.subschema_for::<ModelDiscoveryRunId>(),
    )
}

fn projection_route_schema(
    generator: &mut SchemaGenerator,
    envelope: ProjectionRouteEnvelope,
) -> Schema {
    let global_scope = schema_value(global_scope_schema(generator));
    let endpoint_scope = schema_value(endpoint_scope_schema(generator));
    let model_profile_scope = schema_value(model_profile_scope_schema(generator));
    let session_scope = schema_value(session_scope_schema(generator));
    let workspace_scope = schema_value(workspace_scope_schema(generator));
    let model_availability_test_scope =
        schema_value(model_availability_test_scope_schema(generator));
    let model_discovery_scope = schema_value(model_discovery_scope_schema(generator));
    let runtime_instance_id = schema_value(generator.subschema_for::<RuntimeInstanceId>());
    let optional_runtime_instance_id =
        schema_value(generator.subschema_for::<Option<RuntimeInstanceId>>());
    let optional_revision = schema_value(generator.subschema_for::<Option<Revision>>());

    let variants = [
        ("core_health", global_scope.clone()),
        ("workspace_session", global_scope.clone()),
        ("workspace_files", global_scope),
        ("model_settings", schema_value(global_scope_schema(generator))),
        ("provider_catalog", schema_value(global_scope_schema(generator))),
        ("endpoint_detail", endpoint_scope),
        ("model_profile", model_profile_scope),
        ("session_model_selector", session_scope.clone()),
        ("model_availability_test", model_availability_test_scope),
        ("model_discovery", model_discovery_scope),
        ("context_window_correction", session_scope),
        ("prompt_inspector", schema_value(session_scope_schema(generator))),
        ("tool_settings", schema_value(global_scope_schema(generator))),
        ("tool_package_list", schema_value(global_scope_schema(generator))),
        ("tool_draft_list", schema_value(global_scope_schema(generator))),
        ("workspace_tool_pins", workspace_scope.clone()),
        ("workspace_tool_registry_summary", workspace_scope),
        ("session_effective_tool_registry", schema_value(session_scope_schema(generator))),
        ("session_loaded_tools", schema_value(session_scope_schema(generator))),
        ("session_tool_activity", schema_value(session_scope_schema(generator))),
        ("session_tool_pins", schema_value(session_scope_schema(generator))),
        ("tool_package", schema_value(tool_package_scope_schema(generator))),
        ("tool_definition", schema_value(tool_definition_scope_schema(generator))),
        ("tool_draft", schema_value(tool_draft_scope_schema(generator))),
        ("tool_conformance_run", schema_value(tool_conformance_run_scope_schema(generator))),
        ("tool_batch", schema_value(tool_batch_scope_schema(generator))),
        ("tool_call", schema_value(tool_call_scope_schema(generator))),
        ("tool_execution_attempt", schema_value(tool_execution_attempt_scope_schema(generator))),
        ("tool_long_operation", schema_value(tool_long_operation_scope_schema(generator))),
    ]
    .into_iter()
    .map(|(kind, scope)| {
        route_variant_schema(
            kind,
            scope,
            envelope,
            &runtime_instance_id,
            &optional_runtime_instance_id,
            &optional_revision,
        )
    })
    .collect::<Vec<_>>();

    schemars::json_schema!({
        "oneOf": variants
    })
}

fn route_variant_schema(
    projection_kind: &str,
    projection_scope: Value,
    envelope: ProjectionRouteEnvelope,
    runtime_instance_id: &Value,
    optional_runtime_instance_id: &Value,
    optional_revision: &Value,
) -> Value {
    let mut properties = Map::new();
    properties.insert("projection_kind".to_owned(), json!({"const": projection_kind}));
    properties.insert("projection_scope".to_owned(), projection_scope);
    let mut required = vec!["projection_kind", "projection_scope"];

    match envelope {
        ProjectionRouteEnvelope::Key => {
            properties.insert("runtime_instance_id".to_owned(), runtime_instance_id.clone());
            required.push("runtime_instance_id");
        }
        ProjectionRouteEnvelope::Subscription => {
            properties.insert(
                "known_runtime_instance_id".to_owned(),
                optional_runtime_instance_id.clone(),
            );
            properties.insert("after_revision".to_owned(), optional_revision.clone());
        }
        ProjectionRouteEnvelope::SnapshotRequest => {
            properties.insert(
                "known_runtime_instance_id".to_owned(),
                optional_runtime_instance_id.clone(),
            );
        }
        ProjectionRouteEnvelope::Unsubscribe => {}
    }

    json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn scoped_projection_schema(scope_type: &str, id_field: &str, id_schema: Schema) -> Schema {
    let mut properties = Map::new();
    properties.insert("type".to_owned(), json!({"const": scope_type}));
    properties.insert(id_field.to_owned(), schema_value(id_schema));
    schemars::json_schema!({
        "type": "object",
        "properties": properties,
        "required": ["type", id_field],
        "additionalProperties": false
    })
}

fn schema_value(schema: Schema) -> Value {
    schema.to_value()
}
