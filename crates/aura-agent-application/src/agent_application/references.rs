//! What a message refers to with "@", resolved against the session's workspace and its other
//! conversations when it is sent.
use super::*;
use aura_agent_domain::{AgentMessageReference, ResolvedMessageReference};
use aura_domain_common::{ConversationHandle, WorkspacePath};
use aura_message_references::{Entry, RECAP_TOKENS, ReferenceSources, SessionSource};

/// The session's workspace and the other conversations the message names, as its references are
/// looked up in them.
struct Sources {
    resolver: Option<Arc<aura_workspace::SecurePathResolver>>,
    conversations: Vec<(ConversationHandle, SessionSource)>,
}

impl ReferenceSources for Sources {
    fn workspace_entry(&self, path: &WorkspacePath) -> anyhow::Result<Option<Entry>> {
        // A path that does not open inside the workspace now is not in it.
        Ok(self
            .resolver
            .as_ref()
            .and_then(|resolver| resolver.resolve_existing(path).ok())
            .map(|found| if found.info.is_directory { Entry::Folder } else { Entry::File }))
    }

    fn conversation(&self, handle: &ConversationHandle) -> anyhow::Result<Option<SessionSource>> {
        Ok(self
            .conversations
            .iter()
            .find(|(named, _)| named == handle)
            .map(|(_, found)| found.clone()))
    }
}

impl AgentApplication {
    /// What a message of `session` refers to: the paths typed after "@" in `text` and the
    /// `chosen` references, resolved off the reactor (it reads the workspace).
    pub(super) async fn resolve_references(
        &self,
        session: SessionId,
        workspace: Option<WorkspaceId>,
        text: &str,
        chosen: &[AgentMessageReference],
    ) -> anyhow::Result<Vec<ResolvedMessageReference>> {
        if chosen.is_empty() && !text.contains('@') {
            return Ok(Vec::new());
        }
        let conversations = self.referred_conversations(session, chosen).await?;
        let resolver = workspace.map(|w| self.execution.workspace.resolver(w)).transpose()?;
        let (text, chosen) = (text.to_owned(), chosen.to_vec());
        tokio::task::spawn_blocking(move || {
            if let Some(resolver) = &resolver {
                resolver.verify_root_identity()?;
            }
            aura_message_references::resolve_message(
                &Sources { resolver, conversations },
                &text,
                &chosen,
            )
        })
        .await?
    }

    /// The other conversations of the workspace that `chosen` names and that are not archived,
    /// each with its title and a recap of where it is now.
    async fn referred_conversations(
        &self,
        session: SessionId,
        chosen: &[AgentMessageReference],
    ) -> anyhow::Result<Vec<(ConversationHandle, SessionSource)>> {
        let named: Vec<ConversationHandle> = chosen
            .iter()
            .filter_map(|reference| match reference {
                AgentMessageReference::Session { session_id } => {
                    Some(ConversationHandle::of(*session_id))
                }
                AgentMessageReference::Conversation { conversation } => Some(conversation.clone()),
                AgentMessageReference::WorkspacePath { .. } => None,
            })
            .collect();
        if named.is_empty() {
            return Ok(Vec::new());
        }
        let found: Vec<_> = self
            .execution
            .other_conversations(session)?
            .into_iter()
            .filter(|other| !other.archived && named.contains(&other.handle))
            .collect();
        let ids: Vec<SessionId> = found.iter().map(|other| other.session_id).collect();
        let lines = self
            .store
            .read(StoreStep::operation("agent.references.recaps"), move |db| {
                ids.iter()
                    .map(|id| aura_persistence_v2::agent::rounds::recap_lines(db, *id))
                    .collect::<Result<Vec<_>, _>>()
            })
            .await?;
        Ok(found
            .into_iter()
            .zip(lines)
            .map(|(other, lines)| {
                let recap = aura_message_references::recap(
                    &lines,
                    RECAP_TOKENS,
                    aura_context_runtime::character_class_tokens,
                );
                // The sidebar shows a conversation not named yet as 新会话.
                let title = if other.title.trim().is_empty() {
                    "新会话".to_owned()
                } else {
                    other.title
                };
                (other.handle, SessionSource { title, recap })
            })
            .collect())
    }
}
