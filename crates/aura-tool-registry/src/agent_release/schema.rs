use crate::BuiltinToolDefinitionSetError;

/// Model Tool JSON uses interoperable integers. Large internal counters retain
/// their Rust width; this surface accepts/emits only the exact JSON-safe range.
pub(super) fn schema_value<T: schemars::JsonSchema>()
-> Result<serde_json::Value, BuiltinToolDefinitionSetError> {
    let mut schema = serde_json::to_value(schemars::schema_for!(T))
        .map_err(|_| BuiltinToolDefinitionSetError::InvalidDocument)?;
    if let Some(root) = schema.as_object_mut() {
        root.remove("title");
    }
    constrain(&mut schema);
    Ok(schema)
}
fn constrain(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Array(values) => {
            for v in values {
                constrain(v);
            }
        }
        serde_json::Value::Object(object) => {
            // Defaults describe deserialization; they are not validation rules.
            // Model Tool schemas omit this annotation without making optional
            // properties required or changing the Rust input semantics.
            object.remove("default");
            for value in object.values_mut() {
                constrain(value);
            }
            let bounds = match object.get("format").and_then(serde_json::Value::as_str) {
                Some("uint8") => Some((0, u8::MAX.into())),
                Some("uint32") => Some((0, u32::MAX.into())),
                Some("uint64") => Some((0_i64, 9_007_199_254_740_991_i64)),
                Some("int64") => Some((-9_007_199_254_740_991, 9_007_199_254_740_991)),
                Some("uint16") => Some((0, u16::MAX.into())),
                Some("int32") => Some((i32::MIN.into(), i32::MAX.into())),
                _ => None,
            };
            // IEEE floating-point width is a Rust serialization annotation,
            // not an interoperable JSON Schema format. Preserve number/range
            // constraints and let the domain reject non-finite/out-of-range data.
            if matches!(
                object.get("format").and_then(serde_json::Value::as_str),
                Some("double" | "float")
            ) {
                object.remove("format");
            }
            if let Some((min, max)) = bounds {
                object.remove("format");
                let lower = object
                    .get("minimum")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(min)
                    .max(min);
                let upper = object
                    .get("maximum")
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(max)
                    .min(max);
                object.insert("minimum".into(), lower.into());
                object.insert("maximum".into(), upper.into());
            }
        }
        _ => {}
    }
}
