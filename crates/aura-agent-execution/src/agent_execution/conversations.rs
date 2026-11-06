//! The other conversations of a session's workspace by their handles: the ones a message may refer
//! to with "@", and the history tools' refusal to read one that is archived now.
use super::*;

/// Another conversation of a session's workspace.
#[derive(Clone, Debug)]
pub struct OtherConversation {
    pub session_id: SessionId,
    pub handle: ConversationHandle,
    pub title: String,
    pub archived: bool,
}

impl AgentExecution {
    /// The other conversations of the workspace `session` is in.
    pub fn other_conversations(
        &self,
        session: SessionId,
    ) -> anyhow::Result<Vec<OtherConversation>> {
        let projection = self.persistence.workspace_session_projection()?;
        let Some(workspace) =
            projection.sessions.iter().find(|s| s.session_id == session).map(|s| s.workspace_id)
        else {
            return Ok(Vec::new());
        };
        Ok(projection
            .sessions
            .into_iter()
            .filter(|s| s.workspace_id == workspace && s.session_id != session)
            .map(|s| OtherConversation {
                handle: ConversationHandle::of(s.session_id),
                session_id: s.session_id,
                archived: s.state == aura_persistence::SESSION_ARCHIVED,
                title: s.title,
            })
            .collect())
    }
}

/// The refusal of a history call that names a conversation archived now; none for any other call.
pub(super) fn archived_refusal(
    service: &AgentExecution,
    call: &CallContext,
    request: &DispatchRequest,
) -> anyhow::Result<Option<&'static str>> {
    let content = &request.operation.content;
    if !content.action_kind.starts_with("history.") {
        return Ok(None);
    }
    let Some(named) = content
        .action
        .get("conversation")
        .and_then(|v| v.as_str())
        .and_then(ConversationHandle::parse)
    else {
        return Ok(None);
    };
    let archived = service
        .other_conversations(call.snapshot.session_id)?
        .iter()
        .any(|other| other.archived && other.handle == named);
    Ok(archived.then_some(
        "history.conversation_archived: the referred conversation is archived; it can be read \
         again once the user restores it",
    ))
}
