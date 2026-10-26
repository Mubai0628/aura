//! Model-facing filename lookup over the workspace index.
//!
//! `fs.find` answers "where is this file" without walking the disk: the catalog
//! already holds every authorized path, so the result is deterministic, sorted and
//! de-duplicated. It is deliberately not a text search and not a fuzzy match, and
//! it never claims that an unseen name is absent unless the scan actually
//! finished.
use crate::{FsToolError, FsToolService};
use aura_domain_common::{SessionId, WorkspaceId};
use aura_workspace::NameMatchMode;
use aura_workspace_domain::{
    AgentFileFindCursor, AgentFileFindInput, AgentFileFindMatch, AgentFileFindMatchMode,
    AgentFileFindResult,
};

/// A page of matches is bounded, and a single call may ask for several names.
const MAX_FILE_NAMES: usize = 16;
const MAX_LIMIT: u32 = 100;
/// How many index entries one call may examine inside the queried scope, even when
/// the page is still empty. A large workspace therefore answers with a resumable
/// partial page instead of walking everything for one question.
const SCAN_BUDGET: u64 = 16_384;

impl FsToolService {
    pub fn find_for_agent(
        &self,
        workspace_id: WorkspaceId,
        session_id: SessionId,
        input: AgentFileFindInput,
    ) -> Result<AgentFileFindResult, FsToolError> {
        if input.file_names.is_empty() || input.file_names.len() > MAX_FILE_NAMES {
            return Err(FsToolError::InvalidRequest(format!(
                "file_names must contain 1..={MAX_FILE_NAMES} names"
            )));
        }
        if input.file_names.iter().any(|name| name.trim().is_empty()) {
            return Err(FsToolError::InvalidRequest(
                "file_names must not contain a blank name".into(),
            ));
        }
        if !(1..=MAX_LIMIT).contains(&input.limit) {
            return Err(FsToolError::InvalidRequest(format!("limit must be in 1..={MAX_LIMIT}")));
        }
        // A pattern the matcher cannot compile is a field error, never an empty
        // result: "no matches, complete" would tell the model that a file it asked
        // about does not exist. The check is done with the same glob options the
        // index uses, so the accepted language is identical.
        if input.match_mode == AgentFileFindMatchMode::Glob {
            for (index, name) in input.file_names.iter().enumerate() {
                globset::GlobBuilder::new(name)
                    .literal_separator(true)
                    .backslash_escape(false)
                    .case_insensitive(!input.case_sensitive)
                    .build()
                    .map_err(|error| {
                        FsToolError::InvalidRequest(format!(
                            "file_names[{index}] is not a valid glob: {error}"
                        ))
                    })?;
            }
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
        // Core already normalized the model's spelling; this layer only handles the
        // canonical relative value, and the directory has to exist and be one.
        let directory = input.path.into_relative();
        let resolved = self.resolver(workspace_id)?.resolve_existing(&directory)?;
        if !resolved.info.is_directory {
            return Err(aura_workspace::WorkspaceError::WrongKind { path: directory }.into());
        }
        let after = input.cursor.as_ref().map(|cursor| cursor.after_path.relative().clone());
        let page = self.catalog(workspace_id)?.find_names(
            &directory,
            &input.file_names,
            input.recursive,
            match input.match_mode {
                AgentFileFindMatchMode::Exact => NameMatchMode::Exact,
                AgentFileFindMatchMode::Glob => NameMatchMode::Glob,
            },
            input.case_sensitive,
            after.as_ref(),
            input.limit as usize,
            SCAN_BUDGET,
        )?;
        // A cursor belongs to one directory, one query, one matching rule, one
        // scope and one state of that scope. Anything else has to restart, because
        // continuing could otherwise skip or repeat a file without saying so.
        if let Some(cursor) = &input.cursor {
            let cursor_names =
                cursor.file_names.iter().map(|name| name.trim().to_owned()).collect::<Vec<_>>();
            let query_names =
                input.file_names.iter().map(|name| name.trim().to_owned()).collect::<Vec<_>>();
            if cursor.directory.relative() != &directory
                || cursor.scope_revision != page.scope_revision
                || cursor.recursive != input.recursive
                || cursor.match_mode != input.match_mode
                || cursor.case_sensitive != input.case_sensitive
                || cursor_names != query_names
            {
                return Err(FsToolError::InvalidRequest(
                    "cursor must belong to this directory, file_names, recursive scope, match_mode and current scope revision; restart with cursor=null".into(),
                ));
            }
        }
        let truncated = !page.complete;
        let resolver = self.resolver(workspace_id)?;
        let matches = page
            .matches
            .into_iter()
            .filter(|entry| match resolver.resolve_existing(&entry.path) {
                Ok(resolved) => crate::read_scope::allows(&resolved.info),
                Err(_) => {
                    crate::read_scope::unavailable(
                        &resolver.canonical_root().join(entry.path.as_str()),
                    );
                    false
                }
            })
            .map(|entry| AgentFileFindMatch {
                path: entry.path,
                name: entry.file_name,
                kind: entry.kind,
                byte_length: entry.byte_length,
                ignored: entry.ignored,
                generated: entry.generated,
            })
            .collect::<Vec<_>>();
        self.record_path_use(workspace_id, &directory)?;
        // The continuation starts where this page stopped *examining*, so the next
        // call does not walk the index again from the beginning.
        let next_cursor =
            truncated.then(|| page.resume_after.clone()).flatten().map(|after_path| {
                AgentFileFindCursor {
                    directory: directory.clone().into(),
                    file_names: input.file_names.clone(),
                    recursive: input.recursive,
                    match_mode: input.match_mode,
                    case_sensitive: input.case_sensitive,
                    scope_revision: page.scope_revision,
                    after_path: after_path.into(),
                    // A workspace lookup never produces an external cursor.
                    external_grant_id: None,
                    consumed_names: Vec::new(),
                    listing_directory: None,
                    pending: Vec::new(),
                    finished_directories: Vec::new(),
                    partial_listings: Vec::new(),
                    root_identity: None,
                }
            });
        let skipped_scopes = crate::unindexed::skipped(
            self.catalog(workspace_id)?.as_ref(),
            &resolver,
            &directory,
            input.recursive,
        )?;
        Ok(AgentFileFindResult {
            complete: page.complete && skipped_scopes.is_empty(),
            skipped_scopes,
            matches,
            scope_revision: page.scope_revision,
            next_cursor,
            truncated,
            scanned_entries: page.scanned_entries,
            external: None,
        })
    }
}
