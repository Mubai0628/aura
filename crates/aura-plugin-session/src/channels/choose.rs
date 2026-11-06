//! The conversation a chat's message goes to, always inside the plugin's scope: the chat's own
//! conversation while it is still there, else the one the plugin's chats used last, else the first
//! conversation the scope names, else the newest in the scope's workspaces; with no conversation
//! at all, a new one in the first workspace of the scope.
use aura_domain_common::{SessionId, WorkspaceId};
use aura_plugins::PluginScope;

/// The conversations and workspaces there are now, each conversation with its workspace, in the
/// order they were made.
pub struct Known<'a> {
    pub sessions: &'a [(SessionId, WorkspaceId)],
    pub workspaces: &'a [WorkspaceId],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Use(SessionId),
    /// A new conversation in this workspace.
    Create(WorkspaceId),
    /// Nothing in the scope can take it.
    NoScope,
}

/// The conversation is in the scope by itself or through its workspace.
#[must_use]
pub fn in_scope(scope: &PluginScope, session: SessionId, workspace: WorkspaceId) -> bool {
    scope.sessions.contains(&session) || scope.workspaces.contains(&workspace)
}

/// Where the chat's message goes. `current` is the chat's own conversation, `used` the others the
/// plugin's chats are in, the one used last first.
#[must_use]
pub fn choose(
    scope: &PluginScope,
    known: &Known<'_>,
    current: Option<SessionId>,
    used: &[SessionId],
) -> Choice {
    if scope.is_empty() {
        return Choice::NoScope;
    }
    let usable = |session: SessionId| {
        known
            .sessions
            .iter()
            .find(|(id, workspace)| *id == session && in_scope(scope, *id, *workspace))
            .map(|(id, _)| *id)
    };
    let found = current
        .and_then(usable)
        .or_else(|| used.iter().copied().find_map(usable))
        .or_else(|| scope.sessions.iter().copied().find_map(usable))
        .or_else(|| {
            let mut newest = known.sessions.iter().rev();
            newest.find(|(_, workspace)| scope.workspaces.contains(workspace)).map(|(id, _)| *id)
        });
    match found {
        Some(session) => Choice::Use(session),
        None => scope
            .workspaces
            .iter()
            .find(|workspace| known.workspaces.contains(workspace))
            .map_or(Choice::NoScope, |workspace| Choice::Create(*workspace)),
    }
}
