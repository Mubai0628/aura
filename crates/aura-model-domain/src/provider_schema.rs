//! Flatten local schema references before embedding an argument schema inside
//! the provider envelope. Root-relative refs otherwise point at the wrong root.
use crate::LogicalModelRequestError as Error;
use aura_domain_common::jcs::CanonicalHashV1;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, OnceLock};

type CompiledEnvelope = (Value, CanonicalHashV1);
type EnvelopeCache = BTreeMap<String, Arc<CompiledEnvelope>>;

// Only a pure schema transform is cached. Registry eligibility, frozen bindings,
// credentials and actual execution permissions are still checked independently.
pub(crate) fn compiled_envelope(schema: &Value) -> Result<Arc<CompiledEnvelope>, Error> {
    static CACHE: OnceLock<Mutex<EnvelopeCache>> = OnceLock::new();
    let hash = aura_domain_common::jcs::canonical_sha256(schema)
        .map_err(|_| Error::ProviderArgumentsEnvelopeInvalid)?;
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some(value) =
        cache.lock().map_err(|_| Error::ProviderArgumentsEnvelopeInvalid)?.get(hash.hex()).cloned()
    {
        return Ok(value);
    }
    let compiled =
        Arc::new(crate::provider_arguments_envelope_schema_v1(&inline_local_refs(schema)?)?);
    let mut cache = cache.lock().map_err(|_| Error::ProviderArgumentsEnvelopeInvalid)?;
    if cache.len() >= 64 {
        cache.clear();
    }
    Ok(cache.entry(hash.hex().to_owned()).or_insert(compiled).clone())
}

/// What the model is told about the purpose it must give every call.
const PURPOSE_DESCRIPTION: &str = "Why you are making this call, in one short sentence. The user sees it in approvals and the activity log.";

/// Envelope 3: the package's own root object, plus the required `aura_purpose`.
///
/// A package root is always `type: object` and never declares the reserved name (both are
/// refused when the schema is loaded), so the purpose can sit beside the tool's fields
/// without changing what any of them means.
pub(crate) fn compiled_flat_envelope(schema: &Value) -> Result<Arc<CompiledEnvelope>, Error> {
    static CACHE: OnceLock<Mutex<EnvelopeCache>> = OnceLock::new();
    let hash = aura_domain_common::jcs::canonical_sha256(schema)
        .map_err(|_| Error::ProviderArgumentsEnvelopeInvalid)?;
    let cache = CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));
    if let Some(value) =
        cache.lock().map_err(|_| Error::ProviderArgumentsEnvelopeInvalid)?.get(hash.hex()).cloned()
    {
        return Ok(value);
    }
    let compiled = Arc::new(flat_envelope(&inline_local_refs(schema)?)?);
    let mut cache = cache.lock().map_err(|_| Error::ProviderArgumentsEnvelopeInvalid)?;
    if cache.len() >= 64 {
        cache.clear();
    }
    Ok(cache.entry(hash.hex().to_owned()).or_insert(compiled).clone())
}

fn flat_envelope(package: &Value) -> Result<CompiledEnvelope, Error> {
    use aura_tool_contract::PROVIDER_PURPOSE_FIELD as PURPOSE;
    let Value::Object(mut root) = package.clone() else {
        return Err(Error::ProviderArgumentsEnvelopeInvalid);
    };
    if root.get("type").and_then(Value::as_str) != Some("object") {
        return Err(Error::ProviderArgumentsEnvelopeInvalid);
    }
    let Value::Object(properties) =
        root.entry("properties").or_insert_with(|| Value::Object(Map::new()))
    else {
        return Err(Error::ProviderArgumentsEnvelopeInvalid);
    };
    if properties.contains_key(PURPOSE) {
        return Err(Error::ProviderArgumentsEnvelopeInvalid);
    }
    properties.insert(
        PURPOSE.to_owned(),
        serde_json::json!({
            "type": "string",
            "minLength": 1,
            "maxLength": aura_tool_contract::MAX_TOOL_PURPOSE_BYTES,
            "description": PURPOSE_DESCRIPTION,
        }),
    );
    let Value::Array(required) = root.entry("required").or_insert_with(|| Value::Array(vec![]))
    else {
        return Err(Error::ProviderArgumentsEnvelopeInvalid);
    };
    if required.iter().any(|name| name.as_str() == Some(PURPOSE)) {
        return Err(Error::ProviderArgumentsEnvelopeInvalid);
    }
    required.insert(0, Value::String(PURPOSE.to_owned()));
    let wrapped = Value::Object(root);
    let hash = aura_domain_common::jcs::canonical_sha256(&wrapped)
        .map_err(|_| Error::ProviderArgumentsEnvelopeInvalid)?;
    Ok((wrapped, hash))
}

pub(crate) fn inline_local_refs(schema: &Value) -> Result<Value, Error> {
    fn expand(
        value: &Value,
        root: &Value,
        depth: usize,
        budget: &mut usize,
    ) -> Result<Value, Error> {
        if depth > 64 || *budget == 0 {
            return Err(Error::ProviderArgumentsEnvelopeInvalid);
        }
        *budget -= 1;
        match value {
            Value::Array(values) => {
                values.iter().map(|v| expand(v, root, depth + 1, budget)).collect()
            }
            Value::Object(object) => {
                let mut result = Map::new();
                let base = object
                    .get("$ref")
                    .map(|reference| {
                        let pointer = reference
                            .as_str()
                            .and_then(|s| s.strip_prefix('#'))
                            .ok_or(Error::ProviderArgumentsEnvelopeInvalid)?;
                        let target =
                            root.pointer(pointer).ok_or(Error::ProviderArgumentsEnvelopeInvalid)?;
                        expand(target, root, depth + 1, budget)
                    })
                    .transpose()?;
                for (key, value) in object {
                    if matches!(key.as_str(), "$ref" | "$defs" | "$schema") {
                        continue;
                    }
                    result.insert(key.clone(), expand(value, root, depth + 1, budget)?);
                }
                match base {
                    None => Ok(Value::Object(result)),
                    Some(base) if result.is_empty() => Ok(base),
                    Some(Value::Object(mut base))
                        if result.iter().all(|(key, value)| {
                            base.get(key).is_none_or(|previous| previous == value)
                        }) =>
                    {
                        base.extend(result);
                        Ok(Value::Object(base))
                    }
                    // JSON Schema applies siblings of $ref conjunctively. Keep every
                    // sibling, including constraints that occur after a duplicate key.
                    Some(base) => Ok(serde_json::json!({"allOf":[base,result]})),
                }
            }
            _ => Ok(value.clone()),
        }
    }
    expand(schema, schema, 0, &mut 65536)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn compiled_definitions_are_reused_and_changed_constraints_miss_the_cache() {
        let schema = json!({"type":"object", "properties":{"count":{
            "type":"integer", "maximum":91}}});
        let first = compiled_envelope(&schema).expect("first compilation");
        let second = compiled_envelope(&schema).expect("unchanged schema");
        assert!(Arc::ptr_eq(&first, &second));
        let mut changed = schema;
        changed["properties"]["count"]["maximum"] = json!(13);
        let changed = compiled_envelope(&changed).expect("changed schema");
        assert!(!Arc::ptr_eq(&first, &changed));
        assert_ne!(first.1, changed.1);
        assert_eq!(changed.0["properties"]["arguments"]["properties"]["count"]["maximum"], 13);
        assert!(compiled_envelope(&json!({"$ref":"#"})).is_err());
    }

    #[test]
    fn nested_definitions_keep_constraints_and_invalid_or_recursive_refs_fail() {
        let schema = json!({"type":"object","properties":{"page":{"$ref":"#/$defs/Page"}},
            "$defs":{"Page":{"type":"object","properties":{"limit":{"type":"integer","minimum":1,"maximum":100}},"required":["limit"]}}});
        let value = inline_local_refs(&schema).expect("portable schema");
        assert_eq!(value["properties"]["page"]["properties"]["limit"]["maximum"], 100);
        assert!(!value.to_string().contains("$ref"));
        assert!(inline_local_refs(&json!({"$ref":"#/missing"})).is_err());
        assert!(inline_local_refs(&json!({"$ref":"#"})).is_err());
    }

    #[test]
    fn the_flat_envelope_adds_a_required_purpose_beside_the_tool_fields() {
        let schema = json!({"type":"object","additionalProperties":false,
            "properties":{"path":{"$ref":"#/$defs/Path"},"limit":{"type":"integer"}},
            "required":["path"],"$defs":{"Path":{"type":"string","minLength":1}}});
        let (wrapped, hash) =
            compiled_flat_envelope(&schema).expect("flat envelope").as_ref().clone();
        assert_eq!(wrapped["required"], json!(["aura_purpose", "path"]));
        assert_eq!(wrapped["additionalProperties"], false);
        assert_eq!(wrapped["properties"]["aura_purpose"]["type"], "string");
        assert_eq!(wrapped["properties"]["aura_purpose"]["minLength"], 1);
        assert_eq!(wrapped["properties"]["path"]["minLength"], 1, "references are inlined");
        assert!(wrapped.get("$defs").is_none() && !wrapped.to_string().contains("$ref"));
        assert!(
            wrapped["properties"].get("_aura").is_none()
                && wrapped["properties"].get("arguments").is_none()
        );
        let (_, v2_hash) = compiled_envelope(&schema).expect("v2").as_ref().clone();
        assert_ne!(hash, v2_hash, "a binding records which envelope the model saw");

        let empty = json!({"type":"object","additionalProperties":false});
        let (wrapped, _) = compiled_flat_envelope(&empty).expect("empty tool").as_ref().clone();
        assert_eq!(wrapped["required"], json!(["aura_purpose"]));
        assert_eq!(wrapped["properties"].as_object().map(|p| p.len()), Some(1));

        let colliding = json!({"type":"object","properties":{"aura_purpose":{"type":"string"}}});
        assert!(compiled_flat_envelope(&colliding).is_err());
        assert!(compiled_flat_envelope(&json!({"type":"string"})).is_err());
    }

    #[test]
    fn reference_siblings_preserve_all_intersecting_constraints() {
        let value = inline_local_refs(&json!({"$ref":"#/$defs/Value", "minimum":3,
            "maximum":9, "multipleOf":2, "$defs":{"Value":{"type":"integer","minimum":1}}}))
        .expect("reference with siblings");
        assert_eq!(value["allOf"][0]["minimum"], 1);
        assert_eq!(value["allOf"][1]["minimum"], 3);
        assert_eq!(value["allOf"][1]["multipleOf"], 2);
        assert_eq!(value["allOf"][1]["maximum"], 9);
    }
}
