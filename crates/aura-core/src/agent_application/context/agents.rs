//! The workspace's AGENTS.md for a model step: the root file always, then those of the folders the
//! conversation's tool calls were authorized for, root to deepest, as the workspace's scope
//! resolver finds them. The folders come from each call's stored operation, so a call made through
//! `tools.call` and a call naming several paths count as the real call does. Each file is read for
//! this step and named with its path and hash. One that cannot be used says why instead, and no
//! file is cut short.
use super::*;
use aura_workspace::{
    AgentsScopeError, AgentsScopeResolver, AgentsScopeTarget, AgentsScopeTargetKind,
    SecurePathResolver,
};

/// The largest AGENTS.md a request carries; a larger one is named, not cut.
const LARGEST: u64 = 128 * 1024;
/// How many of the conversation's newest tool calls a step looks at for the folders worked in.
const CALLS: usize = 64;
/// How many of the newest workspace paths a step looks for nested AGENTS.md files around.
const PATHS: usize = 16;

/// The conversation's tool calls, newest first.
pub(super) fn recent_calls(items: &[LogicalConversationItem]) -> Vec<ToolCallId> {
    items
        .iter()
        .rev()
        .filter_map(|item| match item {
            LogicalConversationItem::Assistant { tool_calls, .. } => Some(tool_calls),
            _ => None,
        })
        .flat_map(|calls| calls.iter().rev().map(|call| call.tool_call_id))
        .take(CALLS)
        .collect()
}

fn note(content: String, revision: Revision) -> ContextFragment {
    fragment(
        content,
        ContextSourceRef::RuntimeState {
            state_kind: "workspace.agents.v1".into(),
            state_revision: revision,
        },
        false,
    )
}

fn fragment(content: String, source_ref: ContextSourceRef, file: bool) -> ContextFragment {
    ContextFragment {
        source_ref,
        source_kind: if file {
            ModelEgressSourceKind::WorkspaceFile
        } else {
            ModelEgressSourceKind::AuraCore
        },
        data_class: if file {
            NetworkDataClass::WorkspaceSource
        } else {
            NetworkDataClass::PublicContent
        },
        authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
        content,
    }
}

impl AgentApplication {
    /// This step's AGENTS.md fragments; the files are read off the async runtime.
    pub(super) async fn workspace_agents(
        &self,
        run_id: AgentRunId,
        workspace_id: WorkspaceId,
        revision: Revision,
        items: &[LogicalConversationItem],
    ) -> Vec<ContextFragment> {
        let calls = recent_calls(items);
        let paths = match self
            .store
            .read(StoreStep::run(run_id, "agent.context.agents_scope"), move |db| {
                aura_persistence_v2::tool::execution::workspace_paths(db, workspace_id, &calls)
            })
            .await
        {
            Ok(paths) => paths.into_iter().filter(|path| !path.is_root()).take(PATHS).collect(),
            Err(error) => return vec![unavailable(&error, revision)],
        };
        let resolver = match self.execution.workspace.resolver(workspace_id) {
            Ok(resolver) => (*resolver).clone(),
            Err(error) => return vec![unavailable(&error, revision)],
        };
        tokio::task::spawn_blocking(move || fragments(resolver, workspace_id, revision, paths))
            .await
            .unwrap_or_else(|error| vec![unavailable(&error, revision)])
    }
}

fn unavailable(error: &dyn std::fmt::Display, revision: Revision) -> ContextFragment {
    note(
        format!(
            "The workspace's AGENTS.md files could not be looked up ({error}); their instructions were not loaded. Read AGENTS.md with fs.read, or tell the user."
        ),
        revision,
    )
}

/// The AGENTS.md fragments of this step, in the order they apply.
fn fragments(
    resolver: SecurePathResolver,
    workspace_id: WorkspaceId,
    revision: Revision,
    paths: Vec<WorkspacePath>,
) -> Vec<ContextFragment> {
    let note = |content: String| note(content, revision);
    let targets = paths
        .into_iter()
        .map(|path| {
            let directory =
                resolver.resolve_existing(&path).is_ok_and(|resolved| resolved.info.is_directory);
            AgentsScopeTarget {
                path,
                kind: if directory {
                    AgentsScopeTargetKind::Directory
                } else {
                    AgentsScopeTargetKind::File
                },
            }
        })
        .collect::<Vec<_>>();
    let outcomes = match AgentsScopeResolver::new(resolver, LARGEST)
        .and_then(|scope| scope.resolve_each(&targets))
    {
        Ok(outcomes) => outcomes,
        Err(error) => return vec![unavailable(&error, revision)],
    };
    let mut fragments = Vec::new();
    if outcomes.first().is_none_or(|outcome| outcome.path.as_str() != "AGENTS.md") {
        fragments.push(note("The workspace has no AGENTS.md at its root.".into()));
    }
    for outcome in outcomes {
        let path = outcome.path.as_str().to_owned();
        let document = match outcome.document {
            Ok(document) => document,
            Err(AgentsScopeError::DocumentTooLarge { .. }) => {
                fragments.push(note(format!(
                    "{path} is larger than {LARGEST} bytes and was not loaded; read it with fs.read when its instructions matter."
                )));
                continue;
            }
            Err(error) => {
                fragments.push(note(format!(
                    "{path} could not be read ({error}); its instructions were not loaded. Read it with fs.read, or tell the user."
                )));
                continue;
            }
        };
        if let Some(kind) = aura_secret_scanner::detect_text(&document.normalized_content) {
            fragments.push(note(format!(
                "{path} contains what looks like a {kind}, so it was not sent. Ask the user to take the credential out of it."
            )));
            continue;
        }
        let short = document.content_hash.hex.get(..16).unwrap_or(&document.content_hash.hex);
        let scope = match path.rsplit_once('/') {
            Some((folder, _)) => format!("for {folder}/ and everything in it"),
            None => "for the whole workspace".to_owned(),
        };
        fragments.push(fragment(
            format!(
                "{path} (sha256 {short}): the workspace's own instructions, {scope}.\n\n{}",
                document.normalized_content
            ),
            ContextSourceRef::WorkspaceFile {
                workspace_id,
                relative_path: path,
                content_hash: document.content_hash,
            },
            true,
        ));
    }
    fragments
}
