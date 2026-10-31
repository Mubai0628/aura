use aura_domain_common::jcs::canonical_sha256;
use aura_domain_common::{CorrelationId, ErrorCategory, UtcTimestamp};
use aura_persistence::{PersistenceCoordinator, ToolRegistryBuildRequestV1};
use aura_tool_contract::{
    ToolCapabilitySummaryV1, ToolDescribeRequestV1, ToolDescribeResultV1, ToolListLoadedRequestV1,
    ToolListLoadedResultV1, ToolLoadRequestV1, ToolLoadStateV1, ToolLoadedEntryV1,
    ToolRegistryErrorCode, ToolRegistrySnapshotRef, ToolResourceClaimSummaryV1,
    ToolSafeIssueViewV1, ToolSchemaSnapshotV1,
};
use aura_tool_registry::BuiltinCoreHandlerV1;
use aura_tool_runtime::ToolRuntimeCandidateV1;
use serde_json::{Value, json};

use crate::p07_registry::{ToolRegistryControlErrorV1, ToolRegistryControlServiceV1};

struct BuiltinHandlerInvocationV1 {
    candidate: ToolRuntimeCandidateV1,
    resulting_registry_snapshot_ref: Option<ToolRegistrySnapshotRef>,
    changed: Option<bool>,
}

fn invoke_handler_inner(
    persistence: &PersistenceCoordinator,
    handler: BuiltinCoreHandlerV1,
    arguments: &Value,
    registry: &aura_tool_contract::ToolRegistrySnapshotV1,
    registry_build_request: Option<ToolRegistryBuildRequestV1>,
    correlation_id: CorrelationId,
) -> Result<BuiltinHandlerInvocationV1, ToolRegistryControlErrorV1> {
    let service = ToolRegistryControlServiceV1;
    let response = match handler {
        BuiltinCoreHandlerV1::ToolsSearch => {
            let request: aura_tool_contract::ToolSearchRequestV1 =
                serde_json::from_value(arguments.clone())
                    .map_err(|_| ToolRegistryControlErrorV1::RequestInvalid)?;
            let visibility = canonical_sha256(&json!({
                "visibility_version": 1,
                "scope": registry.scope,
                "registry_snapshot_ref": registry.snapshot_ref,
            }))
            .map_err(|_| ToolRegistryControlErrorV1::RequestInvalid)?;
            serde_json::to_value(service.search(
                persistence,
                &registry.snapshot_ref,
                &visibility,
                &request,
            )?)
            .map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?
        }
        BuiltinCoreHandlerV1::ToolsDescribe => {
            let request: ToolDescribeRequestV1 = serde_json::from_value(arguments.clone())
                .map_err(|_| ToolRegistryControlErrorV1::RequestInvalid)?;
            if request.definition_ref.registry_snapshot_ref != registry.snapshot_ref {
                return Err(ToolRegistryControlErrorV1::SnapshotScopeMismatch);
            }
            let described = service.describe(persistence, &request.definition_ref)?;
            serde_json::to_value(describe_result(persistence, registry, request, described)?)
                .map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?
        }
        BuiltinCoreHandlerV1::ToolsListLoaded => {
            let request: ToolListLoadedRequestV1 = serde_json::from_value(arguments.clone())
                .map_err(|_| ToolRegistryControlErrorV1::RequestInvalid)?;
            if request.registry_snapshot_ref != registry.snapshot_ref {
                return Err(ToolRegistryControlErrorV1::SnapshotScopeMismatch);
            }
            let exact = service.list_loaded(persistence, &request.registry_snapshot_ref)?;
            let tools = registry
                .catalog_entries
                .iter()
                .filter(|entry| {
                    exact.iter().any(|definition| {
                        definition.tool_id == entry.tool_id
                            && definition.tool_version == entry.tool_version
                    }) && entry.load_state != ToolLoadStateV1::Discoverable
                })
                .map(|entry| {
                    let definition_ref = aura_tool_contract::ToolDefinitionSnapshotRef {
                        tool_id: entry.tool_id,
                        tool_version: entry.tool_version.clone(),
                        registry_snapshot_ref: registry.snapshot_ref.clone(),
                    };
                    let audience = persistence
                        .describe_tool_definition_exact(&definition_ref)?
                        .ok_or(ToolRegistryControlErrorV1::DefinitionUnavailable)?
                        .definition
                        .audience;
                    Ok(ToolLoadedEntryV1 {
                        definition_ref,
                        tool_instance_id: entry.tool_instance_id,
                        canonical_name: entry.canonical_name.clone(),
                        display_name: entry.display_name.clone(),
                        model_summary: entry.summary.clone(),
                        source: entry.source,
                        audience,
                        load_state: entry.load_state,
                    })
                })
                .collect::<Result<Vec<_>, ToolRegistryControlErrorV1>>()?;
            serde_json::to_value(ToolListLoadedResultV1 {
                registry_snapshot_ref: registry.snapshot_ref.clone(),
                tools,
            })
            .map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?
        }
        BuiltinCoreHandlerV1::ToolsLoad | BuiltinCoreHandlerV1::ToolsUnload => {
            let request: ToolLoadRequestV1 = serde_json::from_value(arguments.clone())
                .map_err(|_| ToolRegistryControlErrorV1::RequestInvalid)?;
            if request
                .definitions
                .iter()
                .any(|definition| definition.registry_snapshot_ref != registry.snapshot_ref)
            {
                return Err(ToolRegistryControlErrorV1::SnapshotScopeMismatch);
            }
            let mut registry_build_request =
                registry_build_request.ok_or(ToolRegistryControlErrorV1::BuildContextMissing)?;
            if registry_build_request.scope != registry.scope {
                return Err(ToolRegistryControlErrorV1::SnapshotScopeMismatch);
            }
            registry_build_request.created_at = UtcTimestamp::now();
            registry_build_request.correlation_id = correlation_id;
            let published = if handler == BuiltinCoreHandlerV1::ToolsLoad {
                service.load(persistence, request, registry_build_request)?
            } else {
                service.unload(persistence, request, registry_build_request)?
            };
            let response = published.result;
            return Ok(BuiltinHandlerInvocationV1 {
                candidate: ToolRuntimeCandidateV1::CoreBuiltinHandlerCompleted {
                    response_body: aura_domain_common::jcs::canonical_json_jcs(&response)
                        .map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?,
                },
                resulting_registry_snapshot_ref: Some(
                    response.resulting_registry_snapshot_ref.clone(),
                ),
                changed: Some(response.changed),
            });
        }
        // This entry has no Agent run to observe from, so a well-formed query is refused rather
        // than answered from an empty index.
        BuiltinCoreHandlerV1::ActivityQuery => {
            let _: aura_activity_domain::AgentActivityQueryInput =
                serde_json::from_value(arguments.clone())
                    .map_err(|_| ToolRegistryControlErrorV1::RequestInvalid)?;
            return Err(ToolRegistryControlErrorV1::BuildContextMissing);
        }
    };
    Ok(BuiltinHandlerInvocationV1 {
        candidate: ToolRuntimeCandidateV1::CoreBuiltinHandlerCompleted {
            response_body: aura_domain_common::jcs::canonical_json_jcs(&response)
                .map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?,
        },
        resulting_registry_snapshot_ref: None,
        changed: None,
    })
}

/// Service entry; Core has already bound the exact stored Snapshot and
/// derived the build context.
///
/// This entry serves the Renderer/IPC tool-administration surface, which has no model step and
/// therefore no Agent run to observe: `agent.activity.query` is refused here rather than being
/// answered from a fabricated empty index.
pub fn invoke_broker_service(
    persistence: &PersistenceCoordinator,
    handler: BuiltinCoreHandlerV1,
    arguments: &Value,
    registry: &aura_tool_contract::ToolRegistrySnapshotV1,
    build: Option<ToolRegistryBuildRequestV1>,
    correlation: CorrelationId,
) -> Result<aura_tool_contract::ToolBrokerInvokeResult, ToolRegistryControlErrorV1> {
    let result =
        invoke_handler_inner(persistence, handler, arguments, registry, build, correlation)?;
    let ToolRuntimeCandidateV1::CoreBuiltinHandlerCompleted { response_body } = result.candidate
    else {
        return Err(ToolRegistryControlErrorV1::ResponseInvalid);
    };
    Ok(aura_tool_contract::ToolBrokerInvokeResult {
        value: serde_json::from_slice(&response_body)
            .map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?,
        resulting_registry_snapshot_ref: result.resulting_registry_snapshot_ref,
        changed: result.changed,
    })
}

fn describe_result(
    persistence: &PersistenceCoordinator,
    registry: &aura_tool_contract::ToolRegistrySnapshotV1,
    request: ToolDescribeRequestV1,
    described: aura_persistence::ExactToolDefinitionDescriptionV1,
) -> Result<ToolDescribeResultV1, ToolRegistryControlErrorV1> {
    let entry = registry
        .catalog_entries
        .iter()
        .find(|entry| {
            entry.tool_id == request.definition_ref.tool_id
                && entry.tool_version == request.definition_ref.tool_version
        })
        .ok_or(ToolRegistryControlErrorV1::DefinitionUnavailable)?;
    let schema = ToolSchemaSnapshotV1::new(
        described.definition.tool_id,
        described.definition.tool_version.clone(),
        described.definition.canonical_name.clone(),
        described.definition.model_description.clone(),
        described.definition.input_schema.clone(),
        described.definition.strict,
    )
    .map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?;
    let capability_summary = described
        .definition
        .capabilities
        .iter()
        .map(|capability| {
            let capability = serde_json::to_value(capability)
                .ok()
                .and_then(|value| value.as_str().map(ToOwned::to_owned))
                .ok_or(ToolRegistryControlErrorV1::ResponseInvalid)?;
            Ok(ToolCapabilitySummaryV1 { summary: capability.replace('_', " "), capability })
        })
        .collect::<Result<Vec<_>, ToolRegistryControlErrorV1>>()?;
    let resource_claim_summary = if described.definition.tool_package_id.is_some() {
        persistence
            .load_exact_tool_execution_authority(&request.definition_ref, None)?
            .and_then(|authority| authority.package)
            .map(|package| {
                package
                    .manifest_tool()
                    .resource_claims
                    .iter()
                    .map(package_resource_claim_summary)
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default()
    } else {
        // Built-ins and typed External Sources are not Package authority and
        // must never be reinterpreted as one merely to build a description.
        Vec::new()
    };
    let healthy = described.instance.enabled
        && described.instance.runtime_health == aura_tool_domain::ToolRuntimeHealthV1::Healthy;
    Ok(ToolDescribeResultV1 {
        definition_ref: request.definition_ref,
        canonical_name: described.definition.canonical_name,
        display_name: described.definition.display_name,
        model_summary: described.definition.model_summary,
        model_description: described.definition.model_description,
        schema,
        result_contract: described.definition.result_contract,
        source: described.definition.source_kind,
        audience: described.definition.audience,
        capability_summary,
        resource_claim_summary,
        load_state: entry.load_state,
        currently_loadable: healthy,
        safe_unavailability_reason: (!healthy).then(|| ToolSafeIssueViewV1 {
            code: ToolRegistryErrorCode::Disabled.error_code(),
            category: ErrorCategory::Conflict,
            message: "The exact tool instance is not currently eligible.".to_owned(),
            retryable: false,
        }),
    })
}

fn package_resource_claim_summary(
    claim: &aura_tool_contract::PackageDeclaredResourceClaimV1,
) -> Result<ToolResourceClaimSummaryV1, ToolRegistryControlErrorV1> {
    let value =
        serde_json::to_value(claim).map_err(|_| ToolRegistryControlErrorV1::ResponseInvalid)?;
    let resource_kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or(ToolRegistryControlErrorV1::ResponseInvalid)?
        .to_owned();
    let access = value
        .get("access")
        .or_else(|| value.get("selector").and_then(|selector| selector.get("access")))
        .map(|value| {
            value
                .as_str()
                .map(|value| vec![value.to_owned()])
                .or_else(|| {
                    value.as_array().map(|values| {
                        values.iter().filter_map(Value::as_str).map(ToOwned::to_owned).collect()
                    })
                })
                .unwrap_or_default()
        })
        .unwrap_or_default();
    Ok(ToolResourceClaimSummaryV1 { claim_key: claim.claim_key().clone(), resource_kind, access })
}
