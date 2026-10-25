//! Full-field samples are generated from the Rust schema, then decoded through
//! the actual Rust DTO. New optional fields therefore enter desktop checks even
//! when no ordinary runtime fixture happens to set them.
use anyhow::{Context, Result, bail, ensure};
use aura_domain_common::{COMMON_ID_TYPES, Revision, TOOL_ID_TYPES};
use aura_projection_contract::{
    ProjectionChange, ProjectionCompatibilitySnapshotV1, ProjectionContractSchemaRoot,
};
use serde_json::{Map, Value, json};

pub const PATH: &str = "fixtures/contracts/projections-full.v1.json";

pub(crate) fn generated_artifact() -> Result<(&'static str, Vec<u8>)> {
    let samples = full_projection_samples(&crate::canonical_schema_value()?)?;
    Ok((PATH, crate::pretty_json(&samples)?))
}

pub(crate) fn canonical_projection_contract(
    runtime: &crate::RuntimeContractFixture,
) -> ProjectionContractSchemaRoot {
    ProjectionContractSchemaRoot {
        compatibility_snapshot: ProjectionCompatibilitySnapshotV1 {
            contract_version: 1,
            runtime_instance_id: runtime.projection_keys[0].runtime_instance_id,
            revision: Revision::INITIAL,
            paused_projections: Vec::new(),
        },
        projection_key: runtime.projection_keys[0],
        projection_subscription: runtime.projection_subscription,
        projection_unsubscribe_request: runtime.projection_unsubscribe_request,
        projection_snapshot_request: runtime.projection_snapshot_request,
        projection_change: runtime.projection_change.clone(),
    }
}

pub fn full_projection_samples(schema: &Value) -> Result<Value> {
    let definitions = schema["$defs"].as_object().context("contract definitions missing")?;
    let variants = definitions["ProjectionChange"]["oneOf"]
        .as_array()
        .context("projection variants missing")?;
    let mut projections = Vec::with_capacity(variants.len());
    let mut failures = Vec::new();
    for variant in variants {
        let mut sample = instantiate(variant, definitions, 0)?;
        repair_semantics(&mut sample);
        let kind =
            sample["projection_kind"].as_str().context("projection kind missing")?.to_owned();
        let mut complementary = Vec::new();
        if kind == "endpoint_detail" {
            let mut other = sample.clone();
            other["payload"]["authentication"]["auth_kind"] = json!("none");
            other["payload"]["authentication"]["active_credential"] = Value::Null;
            sample["payload"]["authentication"]["auth_kind"] = json!("secret");
            sample["payload"]["authentication"]["dormant_credential"] = Value::Null;
            complementary.push(other);
        }
        if kind == "model_profile" {
            let mut running = sample.clone();
            let active = &mut running["payload"]["active_availability_test"];
            active["state"] = json!("running");
            active["stage"] = json!("config");
            active["result"] = Value::Null;
            active["cancellation_reason"] = Value::Null;
            active["completed_at"] = Value::Null;
            running["payload"]["latest_availability_test"] =
                running["payload"]["active_availability_test"].clone();
            complementary.push(running);
            sample["payload"]["active_availability_test"] = Value::Null;
        }
        if kind == "model_profile" || kind == "model_availability_test" {
            let mut cancelled = sample.clone();
            cancel_availability_views(&mut cancelled);
            complementary.push(cancelled);
        }
        for sample in std::iter::once(sample).chain(complementary) {
            let typed: ProjectionChange = match serde_json::from_value(sample.clone()) {
                Ok(typed) => typed,
                Err(error) => {
                    failures.push(format!("{kind}: Rust DTO rejected: {error}"));
                    continue;
                }
            };
            if let Err(error) = typed.validate() {
                failures.push(format!("{kind}: projection binding rejected: {error}"));
                continue;
            }
            let serialized = serde_json::to_value(typed)?;
            if sample != serialized {
                failures.push(format!(
                    "{kind}: Rust fields changed: {}",
                    differing_paths(&sample, &serialized, "")
                ));
                continue;
            }
            projections.push(serialized);
        }
    }
    ensure!(failures.is_empty(), "full projection samples invalid:\n{}", failures.join("\n"));
    Ok(json!({"schema_version": 1, "projections": projections,
        "semantic_empty_fields": {
            "model_profile.payload.active_availability_test.result": "Active availability runs have not produced a final result.",
            "model_profile.payload.active_availability_test.completed_at": "An active availability run has no completion timestamp.",
            "model_profile.payload.active_availability_test.cancellation_reason": "An active availability run has not been cancelled."
        }
    }))
}

fn instantiate(schema: &Value, definitions: &Map<String, Value>, depth: usize) -> Result<Value> {
    ensure!(depth < 80, "projection sample schema recurses too deeply");
    if let Some(reference) = schema["$ref"].as_str() {
        let name = reference.strip_prefix("#/$defs/").context("unsupported schema reference")?;
        if let Some(value) = named_value(name)? {
            return Ok(value);
        }
        return instantiate(&definitions[name], definitions, depth + 1)
            .with_context(|| format!("projection sample type {name}"));
    }
    if let Some(value) = schema.get("const") {
        return Ok(value.clone());
    }
    if let Some(values) = schema["enum"].as_array() {
        return values.first().cloned().context("empty enum");
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(variants) = schema[key].as_array() {
            let selected = variants
                .iter()
                .find(|variant| variant["type"] != "null")
                .context("optional field has no non-null type")?;
            return instantiate(selected, definitions, depth + 1);
        }
    }
    let kind = schema["type"].as_str().or_else(|| {
        schema["type"].as_array()?.iter().filter_map(Value::as_str).find(|value| *value != "null")
    });
    match kind {
        Some("object") => {
            let mut value = Map::new();
            if let Some(properties) = schema["properties"].as_object() {
                for (key, property) in properties {
                    let mut generated = instantiate(property, definitions, depth + 1)?;
                    if key == "hex" {
                        generated = json!("1".repeat(64));
                    }
                    if key == "url" || key == "base_url" {
                        generated = json!("https://fixture.invalid");
                    }
                    if key == "address" || key == "host" {
                        generated = json!("127.0.0.1");
                    }
                    if key == "port" {
                        generated = json!(1080);
                    }
                    value.insert(key.clone(), generated);
                }
            } else if schema["additionalProperties"].is_object() {
                let item = instantiate(&schema["additionalProperties"], definitions, depth + 1)?;
                let key = item["id"].as_str().unwrap_or("fixture").to_owned();
                value.insert(key, item);
            }
            Ok(Value::Object(value))
        }
        Some("array") => Ok(json!([instantiate(&schema["items"], definitions, depth + 1)?])),
        Some("string") => Ok(json!("fixture")),
        Some("integer") => Ok(schema
            .get("minimum")
            .filter(|n| n.as_f64().is_some_and(|v| v > 0.0))
            .cloned()
            .unwrap_or(json!(1))),
        Some("number") => {
            Ok(json!(schema["minimum"].as_f64().filter(|value| *value > 0.0).unwrap_or(1.0)))
        }
        Some("boolean") => Ok(json!(true)),
        Some("null") => bail!("projection optional field selected null"),
        // Arbitrary JSON values do not have a finite field inventory. A populated
        // object keeps them visible without inventing facts for a domain type.
        None => Ok(json!({"fixture": true})),
        other => bail!("unsupported sample schema type {other:?}"),
    }
}

fn named_value(name: &str) -> Result<Option<Value>> {
    if let Some((_, prefix)) =
        COMMON_ID_TYPES.iter().chain(TOOL_ID_TYPES.iter()).find(|(kind, _)| *kind == name)
    {
        return Ok(Some(json!(format!("{prefix}_01ARZ3NDEKTSV4RRFFQ69G5FAV"))));
    }
    Ok(match name {
        "Revision" | "ExpectedRevision" => Some(json!("1")),
        "UtcTimestamp" => Some(json!("2026-10-14T00:00:00Z")),
        "CanonicalSemVer" => Some(json!("1.0.0")),
        "CatalogDecimal" => Some(json!("1")),
        "NormalizedIssueCode" => Some(json!("internal.unexpected")),
        "ErrorCode" => Some(json!("tool.fixture")),
        "EndpointCredentialState" => Some(json!("configured")),
        "ToolCanonicalName" => Some(json!("user.fixture.fixture")),
        "ToolSettingsSnapshotV1" => Some(serde_json::to_value(
            aura_tool_contract::ToolSettingsSnapshotV1::default().with_network_proxy(Some(
                aura_tool_contract::ToolNetworkProxyV1::new(
                    true,
                    aura_tool_contract::ToolNetworkProxyProtocolV1::Http,
                    "127.0.0.1".into(),
                    1080,
                )?,
            ))?,
        )?),
        "ActivityId" => Some(json!("1".repeat(32))),
        "ArgumentJsonPointer" => Some(json!("/fixture")),
        "FixedHttpsOriginV1" => Some(json!("https://fixture.invalid")),
        "EnvironmentVariableNameV1" => Some(json!("FIXTURE_VALUE")),
        "CanonicalJsonSchemaDocumentV1" => {
            Some(serde_json::to_value(aura_tool_contract::CanonicalJsonSchemaDocumentV1::new(
                json!({"type": "object", "properties": {"fixture": {"type": "string"}}}),
            )?)?)
        }
        "ToolSchemaSnapshotV1" => {
            Some(serde_json::to_value(aura_tool_contract::ToolSchemaSnapshotV1::new(
                aura_domain_common::ToolId::from_bytes([0; 16]),
                aura_tool_contract::ToolVersion {
                    definition_revision: aura_domain_common::Revision::INITIAL,
                    semantic_version: Some("1.0.0".parse()?),
                },
                aura_tool_contract::ToolCanonicalName::new("user.fixture.fixture")?,
                "fixture".to_owned(),
                aura_tool_contract::ToolInputSchemaV1::new(
                    json!({"type": "object", "properties": {"fixture": {"type": "string"}}}),
                )?,
                true,
            )?)?)
        }
        "ProviderWireToolCallId" => Some(json!("aura_01arz3ndektsv4rrffq69g5fav")),
        "ContextUsageSummaryView" => Some(json!({
            "estimated": true, "runtime_effective_window_tokens": 100, "used_input_tokens": 4,
            "segments": (["system_and_rules", "tools_and_results", "conversation", "background_and_compaction"].map(|kind| json!({"kind": kind, "label": "fixture", "estimated_tokens": 1})))
        })),
        _ => None,
    })
}

fn repair_semantics(sample: &mut Value) {
    complete_availability_views(sample);
    // Active and retained credentials must be disjoint. Optional fields are
    // populated in both rows while their independent identities remain valid.
    if sample["projection_kind"] == "model_settings" {
        sample["payload"]["dormant_credentials"][0]["credential_id"] =
            json!("crd_01ARZ3NDEKTSV4RRFFQ69G5FAW");
        sample["payload"]["dormant_credentials"][0]["endpoint_id"] =
            json!("end_01ARZ3NDEKTSV4RRFFQ69G5FAW");
    }
}

fn differing_paths(before: &Value, after: &Value, path: &str) -> String {
    if before == after {
        return String::new();
    }
    if let (Some(before), Some(after)) = (before.as_array(), after.as_array()) {
        if before.len() != after.len() {
            return format!("{path}: array length changed");
        }
        return before
            .iter()
            .zip(after)
            .enumerate()
            .filter(|(_, (before, after))| before != after)
            .map(|(index, (before, after))| {
                differing_paths(before, after, &format!("{path}[{index}]"))
            })
            .collect::<Vec<_>>()
            .join(", ");
    }
    if let (Some(before), Some(after)) = (before.as_object(), after.as_object()) {
        return before
            .keys()
            .chain(after.keys())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .map(|key| {
                let before = before.get(key).unwrap_or(&Value::Null);
                let after = after.get(key).unwrap_or(&Value::Null);
                differing_paths(before, after, &format!("{path}.{key}"))
            })
            .filter(|value| !value.is_empty())
            .collect::<Vec<_>>()
            .join(", ");
    }
    format!("{path}: {before} => {after}")
}

fn complete_availability_views(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for item in object.values_mut() {
                complete_availability_views(item);
            }
            if object.get("credential_slot").is_some_and(Value::is_object)
                && let Some(environment) =
                    object.get_mut("environment").and_then(Value::as_array_mut)
                && let Some(entry) = environment.first_mut()
            {
                entry["name"] = json!("FIXTURE_ORDINARY");
            }
            if object.contains_key("endpoint_revision_at_start") && object.contains_key("state") {
                object.insert("state".into(), json!("completed"));
                object.insert("stage".into(), json!("completed"));
                object.insert("cancellation_reason".into(), Value::Null);
            }
        }
        Value::Array(items) => {
            for item in items {
                complete_availability_views(item);
            }
        }
        _ => {}
    }
}

fn cancel_availability_views(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for item in object.values_mut() {
                cancel_availability_views(item);
            }
            if object.contains_key("endpoint_revision_at_start") && object.contains_key("state") {
                object.insert("state".into(), json!("cancelled"));
                object.insert("stage".into(), json!("config"));
                object.insert("result".into(), Value::Null);
                object.insert("cancellation_reason".into(), json!("user_requested"));
            }
        }
        Value::Array(items) => {
            for item in items {
                cancel_availability_views(item);
            }
        }
        _ => {}
    }
}
