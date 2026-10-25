use super::*;

pub(super) fn external_directory_revision(
    directory: &aura_windows_platform::VerifiedFile,
) -> Result<aura_domain_common::Revision> {
    let identity = aura_workspace::domain_identity(&directory.info().identity);
    let modified_ms = directory
        .file()
        .metadata()
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0);
    Ok(aura_fs_tools::external_scope_revision(&identity, modified_ms)?)
}

/// Finds names under one ordinary external directory the policy allowed directly.
///
/// There is no grant, so there is no grant scope to intersect the request with: the
/// model's `recursive` is the reach, and the walk's own budget bounds it. The cursor
/// binds the query, the directory and that reach exactly as the grant case does, and
/// names no grant because none authorized it.
pub(super) fn find_policy_external(
    service: &AgentExecution,
    call: &CallContext,
    path: &aura_security_contract::CanonicalPath,
    input: AgentFileFindInput,
) -> Result<AgentFileFindResult> {
    let session_id = call.snapshot.session_id;
    let directory = input.path.into_relative();
    let (_path, handle) =
        open_authorized_external(call, aura_windows_platform::NativeFileAccess::Read)?;
    if !handle.info().is_directory {
        anyhow::bail!(
            "agent.external_find_target_not_a_directory: the authorized target is a file, not a \
             directory"
        );
    }
    let root_identity =
        serde_json::to_string(&aura_workspace::domain_identity(&handle.info().identity)).ok();
    let root_revision = external_directory_revision(&handle)?;
    if let Some(cursor) = &input.cursor {
        let cursor_names =
            cursor.file_names.iter().map(|name| name.trim().to_owned()).collect::<Vec<_>>();
        let query_names =
            input.file_names.iter().map(|name| name.trim().to_owned()).collect::<Vec<_>>();
        if (cursor.root_identity.is_some() && cursor.root_identity != root_identity)
            || cursor.scope_revision != root_revision
            || cursor.external_grant_id.is_some()
            || cursor.directory.relative() != &directory
            || cursor.recursive != input.recursive
            || cursor.match_mode != input.match_mode
            || cursor.case_sensitive != input.case_sensitive
            || cursor_names != query_names
        {
            anyhow::bail!(
                "agent.external_find_cursor_mismatch: the cursor belongs to another query or \
                 another authorized directory; restart with cursor=null"
            );
        }
    }
    let after = input.cursor.as_ref().map(|cursor| cursor.after_path.relative().clone());
    let consumed_names =
        input.cursor.as_ref().map(|cursor| cursor.consumed_names.clone()).unwrap_or_default();
    let listing_directory = input
        .cursor
        .as_ref()
        .and_then(|cursor| cursor.listing_directory.as_ref().map(|path| path.relative().clone()));
    let cancel = external_search_cancel(call);
    let found = service.fs.find_external(
        aura_fs_tools::FsExternalFindRequest {
            target: aura_fs_tools::ExternalFindTarget {
                grant_id: None,
                session_id,
                display_path: path.display.clone(),
                relative_path: WorkspacePath::parse(WorkspacePath::ROOT)?,
                directory: handle,
            },
            file_names: input.file_names.clone(),
            recursive: input.recursive,
            match_mode: match input.match_mode {
                AgentFileFindMatchMode::Exact => aura_fs_tools::ExternalNameMatch::Exact,
                AgentFileFindMatchMode::Glob => aura_fs_tools::ExternalNameMatch::Glob,
            },
            case_sensitive: input.case_sensitive,
            after: after.clone(),
            consumed_names,
            listing_directory,
            pending: input
                .cursor
                .as_ref()
                .map(|cursor| {
                    cursor
                        .pending
                        .iter()
                        .map(|entry| aura_fs_tools::FsExternalPending {
                            relative_path: entry.relative_path.relative().clone(),
                            name: entry.name.clone(),
                            kind: match entry.kind {
                                CatalogEntryKind::File => FsNodeKind::File,
                                CatalogEntryKind::Directory => FsNodeKind::Directory,
                                CatalogEntryKind::ReparsePoint => FsNodeKind::ReparsePoint,
                            },
                            byte_length: entry.byte_length,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            finished_directories: input
                .cursor
                .as_ref()
                .map(|cursor| {
                    cursor.finished_directories.iter().map(|path| path.relative().clone()).collect()
                })
                .unwrap_or_default(),
            partial_listings: input
                .cursor
                .as_ref()
                .map(|cursor| {
                    cursor
                        .partial_listings
                        .iter()
                        .map(|item| aura_fs_tools::FsExternalPartialListing {
                            directory: item.directory.relative().clone(),
                            consumed_names: item.consumed_names.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            limit: input.limit,
            scan_budget: EXTERNAL_FIND_SCAN_BUDGET,
        },
        cancel.as_ref(),
    )?;
    let matches = found
        .matches
        .into_iter()
        .map(|entry| AgentFileFindMatch {
            path: entry.relative_path,
            name: entry.name,
            kind: match entry.kind {
                FsNodeKind::File => CatalogEntryKind::File,
                FsNodeKind::Directory => CatalogEntryKind::Directory,
                FsNodeKind::ReparsePoint => CatalogEntryKind::ReparsePoint,
            },
            byte_length: entry.byte_length,
            ignored: false,
            generated: false,
        })
        .collect::<Vec<_>>();
    let next_cursor = found
        .truncated
        .then(|| {
            let after_path = found.resume_after.clone().unwrap_or_else(|| directory.clone());
            Some(after_path)
        })
        .flatten()
        .map(|after_path| AgentFileFindCursor {
            directory: directory.clone().into(),
            file_names: input.file_names.clone(),
            recursive: input.recursive,
            match_mode: input.match_mode,
            case_sensitive: input.case_sensitive,
            scope_revision: found.scope_revision,
            after_path: after_path.into(),
            external_grant_id: None,
            consumed_names: found.consumed_names.clone(),
            listing_directory: found.listing_directory.clone().map(Into::into),
            pending: found
                .pending
                .iter()
                .map(|entry| aura_workspace_domain::AgentFileFindPending {
                    relative_path: entry.relative_path.clone().into(),
                    name: entry.name.clone(),
                    kind: match entry.kind {
                        FsNodeKind::File => CatalogEntryKind::File,
                        FsNodeKind::Directory => CatalogEntryKind::Directory,
                        FsNodeKind::ReparsePoint => CatalogEntryKind::ReparsePoint,
                    },
                    byte_length: entry.byte_length,
                })
                .collect(),
            finished_directories: found
                .finished_directories
                .iter()
                .cloned()
                .map(Into::into)
                .collect(),
            partial_listings: found
                .partial_listings
                .iter()
                .map(|item| aura_workspace_domain::AgentFileFindPartialListing {
                    directory: item.directory.clone().into(),
                    consumed_names: item.consumed_names.clone(),
                })
                .collect(),
            root_identity: root_identity.clone(),
        });
    Ok(AgentFileFindResult {
        skipped_scopes: Vec::new(),
        matches,
        scope_revision: found.scope_revision,
        next_cursor,
        truncated: found.truncated,
        scanned_entries: found.scanned_entries,
        complete: found.complete,
        external: Some(FsExternalFindTarget { grant_id: None, display_path: found.display_path }),
    })
}
/// How many external entries one text search may examine, and how much it may read.
const EXTERNAL_SEARCH_SCAN_BUDGET: u64 = 4_096;
/// Files above this size are reported as skipped rather than read.
const EXTERNAL_SEARCH_MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
/// Total file bytes one external search may read.
const EXTERNAL_SEARCH_BYTE_BUDGET: u64 = 32 * 1024 * 1024;

/// Searches text under one authorized external directory, through the grant.
pub(super) fn search_external_grant(
    service: &AgentExecution,
    call: &CallContext,
    grant_id: aura_domain_common::ExternalGrantId,
    input: AgentFileSearchInput,
) -> Result<FsSearchTextResult> {
    let session_id = call.snapshot.session_id;
    let directory = input.root.clone().into_relative();
    let record = service.external_grants.record(grant_id)?;
    let display_path = join_display(&record.selected_root.display, &directory);
    let resolved = service.external_grants.resolve_existing(
        grant_id,
        session_id,
        &directory,
        aura_permission_domain::ExternalGrantCapability::Read,
        aura_windows_platform::NativeFileAccess::Read,
        call.operation.content.identity.correlation_id,
    )?;
    // A search reads the target's entries and their contents, so it may only start where
    // the grant covers the contents. A direct child of a non-recursive root is an
    // authorized object, but searching inside it would read grandchildren.
    if !crate::external_grants::contents_are_authorized(&record, &directory) {
        anyhow::bail!(
            "agent.external_scope_contents: the directory is authorized as an object, but its \
             contents are outside this session's authorized scope; the grant covers {} and its \
             direct children only",
            record.selected_root.display
        );
    }
    external_search(
        service,
        call,
        Some(grant_id),
        display_path,
        directory,
        resolved.file,
        // The grant's own depth decides how far a search may reach inside it.
        record.recursive_scope,
        input,
    )
}

/// Searches text under one ordinary external directory the policy allowed directly.
pub(super) fn search_policy_external(
    service: &AgentExecution,
    call: &CallContext,
    path: &aura_security_contract::CanonicalPath,
    input: AgentFileSearchInput,
) -> Result<FsSearchTextResult> {
    let directory = input.root.clone().into_relative();
    let (_path, handle) =
        open_authorized_external(call, aura_windows_platform::NativeFileAccess::Read)?;
    external_search(
        service,
        call,
        None,
        path.display.clone(),
        directory,
        handle,
        // No grant scope to intersect with: the search covers the tree it was pointed at.
        true,
        input,
    )
}

/// The one external search: bounded on the real work, and never a false "no match".
#[allow(clippy::too_many_arguments)]
pub(super) fn external_search(
    service: &AgentExecution,
    call: &CallContext,
    grant_id: Option<aura_domain_common::ExternalGrantId>,
    display_path: String,
    relative_path: WorkspacePath,
    handle: aura_windows_platform::VerifiedFile,
    recursive: bool,
    input: AgentFileSearchInput,
) -> Result<FsSearchTextResult> {
    ensure!(
        input.path_glob.is_none(),
        "agent.external_search_glob_unsupported: a path glob is a workspace index \
         selector and is not available for an external directory"
    );
    let session_id = call.snapshot.session_id;
    let limit = input.limit;
    let cancel = external_search_cancel(call);
    let found = service.fs.search_external(
        aura_fs_tools::FsExternalSearchRequest {
            target: aura_fs_tools::ExternalSearchTarget {
                grant_id,
                session_id,
                display_path: display_path.clone(),
                relative_path,
                directory: handle,
            },
            pattern: input.pattern,
            recursive,
            limit,
            scan_budget: EXTERNAL_SEARCH_SCAN_BUDGET,
            max_file_bytes: EXTERNAL_SEARCH_MAX_FILE_BYTES,
            byte_budget: EXTERNAL_SEARCH_BYTE_BUDGET,
        },
        cancel.as_ref(),
    )?;
    let matches = found
        .matches
        .into_iter()
        .map(|found| FsSearchTextMatch {
            path: found.relative_path,
            line_index: found.line_number.saturating_sub(1),
            column_character_offset: found.column_character_offset,
            match_character_length: found.match_character_length,
            excerpt: found.line_text,
            excerpt_start_character_offset: 0,
            excerpt_truncated_before: false,
            excerpt_truncated_after: found.line_truncated,
            source: FileStorageSource::Disk,
            logical_hash: found.logical_hash,
            buffer_revision: None,
            ignored: false,
            generated: false,
        })
        .collect::<Vec<_>>();
    let page_full = matches.len() >= limit as usize;
    let completion = if found.complete {
        FsSearchCompletion::Complete
    } else {
        FsSearchCompletion::Partial {
            reasons: vec![if found.skipped_too_large > 0 {
                FsSearchIncompleteReason::FileTooLarge
            } else if found.skipped_not_text > 0 {
                FsSearchIncompleteReason::UnsupportedEncoding
            } else if page_full {
                FsSearchIncompleteReason::ResultLimit
            } else {
                FsSearchIncompleteReason::ScanByteBudget
            }],
        }
    };
    // An external tree has no workspace index, so the revision reported is the
    // workspace's own — which this search does not change — and the external facts say
    // which directory the matches are relative to.
    let catalog_revision = service.workspace.catalog(call.snapshot.workspace_id)?.revision()?;
    Ok(FsSearchTextResult {
        skipped_scopes: Vec::new(),
        catalog_revision,
        matches,
        completion,
        candidate_file_count: found.scanned_entries,
        scanned_file_count: found.scanned_file_count,
        scanned_bytes: found.scanned_bytes,
        skipped_file_count: found.skipped_too_large.saturating_add(found.skipped_not_text),
        external: Some(FsExternalSearchTarget { grant_id: found.grant_id, display_path }),
    })
}
/// The cancellation token of this call, when one was registered for its correlation.
///
/// The same registry the workspace text search uses, so a cancelled call stops either
/// kind of scan by the same mechanism.
pub(super) fn external_search_cancel(call: &CallContext) -> Option<aura_fs_tools::SearchCancel> {
    aura_fs_tools::search_flight(call.operation.content.identity.correlation_id)
}
/// How many external entries one filename lookup may examine.
const EXTERNAL_FIND_SCAN_BUDGET: u64 = 16_384;

/// Finds names under one authorized external directory.
///
/// The grant is resolved on the call path, so the decision uses the live grant. An
/// external tree has no index, so the lookup walks the real directory through the
/// file layer: one bounded walk in path order, a continuation that resumes where the
/// previous call stopped examining, and a reparse point that is reported but never
/// followed.
pub(super) fn find_external(
    service: &AgentExecution,
    call: &CallContext,
    grant_id: aura_domain_common::ExternalGrantId,
    input: AgentFileFindInput,
) -> Result<AgentFileFindResult> {
    let session_id = call.snapshot.session_id;
    let directory = input.path.into_relative();
    let record = service.external_grants.record(grant_id)?;
    let display_path = join_display(&record.selected_root.display, &directory);
    let resolved = service.external_grants.resolve_existing(
        grant_id,
        session_id,
        &directory,
        aura_permission_domain::ExternalGrantCapability::Read,
        aura_windows_platform::NativeFileAccess::Read,
        call.operation.content.identity.correlation_id,
    )?;
    if !resolved.file.info().is_directory {
        anyhow::bail!(
            "agent.external_find_target_not_a_directory: the authorized target is a file, not a \
             directory"
        );
    }
    let root_identity =
        serde_json::to_string(&aura_workspace::domain_identity(&resolved.file.info().identity))
            .ok();
    let root_revision = external_directory_revision(&resolved.file)?;
    let recursive = input.recursive && service.external_grants.record(grant_id)?.recursive_scope;
    if let Some(cursor) = &input.cursor {
        let cursor_names =
            cursor.file_names.iter().map(|name| name.trim().to_owned()).collect::<Vec<_>>();
        let query_names =
            input.file_names.iter().map(|name| name.trim().to_owned()).collect::<Vec<_>>();
        if (cursor.root_identity.is_some() && cursor.root_identity != root_identity)
            || cursor.scope_revision != root_revision
            || cursor.external_grant_id != Some(grant_id)
            || cursor.directory.relative() != &directory
            || cursor.recursive != recursive
            || cursor.match_mode != input.match_mode
            || cursor.case_sensitive != input.case_sensitive
            || cursor_names != query_names
        {
            anyhow::bail!(
                "agent.external_find_cursor_mismatch: the cursor belongs to another query or \
                 another authorized directory; restart with cursor=null"
            );
        }
    }
    // The walk reads the target's entries, so the target has to be a directory whose
    // contents the grant covers. A direct child of a non-recursive root may be named, but
    // its own entries are the next level down and are not authorized.
    if !crate::external_grants::contents_are_authorized(&record, &directory) {
        anyhow::bail!(
            "agent.external_scope_contents: the directory is authorized as an object, but its \
             contents are outside this session's authorized scope; the grant covers {} and its \
             direct children only",
            record.selected_root.display
        );
    }
    let after = input.cursor.as_ref().map(|cursor| cursor.after_path.relative().clone());
    let consumed_names =
        input.cursor.as_ref().map(|cursor| cursor.consumed_names.clone()).unwrap_or_default();
    let listing_directory = input
        .cursor
        .as_ref()
        .and_then(|cursor| cursor.listing_directory.as_ref().map(|path| path.relative().clone()));
    // The model's `recursive` says how far it wants to look; the grant says how far it
    // may. The walk gets the intersection, so a recursive request cannot turn a
    // non-recursive directory authorization into a descendant search. The effective
    // value is also what the cursor is bound to below, so a continuation stays inside
    // the same scope the first page used.
    let cancel = external_search_cancel(call);
    let found = service.fs.find_external(
        aura_fs_tools::FsExternalFindRequest {
            target: aura_fs_tools::ExternalFindTarget {
                grant_id: Some(grant_id),
                session_id,
                display_path: display_path.clone(),
                relative_path: directory.clone(),
                directory: resolved.file,
            },
            file_names: input.file_names.clone(),
            recursive,
            match_mode: match input.match_mode {
                AgentFileFindMatchMode::Exact => aura_fs_tools::ExternalNameMatch::Exact,
                AgentFileFindMatchMode::Glob => aura_fs_tools::ExternalNameMatch::Glob,
            },
            case_sensitive: input.case_sensitive,
            after: after.clone(),
            consumed_names,
            listing_directory,
            pending: input
                .cursor
                .as_ref()
                .map(|cursor| {
                    cursor
                        .pending
                        .iter()
                        .map(|entry| aura_fs_tools::FsExternalPending {
                            relative_path: entry.relative_path.relative().clone(),
                            name: entry.name.clone(),
                            kind: match entry.kind {
                                CatalogEntryKind::File => FsNodeKind::File,
                                CatalogEntryKind::Directory => FsNodeKind::Directory,
                                CatalogEntryKind::ReparsePoint => FsNodeKind::ReparsePoint,
                            },
                            byte_length: entry.byte_length,
                        })
                        .collect()
                })
                .unwrap_or_default(),
            finished_directories: input
                .cursor
                .as_ref()
                .map(|cursor| {
                    cursor.finished_directories.iter().map(|path| path.relative().clone()).collect()
                })
                .unwrap_or_default(),
            partial_listings: input
                .cursor
                .as_ref()
                .map(|cursor| {
                    cursor
                        .partial_listings
                        .iter()
                        .map(|item| aura_fs_tools::FsExternalPartialListing {
                            directory: item.directory.relative().clone(),
                            consumed_names: item.consumed_names.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            limit: input.limit,
            scan_budget: EXTERNAL_FIND_SCAN_BUDGET,
        },
        cancel.as_ref(),
    )?;
    let matches = found
        .matches
        .into_iter()
        .map(|entry| AgentFileFindMatch {
            path: entry.relative_path,
            name: entry.name,
            kind: match entry.kind {
                FsNodeKind::File => CatalogEntryKind::File,
                FsNodeKind::Directory => CatalogEntryKind::Directory,
                FsNodeKind::ReparsePoint => CatalogEntryKind::ReparsePoint,
            },
            byte_length: entry.byte_length,
            // An external directory has no workspace ignore rules or generated-path
            // classification, so both flags stay false rather than being invented.
            ignored: false,
            generated: false,
        })
        .collect::<Vec<_>>();
    let next_cursor = found
        .truncated
        .then(|| {
            let after_path = found.resume_after.clone().unwrap_or_else(|| directory.clone());
            Some(after_path)
        })
        .flatten()
        .map(|after_path| {
            AgentFileFindCursor {
                directory: directory.clone().into(),
                file_names: input.file_names.clone(),
                // The effective range, not the requested one: a non-recursive grant turns a
                // recursive request into a walk of the grant root's direct children, and the
                // continuation has to be validated against the scope the first page really
                // used. Writing the requested value here made the next page reject itself.
                recursive,
                match_mode: input.match_mode,
                case_sensitive: input.case_sensitive,
                scope_revision: found.scope_revision,
                after_path: after_path.into(),
                external_grant_id: Some(grant_id),
                consumed_names: found.consumed_names.clone(),
                listing_directory: found.listing_directory.clone().map(Into::into),
                pending: found
                    .pending
                    .iter()
                    .map(|entry| aura_workspace_domain::AgentFileFindPending {
                        relative_path: entry.relative_path.clone().into(),
                        name: entry.name.clone(),
                        kind: match entry.kind {
                            FsNodeKind::File => CatalogEntryKind::File,
                            FsNodeKind::Directory => CatalogEntryKind::Directory,
                            FsNodeKind::ReparsePoint => CatalogEntryKind::ReparsePoint,
                        },
                        byte_length: entry.byte_length,
                    })
                    .collect(),
                finished_directories: found
                    .finished_directories
                    .iter()
                    .cloned()
                    .map(Into::into)
                    .collect(),
                partial_listings: found
                    .partial_listings
                    .iter()
                    .map(|item| aura_workspace_domain::AgentFileFindPartialListing {
                        directory: item.directory.clone().into(),
                        consumed_names: item.consumed_names.clone(),
                    })
                    .collect(),
                root_identity: root_identity.clone(),
            }
        });
    Ok(AgentFileFindResult {
        skipped_scopes: Vec::new(),
        matches,
        scope_revision: found.scope_revision,
        next_cursor,
        truncated: found.truncated,
        scanned_entries: found.scanned_entries,
        complete: found.complete,
        external: Some(FsExternalFindTarget {
            grant_id: Some(grant_id),
            display_path: found.display_path,
        }),
    })
}
///
/// The grant is resolved on the call path exactly as it is for a read, so the
/// decision uses the live grant rather than the classification that built the
/// operation. An external tree has no index: the listing reads the directory itself,
/// one level, bounded by the model's limit, and a continuation has to belong to the
/// same grant, the same directory and the same listing state.
/// Lists one ordinary external directory the session policy allowed directly.
///
/// The absolute path is the operation's own resource; the handle is opened here and the
/// directory identity re-checked by the listing itself. There is no grant, so the cursor
/// and the result both say so.
pub(super) fn list_policy_external(
    service: &AgentExecution,
    call: &CallContext,
    path: &aura_security_contract::CanonicalPath,
    input: AgentFileListInput,
) -> Result<AgentFileListResult> {
    let session_id = call.snapshot.session_id;
    let directory = input.path.into_relative();
    let (_path, handle) =
        open_authorized_external(call, aura_windows_platform::NativeFileAccess::Read)?;
    if !handle.info().is_directory {
        anyhow::bail!(
            "agent.external_list_target_not_a_directory: the authorized target is a file, not a \
             directory"
        );
    }
    let listed = service.fs.list_external(aura_fs_tools::FsExternalListRequest {
        target: aura_fs_tools::ExternalListTarget {
            grant_id: None,
            session_id,
            display_path: path.display.clone(),
            relative_path: WorkspacePath::parse(WorkspacePath::ROOT)?,
            directory: handle,
        },
        limit: input.limit,
        after_name: input
            .cursor
            .as_ref()
            .map(|cursor| cursor.after_path.relative().as_str().to_owned()),
    })?;
    if let Some(cursor) = &input.cursor
        && (cursor.external_grant_id.is_some()
            || cursor.directory.relative() != &directory
            || cursor.catalog_revision != listed.scope_revision)
    {
        anyhow::bail!(
            "agent.external_list_cursor_stale: the directory changed or the cursor belongs to \
             another listing; restart with cursor=null"
        );
    }
    let entries = listed
        .entries
        .into_iter()
        .map(|entry| FsListEntry {
            path: entry.relative_path,
            name: entry.name,
            kind: entry.kind,
            byte_length: entry.byte_length,
            ignored: false,
            generated: false,
            reparse_tag: entry.reparse_tag,
        })
        .collect::<Vec<_>>();
    let next_cursor =
        listed.truncated.then(|| entries.last()).flatten().map(|last| AgentFileListCursor {
            directory: directory.clone().into(),
            catalog_revision: listed.scope_revision,
            after_path: last.path.clone().into(),
            external_grant_id: None,
        });
    Ok(AgentFileListResult {
        skipped_scopes: Vec::new(),
        entries,
        catalog_revision: listed.scope_revision,
        next_cursor,
        truncated: listed.truncated,
        external: Some(FsExternalListTarget { grant_id: None, display_path: listed.display_path }),
    })
}
