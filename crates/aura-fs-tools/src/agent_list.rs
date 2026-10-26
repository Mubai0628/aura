//! Model-facing pagination over the same authorized catalog used by the file UI.
use crate::{FsToolError, FsToolService};
use aura_domain_common::{SessionId, WorkspaceId};
use aura_workspace_domain::{
    AgentFileListCursor, AgentFileListInput, AgentFileListResult, FsListEntry,
};

impl FsToolService {
    pub fn list_for_agent(
        &self,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        input: AgentFileListInput,
    ) -> Result<AgentFileListResult, FsToolError> {
        if !(1..=100).contains(&input.limit) {
            return Err(FsToolError::InvalidRequest("limit must be in 1..=100".into()));
        }
        if !self
            .persistence
            .workspace_session_projection()?
            .sessions
            .iter()
            .any(|s| s.session_id == session_id && s.workspace_id == workspace_id)
        {
            return Err(FsToolError::ScopeUnavailable);
        }
        let directory = input.path.into_relative();
        let resolver = self.resolver(workspace_id)?;
        let resolved = resolver.resolve_existing(&directory)?;
        if !resolved.info.is_directory {
            return Err(aura_workspace::WorkspaceError::WrongKind { path: directory }.into());
        }
        let after = input.cursor.as_ref().map(|cursor| cursor.after_path.relative().clone());
        let catalog = self.catalog(workspace_id)?;
        let page = input.limit as usize + 1;
        // A folder the index stops above (an ignored one, or one below it) is read from disk.
        let (entries, catalog_revision, directory_revision) =
            match aura_workspace::children_beyond_index(
                &catalog,
                &resolver,
                &directory,
                after.as_ref(),
                page,
            )? {
                Some((entries, revision)) => (entries, revision, revision),
                None => catalog.children_page(&directory, after.as_ref(), page)?,
            };
        if let Some(cursor) = &input.cursor
            && (cursor.directory.relative() != &directory
                || cursor.catalog_revision != directory_revision)
        {
            return Err(FsToolError::InvalidRequest(
                "cursor must belong to this directory and its current contents; restart with cursor=null".into(),
            ));
        }
        let truncated = entries.len() > input.limit as usize;
        let after_examined =
            entries.iter().take(input.limit as usize).next_back().map(|e| e.path.clone());
        let entries = entries
            .into_iter()
            .take(input.limit as usize)
            .filter(|entry| match resolver.resolve_existing(&entry.path) {
                Ok(resolved) => crate::read_scope::allows(&resolved.info),
                Err(_) => {
                    crate::read_scope::unavailable(
                        &resolver.canonical_root().join(entry.path.as_str()),
                    );
                    false
                }
            })
            .map(|e| FsListEntry {
                path: e.path,
                name: e.file_name,
                kind: crate::read::node_kind(e.kind),
                byte_length: e.byte_length,
                ignored: e.ignored,
                generated: e.generated,
                reparse_tag: e.reparse_tag,
            })
            .collect::<Vec<_>>();
        self.record_path_use(workspace_id, &directory)?;
        let next_cursor =
            truncated.then_some(after_examined).flatten().map(|after_path| AgentFileListCursor {
                directory: directory.clone().into(),
                catalog_revision: directory_revision,
                after_path: after_path.into(),
                // A workspace cursor never belongs to an external grant.
                external_grant_id: None,
            });
        Ok(AgentFileListResult {
            skipped_scopes: Vec::new(),
            entries,
            catalog_revision,
            next_cursor,
            truncated,
            external: None,
        })
    }
}
