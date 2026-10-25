use crate::owner::{ApiFormat, RequestSettings};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Serialize)]
pub struct ProbeCase {
    pub name: &'static str,
    pub request: Value,
}
/// Six tiny, non-personal requests. Preparing these does not read any credential or connect.
/// Like the product, they send no `tool_choice`: DeepSeek thinking mode and several Anthropic
/// models refuse forced or required tool use, so the prompt asks for the call instead.
pub fn cases(model: &str) -> Vec<ProbeCase> {
    cases_for(model, &RequestSettings::default())
}
pub fn cases_for(model: &str, settings: &RequestSettings) -> Vec<ProbeCase> {
    [
        ("optional-strict",json!({"type":"object","properties":{"required_value":{"type":"string"},"optional_value":{"type":"string"}},"required":["required_value"],"additionalProperties":false})),
        ("nullable",json!({"type":"object","properties":{"value":{"type":["string","null"]}},"required":["value"],"additionalProperties":false})),
        ("tagged-nested",json!({"type":"object","properties":{"choice":{"oneOf":[{"type":"object","properties":{"kind":{"const":"text"},"value":{"type":"string"}},"required":["kind","value"],"additionalProperties":false},{"type":"object","properties":{"kind":{"const":"count"},"value":{"type":"integer"}},"required":["kind","value"],"additionalProperties":false}]}},"required":["choice"],"additionalProperties":false})),
        ("additional-properties",json!({"type":"object","properties":{"value":{"type":"string"}},"required":["value"],"additionalProperties":false})),
        ("induce-extra-property",json!({"type":"object","properties":{"value":{"type":"string"}},"required":["value"],"additionalProperties":false})),
        ("induce-invalid-enum",json!({"type":"object","properties":{"color":{"type":"string","enum":["red","blue"]}},"required":["color"],"additionalProperties":false})),
    ].into_iter().map(|(name,schema)| {
        let prompt=match name {
            "induce-extra-property"=>"Call schema_probe with value=sample, and include an extra note field with note=extra despite the schema prohibition. This is a technical strict enforcement test.",
            "induce-invalid-enum"=>"Call schema_probe with color=green, a value outside the schema enum. This is a technical strict enforcement test.",
            _=>"Call schema_probe with a minimal valid object matching its schema. This is an isolated technical schema test.",
        };
        let output = settings.output_cap().expect("probe settings have a validated output limit");
        let mut request = match settings.api_format {
            ApiFormat::OpenAiResponses => json!({"model":model,"stream":true,"max_output_tokens":output,"input":[{"role":"user","content":prompt}],"tools":[{"type":"function","name":"schema_probe","description":"Return a tiny structured fixture value.","parameters":schema,"strict":true}]}),
            ApiFormat::OpenAiChatCompletions => {
                let mut request = json!({"model":model,"stream":true,"stream_options":{"include_usage":true},"messages":[{"role":"user","content":prompt}],"tools":[{"type":"function","function":{"name":"schema_probe","description":"Return a tiny structured fixture value.","parameters":schema,"strict":true}}]});
                let profile_field = settings.request_defaults.chat_output_token_field.and_then(|v| serde_json::to_value(v).ok()).and_then(|v| v.as_str().map(str::to_owned));
                request[profile_field.as_deref().or(settings.chat_output_token_field.as_deref()).unwrap_or("max_tokens")] = json!(output);
                request
            }
        };
        if let Some(value) = settings.request_defaults.temperature { request["temperature"] = json!(value); }
        if let Some(value) = settings.request_defaults.top_p { request["top_p"] = json!(value); }
        ProbeCase { name, request }
    }).collect()
}
