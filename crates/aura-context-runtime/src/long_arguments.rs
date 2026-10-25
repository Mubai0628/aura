//! A tool call's own long text - a whole
//! game written in one `fs.write`, 136,529 characters in the owner's DeepSeek run - went out again
//! with every later step. Once the call has run, a string argument longer than
//! [`LONG_ARGUMENT_CHARS`] is shown as a line naming its size; the call's result and the file it
//! wrote keep the text. The request right after the call already shows the line, and the call was
//! the previous request's output, not its input, so no cached prefix is lost.
use aura_domain_common::ContentHash;
use aura_model_domain::LogicalConversationItem;
use serde_json::Value;

/// A string argument longer than this is not repeated once its call has run.
pub const LONG_ARGUMENT_CHARS: usize = 32_768;
const NOTE: &str = "[Long argument not repeated:";

/// Shortens the long string arguments of the calls in these items; returns how many calls changed.
pub fn shorten_long_arguments(items: &mut [LogicalConversationItem]) -> usize {
    let mut changed = 0;
    for item in items {
        let LogicalConversationItem::Assistant { tool_calls, .. } = item else {
            continue;
        };
        for call in tool_calls {
            let Ok(mut arguments) = serde_json::from_str::<Value>(&call.arguments_json) else {
                continue;
            };
            if shorten(&mut arguments) {
                call.arguments_json = arguments.to_string();
                call.arguments_hash = ContentHash::sha256(call.arguments_json.as_bytes());
                changed += 1;
            }
        }
    }
    changed
}

/// Every value is visited: one long string does not stop the next from being shortened.
fn shorten_all<'a>(values: impl Iterator<Item = &'a mut Value>) -> bool {
    let mut changed = false;
    for value in values {
        changed |= shorten(value);
    }
    changed
}

fn shorten(value: &mut Value) -> bool {
    match value {
        Value::String(text)
            if text.chars().count() > LONG_ARGUMENT_CHARS && !text.starts_with(NOTE) =>
        {
            *text = format!(
                "{NOTE} {} characters. The call ran with the full text; read its target to see it.]",
                text.chars().count()
            );
            true
        }
        Value::Array(values) => shorten_all(values.iter_mut()),
        Value::Object(fields) => shorten_all(fields.values_mut()),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use aura_domain_common::{EventId, ToolCallId};
    use aura_model_domain::LogicalHistoricalToolCall;

    use super::*;

    fn write(text: &str) -> LogicalConversationItem {
        let arguments = serde_json::json!({ "path": "game.html", "text": text }).to_string();
        LogicalConversationItem::Assistant {
            source_event_id: EventId::new(),
            content: None,
            content_hash: None,
            tool_calls: vec![LogicalHistoricalToolCall {
                tool_call_id: ToolCallId::new(),
                tool_id: None,
                canonical_name: "fs.write".into(),
                arguments_hash: ContentHash::sha256(arguments.as_bytes()),
                arguments_json: arguments,
            }],
            provider_history: None,
        }
    }

    fn arguments(item: &LogicalConversationItem) -> (Value, ContentHash) {
        let LogicalConversationItem::Assistant { tool_calls, .. } = item else { panic!("a call") };
        let call = &tool_calls[0];
        (serde_json::from_str(&call.arguments_json).expect("json"), call.arguments_hash.clone())
    }

    #[test]
    fn a_long_string_argument_is_named_by_its_size_and_a_short_one_stays() {
        let mut items = vec![write(&"x".repeat(136_529)), write("<p>short</p>")];
        let before = arguments(&items[1]);
        assert_eq!(shorten_long_arguments(&mut items), 1);
        let (long, hash) = arguments(&items[0]);
        assert_eq!(long["path"], "game.html");
        let text = long["text"].as_str().expect("text");
        assert!(text.starts_with(NOTE) && text.contains("136529 characters"), "{text}");
        assert_eq!(hash, ContentHash::sha256(long.to_string().as_bytes()));
        assert_eq!(arguments(&items[1]), before, "a short argument is untouched");
        assert_eq!(shorten_long_arguments(&mut items), 0, "a shortened argument stays as it is");
    }
}
