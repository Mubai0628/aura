use super::*;
use std::collections::HashSet;

pub(super) fn validate_schema_document(schema: &Value) -> Result<(), ToolSchemaError> {
    if !schema.is_object() {
        return Err(ToolSchemaError::BooleanOrNonObjectSchema);
    }
    let canonical =
        canonicalize_json_value(schema).map_err(|_| ToolSchemaError::Canonicalization)?;
    if canonical.len() > MAX_CANONICAL_SCHEMA_BYTES {
        return Err(ToolSchemaError::CanonicalByteLimit);
    }

    // Compute the key from actual content; caller-provided hashes never select entries.
    let key = CanonicalHashV1::sha256(&canonical);
    if super::policy_cache::contains(key.hex()) {
        return Ok(());
    }

    #[cfg(test)]
    POLICY_VALIDATIONS.with(|count| count.set(count.get() + 1));

    if let Some(meta_schema) = schema.get("$schema") {
        let Some(meta_schema) = meta_schema.as_str() else {
            return Err(ToolSchemaError::UnsupportedMetaSchema);
        };
        if meta_schema != DRAFT_2020_12_URI && meta_schema != format!("{DRAFT_2020_12_URI}#") {
            return Err(ToolSchemaError::UnsupportedMetaSchema);
        }
    }

    let mut counters = SchemaCounters::default();
    inspect_schema(schema, 0, &mut counters)?;
    if counters.nodes > MAX_SCHEMA_NODES
        || counters.refs > MAX_SCHEMA_REFS
        || counters.pattern_bytes > MAX_SCHEMA_PATTERN_TOTAL_BYTES
        || counters.applicator_branches > MAX_SCHEMA_APPLICATOR_BRANCHES
        || counters.enum_values > MAX_SCHEMA_ENUM_VALUES
    {
        return Err(ToolSchemaError::ComplexityLimit);
    }
    validate_reference_chains(schema)?;
    jsonschema::meta::options().validate(schema).map_err(|_| ToolSchemaError::MetaSchemaInvalid)?;
    super::policy_cache::remember(key.hex());
    Ok(())
}

#[derive(Default)]
struct SchemaCounters {
    nodes: usize,
    refs: usize,
    pattern_bytes: usize,
    applicator_branches: usize,
    enum_values: usize,
}

fn inspect_schema(
    schema: &Value,
    depth: usize,
    counters: &mut SchemaCounters,
) -> Result<(), ToolSchemaError> {
    if depth > MAX_SCHEMA_DEPTH {
        return Err(ToolSchemaError::ComplexityLimit);
    }
    if schema.is_boolean() {
        counters.nodes = counters.nodes.saturating_add(1);
        return Ok(());
    }
    let object = schema.as_object().ok_or(ToolSchemaError::BooleanOrNonObjectSchema)?;
    counters.nodes = counters.nodes.saturating_add(1);

    for (keyword, value) in object {
        if !ALLOWED_KEYWORDS.contains(&keyword.as_str()) {
            return Err(ToolSchemaError::UnsupportedKeyword { keyword: keyword.clone() });
        }
        match keyword.as_str() {
            "$ref" => {
                let reference = value.as_str().ok_or(ToolSchemaError::ExternalReference)?;
                if !is_local_reference(reference) {
                    return Err(ToolSchemaError::ExternalReference);
                }
                counters.refs = counters.refs.saturating_add(1);
            }
            "format" => {
                let format = value.as_str().ok_or_else(|| ToolSchemaError::UnsupportedFormat {
                    format: "<non-string>".to_owned(),
                })?;
                if !ALLOWED_FORMATS.contains(&format) {
                    return Err(ToolSchemaError::UnsupportedFormat { format: format.to_owned() });
                }
            }
            "pattern" => inspect_pattern(value, counters)?,
            "patternProperties" => {
                let children = value.as_object().ok_or(ToolSchemaError::MetaSchemaInvalid)?;
                for (pattern, child) in children {
                    inspect_pattern(&Value::String(pattern.clone()), counters)?;
                    inspect_schema(child, depth + 1, counters)?;
                }
            }
            "$defs" | "properties" | "dependentSchemas" => {
                let children = value.as_object().ok_or(ToolSchemaError::MetaSchemaInvalid)?;
                for child in children.values() {
                    inspect_schema(child, depth + 1, counters)?;
                }
            }
            "prefixItems" | "allOf" | "anyOf" | "oneOf" => {
                let children = value.as_array().ok_or(ToolSchemaError::MetaSchemaInvalid)?;
                counters.applicator_branches =
                    counters.applicator_branches.saturating_add(children.len());
                for child in children {
                    inspect_schema(child, depth + 1, counters)?;
                }
            }
            "items"
            | "contains"
            | "additionalProperties"
            | "propertyNames"
            | "not"
            | "if"
            | "then"
            | "else"
            | "unevaluatedItems"
            | "unevaluatedProperties" => inspect_schema(value, depth + 1, counters)?,
            "enum" => {
                counters.enum_values = counters.enum_values.saturating_add(
                    value.as_array().ok_or(ToolSchemaError::MetaSchemaInvalid)?.len(),
                );
            }
            _ => {}
        }
    }
    Ok(())
}

fn inspect_pattern(value: &Value, counters: &mut SchemaCounters) -> Result<(), ToolSchemaError> {
    let pattern = value.as_str().ok_or(ToolSchemaError::InvalidPattern)?;
    if pattern.len() > MAX_SCHEMA_PATTERN_BYTES {
        return Err(ToolSchemaError::InvalidPattern);
    }
    counters.pattern_bytes = counters.pattern_bytes.saturating_add(pattern.len());
    regex::Regex::new(pattern).map_err(|_| ToolSchemaError::InvalidPattern)?;
    Ok(())
}

fn is_local_reference(reference: &str) -> bool {
    reference == "#" || reference.starts_with("#/")
}

fn validate_reference_chains(root: &Value) -> Result<(), ToolSchemaError> {
    fn walk(
        root: &Value,
        schema: &Value,
        depth: usize,
        active: &mut HashSet<String>,
    ) -> Result<(), ToolSchemaError> {
        if depth > MAX_SCHEMA_REF_CHAIN {
            return Err(ToolSchemaError::ReferenceChainLimit);
        }
        let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
            return Ok(());
        };
        if !active.insert(reference.to_owned()) {
            return Err(ToolSchemaError::ReferenceChainLimit);
        }
        let target =
            resolve_local_reference(root, reference).ok_or(ToolSchemaError::ExternalReference)?;
        walk(root, target, depth + 1, active)?;
        active.remove(reference);
        Ok(())
    }

    fn visit(
        root: &Value,
        schema: &Value,
        active: &mut HashSet<String>,
    ) -> Result<(), ToolSchemaError> {
        walk(root, schema, 0, active)?;
        if schema.is_boolean() {
            return Ok(());
        }
        let object = schema.as_object().ok_or(ToolSchemaError::BooleanOrNonObjectSchema)?;
        for (keyword, value) in object {
            match keyword.as_str() {
                "$defs" | "properties" | "patternProperties" | "dependentSchemas" => {
                    if let Some(children) = value.as_object() {
                        for child in children.values() {
                            visit(root, child, active)?;
                        }
                    }
                }
                "prefixItems" | "allOf" | "anyOf" | "oneOf" => {
                    if let Some(children) = value.as_array() {
                        for child in children {
                            visit(root, child, active)?;
                        }
                    }
                }
                "items"
                | "contains"
                | "additionalProperties"
                | "propertyNames"
                | "not"
                | "if"
                | "then"
                | "else"
                | "unevaluatedItems"
                | "unevaluatedProperties" => visit(root, value, active)?,
                _ => {}
            }
        }
        Ok(())
    }

    visit(root, root, &mut HashSet::new())
}

fn resolve_local_reference<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    if reference == "#" {
        return Some(root);
    }
    let pointer = reference.strip_prefix('#')?;
    root.pointer(pointer)
}
