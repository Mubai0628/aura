use anyhow::{Result, ensure};
use serde_json::Value;

/// Collect bounded SSE data frames without treating intermediate telemetry as final usage.
pub(crate) fn values(body: &[u8]) -> Result<Vec<Value>> {
    let text = std::str::from_utf8(body)?;
    if text.trim_start().starts_with('{') {
        return Ok(vec![serde_json::from_str(text)?]);
    }
    let mut output = Vec::new();
    let mut data = Vec::new();
    let mut event = None;
    for line in text.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            if !data.is_empty() {
                let joined = data.join("\n");
                if joined != "[DONE]" {
                    let mut value: Value = serde_json::from_str(&joined)?;
                    if let Some(kind) = event.take() {
                        if let Some(existing) = value.get("type") {
                            ensure!(existing.as_str() == Some(kind), "eval.conflicting_event_type");
                        } else {
                            value["type"] = Value::String(kind.into());
                        }
                    }
                    output.push(value);
                }
                data.clear();
            }
            event = None;
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push(value.strip_prefix(' ').unwrap_or(value));
        } else if let Some(value) = line.strip_prefix("event:") {
            event = Some(value.trim());
        }
    }
    Ok(output)
}
