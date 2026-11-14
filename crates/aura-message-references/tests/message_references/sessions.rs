//! Another conversation referred to with "@": named by its title with a recap of where it was,
//! at most about a thousand tokens, newest kept first; one that is not there to refer to is
//! refused.
use aura_agent_domain::{AgentMessageReference, ResolvedMessageReference};
use aura_domain_common::{ConversationHandle, SessionId, WorkspacePath};
use aura_message_references::{
    Entry, RECAP_TOKENS, ReferenceSources, SessionSource, recap, resolve_message,
};

struct Sources;

fn earlier() -> SessionId {
    serde_json::from_value(serde_json::json!("ses_01ARZ3NDEKTSV4RRFFQ69G5FAV")).expect("id")
}

impl ReferenceSources for Sources {
    fn workspace_entry(&self, _path: &WorkspacePath) -> anyhow::Result<Option<Entry>> {
        Ok(None)
    }
    fn conversation(&self, handle: &ConversationHandle) -> anyhow::Result<Option<SessionSource>> {
        Ok((*handle == ConversationHandle::of(earlier()))
            .then(|| SessionSource {
                title: "发布清单".into(), recap: "T1: 列出清单".into()
            }))
    }
}

/// One token a character, as Chinese text counts.
fn chars(text: &str) -> u64 {
    text.chars().count() as u64
}

#[test]
fn a_conversation_is_named_by_its_handle_with_its_recap_and_one_not_there_is_refused() {
    let handle = ConversationHandle::of(earlier());
    let resolved = vec![ResolvedMessageReference::Session {
        conversation: handle.clone(),
        title: "发布清单".into(),
        recap: "T1: 列出清单".into(),
    }];
    let chosen = [AgentMessageReference::Session { session_id: earlier() }];
    assert_eq!(resolve_message(&Sources, "接着做", &chosen).expect("resolved"), resolved);
    let receipt = [AgentMessageReference::Conversation { conversation: handle }];
    assert_eq!(resolve_message(&Sources, "", &receipt).expect("by its handle"), resolved);
    let other = [AgentMessageReference::Session { session_id: SessionId::new() }];
    let error = resolve_message(&Sources, "", &other).expect_err("refused");
    assert_eq!(
        error.to_string(),
        "references.session_unavailable: 引用的会话不在这个工作区里，或已归档、删除"
    );
}

#[test]
fn a_recap_keeps_the_newest_lines_within_its_tokens() {
    let lines = ["一".repeat(10), "二".repeat(10), "三".repeat(10)];
    assert_eq!(recap(&lines, 100, chars), lines.join("\n"), "all fit");
    // Room for the last line whole and part of the one before: that part ends with "…".
    assert_eq!(recap(&lines, 18, chars), format!("{}…\n{}", "二".repeat(6), "三".repeat(10)));
    // One long paragraph keeps its start.
    let paragraph = ["长".repeat(50)];
    assert_eq!(recap(&paragraph, 11, chars), format!("{}…", "长".repeat(10)));
    assert_eq!(recap(&[], 100, chars), "");
    assert_eq!(RECAP_TOKENS, 1_000);
}
