use crate::owner::ApiFormat;
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReplayStep {
    Tool { name: String, arguments: Value },
    Final { text: String },
}
pub struct Replay {
    steps: Mutex<VecDeque<ReplayStep>>,
    calls: AtomicU64,
}
impl Replay {
    pub fn new(steps: Vec<ReplayStep>) -> Self {
        Self { steps: Mutex::new(steps.into()), calls: AtomicU64::new(0) }
    }
    pub fn calls(&self) -> u64 {
        self.calls.load(Ordering::SeqCst)
    }
    pub(crate) fn response(
        &self,
        request: &Value,
        format: ApiFormat,
    ) -> Result<(u16, Vec<u8>, String)> {
        let step = self
            .steps
            .lock()
            .map_err(|_| anyhow::anyhow!("eval.replay_lock"))?
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("eval.replay_exhausted"))?;
        let index = self.calls.fetch_add(1, Ordering::SeqCst);
        if format == ApiFormat::OpenAiResponses && index > 0 {
            ensure!(
                request["input"]
                    .as_array()
                    .is_some_and(|items| items.iter().any(|v| v["type"] == "reasoning")),
                "eval.replay_reasoning_history_missing"
            );
        }
        let (delta, finish) = match step {
            ReplayStep::Tool { name, arguments } => {
                let prefix = format!("{}__", name.replace('.', "_"));
                let tools = request["tools"]
                    .as_array()
                    .ok_or_else(|| anyhow::anyhow!("eval.tools_missing"))?;
                let tool = tools
                    .iter()
                    .find_map(|t| {
                        (if format == ApiFormat::OpenAiResponses {
                            t.get("name")
                        } else {
                            t.pointer("/function/name")
                        })
                        .and_then(Value::as_str)
                        .filter(|n| n.starts_with(&prefix))
                    })
                    .ok_or_else(|| anyhow::anyhow!("eval.replay_tool_not_exposed"))?;
                // Envelope 3: the tool's own fields at the top level beside the purpose.
                let Some(fields) = arguments.as_object() else {
                    anyhow::bail!("eval.replay_arguments_object_required");
                };
                let mut call = fields.clone();
                call.insert("aura_purpose".into(), json!("Complete the isolated coding fixture"));
                (
                    json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("eval_{index}"),"type":"function","function":{"name":tool,"arguments":Value::Object(call).to_string()}}]}),
                    "tool_calls",
                )
            }
            ReplayStep::Final { text } => (json!({"role":"assistant","content":text}), "stop"),
        };
        if format == ApiFormat::OpenAiResponses {
            return Ok((
                200,
                super::responses::response(index, &delta),
                "text/event-stream".into(),
            ));
        }
        let frame = json!({"id":format!("eval_response_{index}"),"choices":[{"index":0,"delta":delta,"finish_reason":finish}],"usage":{"prompt_tokens":1000,"completion_tokens":100,"prompt_cache_hit_tokens":400,"total_tokens":1100}});
        Ok((
            200,
            format!("data: {frame}\n\ndata: [DONE]\n\n").into_bytes(),
            "text/event-stream".into(),
        ))
    }
}
