//! One JSON document in on stdin, one JSON result out on stdout.
use crate::event::HookEvent;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const HOOK_PROTOCOL: &str = "aura.hook.v1";
pub const MAX_RESULT_BYTES: usize = 1024 * 1024;
const MAX_REASON_CHARS: usize = 2000;
const MAX_TEXT_BYTES: usize = 64 * 1024;
const MAX_INJECT_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct HookEnvelopeHook {
    pub name: String,
    pub version: String,
}

/// What a Hook reads on stdin. Core fills `payload` per event; nothing in it is a
/// credential, and paths are the workspace's own.
#[derive(Clone, Debug, Serialize)]
pub struct HookEnvelope {
    pub protocol: &'static str,
    pub event: HookEvent,
    pub hook: HookEnvelopeHook,
    pub session_id: Option<String>,
    pub turn_task_id: Option<String>,
    pub workspace_root: Option<String>,
    pub payload: Value,
}

/// Limited model parameters. A Hook can lower the output budget or change sampling; it can
/// never change the model, the endpoint or the key.
#[derive(Clone, Debug, Default, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HookModelParameters {
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum HookTransform {
    /// The model-visible text of a user message or of the model's reply.
    Text(String),
    /// Added to the `Hook-Injections` section of this step.
    Inject(String),
    Parameters(HookModelParameters),
    /// New tool arguments; Core checks them again from the start.
    Arguments(Value),
    /// The model-visible tool result.
    Result(String),
    /// The model-visible error and recovery advice.
    Error(String),
    /// Added to what compaction must keep.
    Focus(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum HookDecision {
    Continue,
    Transform(HookTransform),
    Block { reason: String },
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HookResultError {
    #[error("结果超过 {MAX_RESULT_BYTES} 字节")]
    TooLarge,
    #[error("结果不是合法的 JSON：{0}")]
    NotJson(String),
    #[error("decision 只能是 continue、transform 或 block")]
    UnknownDecision,
    #[error("{0}")]
    OutOfBounds(String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawResult {
    decision: String,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    inject: Option<String>,
    #[serde(default)]
    parameters: Option<HookModelParameters>,
    #[serde(default)]
    arguments: Option<Value>,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    focus: Option<String>,
}

impl RawResult {
    fn fields(&self) -> Vec<&'static str> {
        [
            ("reason", self.reason.is_some()),
            ("text", self.text.is_some()),
            ("inject", self.inject.is_some()),
            ("parameters", self.parameters.is_some()),
            ("arguments", self.arguments.is_some()),
            ("result", self.result.is_some()),
            ("error", self.error.is_some()),
            ("focus", self.focus.is_some()),
        ]
        .into_iter()
        .filter_map(|(name, present)| present.then_some(name))
        .collect()
    }
}

/// The one way a Hook's stdout becomes a decision. Fields the event does not allow, an
/// unknown field (including anything that looks like an approval) or an oversized value
/// make the whole result invalid, which the failure policy then handles.
pub fn parse_result(event: HookEvent, stdout: &[u8]) -> Result<HookDecision, HookResultError> {
    if stdout.len() > MAX_RESULT_BYTES {
        return Err(HookResultError::TooLarge);
    }
    let text =
        std::str::from_utf8(stdout).map_err(|_| HookResultError::NotJson("不是 UTF-8".into()))?;
    let raw: RawResult =
        serde_json::from_str(text.trim()).map_err(|e| HookResultError::NotJson(e.to_string()))?;
    let fields = raw.fields();
    let bounds = |message: String| Err(HookResultError::OutOfBounds(message));
    match raw.decision.as_str() {
        "continue" if fields.is_empty() => Ok(HookDecision::Continue),
        "continue" => bounds(format!("continue 不能带字段：{}", fields.join("、"))),
        "block" => {
            let reason = raw.reason.as_deref().map(str::trim).unwrap_or_default();
            if fields != ["reason"]
                || reason.is_empty()
                || reason.chars().count() > MAX_REASON_CHARS
            {
                return bounds(format!(
                    "block 只能带 reason，且 reason 不能为空、最长 {MAX_REASON_CHARS} 字"
                ));
            }
            Ok(HookDecision::Block { reason: reason.to_owned() })
        }
        "transform" => {
            let Some(field) = event.transform_field() else {
                return bounds(format!("{} 不能变换", event.name()));
            };
            if fields != [field] {
                return bounds(format!("{} 的 transform 只能带 {field}", event.name()));
            }
            let limited = |value: String, limit: usize| {
                if value.len() > limit {
                    Err(HookResultError::OutOfBounds(format!("{field} 超过 {limit} 字节")))
                } else {
                    Ok(value)
                }
            };
            let transform = match field {
                "text" => {
                    HookTransform::Text(limited(raw.text.unwrap_or_default(), MAX_TEXT_BYTES)?)
                }
                "inject" => HookTransform::Inject(limited(
                    raw.inject.unwrap_or_default(),
                    MAX_INJECT_BYTES,
                )?),
                "parameters" => {
                    let parameters = raw.parameters.unwrap_or_default();
                    if parameters.temperature.is_some_and(|t| !(0.0..=2.0).contains(&t))
                        || parameters.top_p.is_some_and(|p| !(0.0..=1.0).contains(&p))
                        || parameters.max_output_tokens == Some(0)
                    {
                        return bounds("parameters 超出范围：temperature 0–2，top_p 0–1，max_output_tokens 大于 0".into());
                    }
                    HookTransform::Parameters(parameters)
                }
                "arguments" => match raw.arguments {
                    Some(value @ Value::Object(_)) => HookTransform::Arguments(value),
                    _ => return bounds("arguments 必须是 JSON 对象".into()),
                },
                "result" => {
                    HookTransform::Result(limited(raw.result.unwrap_or_default(), MAX_TEXT_BYTES)?)
                }
                "error" => {
                    HookTransform::Error(limited(raw.error.unwrap_or_default(), MAX_TEXT_BYTES)?)
                }
                _ => {
                    HookTransform::Focus(limited(raw.focus.unwrap_or_default(), MAX_INJECT_BYTES)?)
                }
            };
            Ok(HookDecision::Transform(transform))
        }
        _ => Err(HookResultError::UnknownDecision),
    }
}
