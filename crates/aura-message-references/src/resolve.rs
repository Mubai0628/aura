//! Resolving a message's references against the session's sources: the paths typed after "@" in
//! its words in the order they appear, then the ones chosen beside them, each once, at most
//! [`MAX_MESSAGE_REFERENCES`], and only what is there now.
use crate::typed::typed_paths;
use aura_agent_domain::{AgentMessageReference, MAX_MESSAGE_REFERENCES, ResolvedMessageReference};
use aura_domain_common::{ConversationHandle, WorkspacePath};

/// What a workspace path is now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    File,
    Folder,
}

impl Entry {
    fn reference(self, path: WorkspacePath) -> ResolvedMessageReference {
        match self {
            Self::File => ResolvedMessageReference::WorkspaceFile { path },
            Self::Folder => ResolvedMessageReference::WorkspaceFolder { path },
        }
    }
}

/// Another conversation a message may refer to, as it is now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionSource {
    pub title: String,
    /// What it was about, within [`crate::RECAP_TOKENS`].
    pub recap: String,
}

/// Where a message's references are looked up: the session's workspace and its conversations.
pub trait ReferenceSources {
    /// What `path` is in the session's workspace now; none when nothing is there.
    fn workspace_entry(&self, path: &WorkspacePath) -> anyhow::Result<Option<Entry>>;

    /// Another conversation of the workspace that is not archived, by its handle; none for any
    /// other.
    fn conversation(&self, _handle: &ConversationHandle) -> anyhow::Result<Option<SessionSource>> {
        Ok(None)
    }
}

/// The chosen references as they are now, in order, each once.
pub fn resolve(
    sources: &dyn ReferenceSources,
    references: &[AgentMessageReference],
) -> anyhow::Result<Vec<ResolvedMessageReference>> {
    resolve_message(sources, "", references)
}

/// What a message refers to: each path typed after "@" in `text` that is in the workspace now, in
/// the order they appear, then each of `chosen`, which must be there. A typed path that is not
/// there is only text.
pub fn resolve_message(
    sources: &dyn ReferenceSources,
    text: &str,
    chosen: &[AgentMessageReference],
) -> anyhow::Result<Vec<ResolvedMessageReference>> {
    let mut found: Vec<ResolvedMessageReference> = Vec::new();
    let mut keep = |reference: ResolvedMessageReference| {
        if !found.contains(&reference) {
            found.push(reference);
        }
    };
    for typed in typed_paths(text) {
        for reading in typed.readings {
            if let Some(entry) = sources.workspace_entry(&reading)? {
                keep(entry.reference(reading));
                break;
            }
        }
    }
    for reference in chosen {
        match reference {
            AgentMessageReference::WorkspacePath { path } => match sources.workspace_entry(path)? {
                Some(entry) => keep(entry.reference(path.clone())),
                None => anyhow::bail!("references.not_found: 当前工作区里没有 {path}"),
            },
            AgentMessageReference::Session { session_id } => {
                keep(conversation(sources, ConversationHandle::of(*session_id))?);
            }
            AgentMessageReference::Conversation { conversation: handle } => {
                keep(conversation(sources, handle.clone())?);
            }
        }
    }
    anyhow::ensure!(
        found.len() <= MAX_MESSAGE_REFERENCES,
        "references.too_many: 一条消息最多引用 {MAX_MESSAGE_REFERENCES} 项"
    );
    Ok(found)
}

/// Another conversation as it is now, named by its handle.
fn conversation(
    sources: &dyn ReferenceSources,
    handle: ConversationHandle,
) -> anyhow::Result<ResolvedMessageReference> {
    let Some(SessionSource { title, recap }) = sources.conversation(&handle)? else {
        anyhow::bail!("references.session_unavailable: 引用的会话不在这个工作区里，或已归档、删除");
    };
    Ok(ResolvedMessageReference::Session { conversation: handle, title, recap })
}
