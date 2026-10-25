//! Core's live
//! record of what a running step is writing into tool calls, put on that step's view. The tool's
//! name is read back through the attempt that asked for it; the file it writes is taken from the
//! start of its input once that much has arrived. Nothing here is stored.
use aura_domain_common::{ModelStepId, TransportAttemptId};
use aura_persistence_core::StoreResult;
use aura_projection_contract::{AgentSessionProjection, AgentToolInputView};
use rusqlite::Connection;
use std::collections::BTreeMap;

/// One tool call a running step is writing, as Core saw it stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveToolInput {
    /// The tool's name on the wire, as the provider streamed it.
    pub wire_name: String,
    pub received_bytes: u64,
    /// The start of the input: enough to name the file a write names first.
    pub head: String,
    pub complete: bool,
}

/// Puts Core's live tool input on the running steps it belongs to. A step that has ended keeps
/// what its sealed output says and shows none of it.
pub fn with_tool_input(
    db: &Connection,
    projection: &mut AgentSessionProjection,
    live: &BTreeMap<ModelStepId, (TransportAttemptId, Vec<LiveToolInput>)>,
) -> StoreResult<()> {
    for message in projection.turns.iter_mut().flat_map(|turn| turn.assistant_messages.iter_mut()) {
        let Some((attempt, calls)) = live.get(&message.model_step_id).filter(|_| message.partial)
        else {
            continue;
        };
        let bindings = crate::model::agent_transport::attempt(db, *attempt)?
            .request_evidence
            .tool_name_bindings;
        message.tool_input = calls
            .iter()
            .map(|call| {
                let name = bindings
                    .iter()
                    .find(|binding| binding.provider_wire_name == call.wire_name)
                    .map_or(call.wire_name.as_str(), |binding| binding.canonical_name.as_str());
                AgentToolInputView {
                    title: crate::tool::activity::tool_title(name).into(),
                    target: named_path(&call.head),
                    received_bytes: call.received_bytes,
                    complete: call.complete,
                }
            })
            .collect();
    }
    Ok(())
}

/// The `"path"` an input names, once the whole string has arrived; `None` before that or when
/// the input names none. Only a JSON string's own escapes are read.
pub(crate) fn named_path(head: &str) -> Option<String> {
    let after = &head[head.find("\"path\"")? + "\"path\"".len()..];
    let after = after.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    let mut path = String::new();
    let mut characters = after.chars();
    while let Some(character) = characters.next() {
        match character {
            '"' => return (!path.is_empty()).then_some(path),
            '\\' => match characters.next()? {
                'n' => path.push('\n'),
                't' => path.push('\t'),
                'u' => {
                    let code: String = characters.by_ref().take(4).collect();
                    path.push(char::from_u32(u32::from_str_radix(&code, 16).ok()?)?);
                }
                other => path.push(other),
            },
            other => path.push(other),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::named_path;

    #[test]
    fn a_path_is_named_once_its_whole_string_has_arrived() {
        assert_eq!(
            named_path(r#"{"path": "src\\main.rs", "content": "fn"#),
            Some(r"src\main.rs".into())
        );
        assert_eq!(named_path(r#"{"content":"x","path":"a b/c.txt"}"#), Some("a b/c.txt".into()));
        assert_eq!(named_path(r#"{"path":"docs/été.md"}"#), Some("docs/été.md".into()));
        // Not yet whole, or none at all.
        assert_eq!(named_path(r#"{"path": "src/ma"#), None);
        assert_eq!(named_path(r#"{"pa"#), None);
        assert_eq!(named_path(r#"{"command":"ls"}"#), None);
        assert_eq!(named_path(r#"{"path": ""}"#), None);
    }
}
