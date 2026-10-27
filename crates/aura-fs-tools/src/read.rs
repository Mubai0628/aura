use std::sync::Arc;

use aura_artifact_contract::{ArtifactAccess, ArtifactScope, CaptureWorkspaceFileResult};
use aura_domain_common::WorkspacePath;
use aura_workspace::{DecodedText, DiskTextSnapshot, decode_text, decode_text_with_encoding};
use aura_workspace_domain::{
    CaptureWorkspaceFileRequest, CatalogEntryKind, FileStorageSource, FsFindPathsRequest,
    FsGlobRequest, FsGlobResult, FsHashRequest, FsHashResult, FsInspectRequest, FsInspectResult,
    FsListEntry, FsListRequest, FsListResult, FsNodeKind, FsReadBinaryRangeRequest,
    FsReadBinaryRangeResult, FsReadTextRequest, FsReadTextResult, FsSearchTextRequest,
    FsSearchTextResult, FsStatRequest, FsStatResult, LargeFileReadMode, PathSearchResult,
    TextEncoding, TextReadRange,
};

use crate::inspect::{inspect_bytes, render_binary};
use crate::{FsToolError, FsToolService};
use aura_persistence::ArtifactWriteRequest;

/// The entries one page of a folder listing holds, what the file tree shows at once. A folder
/// with more is read page by page with `after`, and a page inspects only its own entries.
pub const LISTED: usize = 1_000;

/// Whether `path` is a direct child of `folder`.
fn is_child(folder: &WorkspacePath, path: &WorkspacePath) -> bool {
    match path.as_str().rsplit_once('/') {
        Some((parent, _)) => parent == folder.as_str(),
        None => folder.is_root(),
    }
}

impl FsToolService {
    pub fn stat(&self, request: FsStatRequest) -> Result<FsStatResult, FsToolError> {
        let resolver = self.resolver(request.workspace_id)?;
        let path = &request.target.path;
        let resolved = resolver.resolve_existing(path)?;
        let catalog_entry = self.catalog(request.workspace_id)?.entry(path)?;
        let kind = if resolved.info.is_reparse_point {
            FsNodeKind::ReparsePoint
        } else if resolved.info.is_directory {
            FsNodeKind::Directory
        } else {
            FsNodeKind::File
        };
        let snapshot = if kind == FsNodeKind::File {
            Some(self.buffers().logical_file_snapshot(&resolver, request.session_id, path)?)
        } else {
            None
        };
        self.record_path_use(request.workspace_id, path)?;
        Ok(FsStatResult {
            path: path.clone(),
            kind,
            byte_length: snapshot
                .as_ref()
                .map(|value| u64::try_from(value.bytes.len()).unwrap_or(u64::MAX))
                .or_else(|| catalog_entry.as_ref().and_then(|entry| entry.byte_length)),
            modified_at: catalog_entry.as_ref().and_then(|entry| entry.modified_at.clone()),
            content_hash: snapshot.map(|value| value.logical_hash),
            ignored: catalog_entry.as_ref().is_some_and(|entry| entry.ignored),
            generated: catalog_entry.as_ref().is_some_and(|entry| entry.generated),
            reparse_tag: resolved.info.is_reparse_point.then_some(resolved.info.reparse_tag),
            active_editor_session: self.buffers().active_editor(&resolver, path)?,
        })
    }

    /// One page of a folder's entries, at most [`LISTED`], in path order; `next_after` says where
    /// the next one starts.
    pub fn list(&self, request: FsListRequest) -> Result<FsListResult, FsToolError> {
        let resolver = self.resolver(request.workspace_id)?;
        let resolved = resolver.resolve_existing(&request.target.path)?;
        if !resolved.info.is_directory {
            return Err(
                aura_workspace::WorkspaceError::WrongKind { path: request.target.path }.into()
            );
        }
        let after = request.after.as_ref();
        if after.is_some_and(|after| !is_child(&request.target.path, after)) {
            return Err(FsToolError::InvalidRequest(
                "after must be a child of the listed folder: a listing's next_after".into(),
            ));
        }
        let catalog = self.catalog(request.workspace_id)?;
        // A folder the index stops above (an ignored one, or one below it) is read from disk.
        let (mut entries, revision) = match aura_workspace::children_beyond_index(
            &catalog,
            &resolver,
            &request.target.path,
            after,
            LISTED + 1,
        )? {
            Some(page) => page,
            None => catalog.children_after(&request.target.path, after, LISTED + 1)?,
        };
        let next_after = (entries.len() > LISTED)
            .then(|| {
                entries.truncate(LISTED);
                entries.last().map(|entry| entry.path.clone())
            })
            .flatten();
        let entries = entries
            .into_iter()
            .map(|entry| FsListEntry {
                path: entry.path,
                name: entry.file_name,
                kind: node_kind(entry.kind),
                byte_length: entry.byte_length,
                ignored: entry.ignored,
                generated: entry.generated,
                reparse_tag: entry.reparse_tag,
            })
            .collect();
        self.record_path_use(request.workspace_id, &request.target.path)?;
        Ok(FsListResult { entries, catalog_revision: revision, next_after })
    }

    pub fn find_paths(&self, request: FsFindPathsRequest) -> Result<PathSearchResult, FsToolError> {
        let limit = usize::try_from(request.limit)
            .unwrap_or(self.policy.max_search_results)
            .clamp(1, self.policy.max_search_results);
        Ok(self.catalog(request.workspace_id)?.find_paths(&request.query, limit)?)
    }

    pub fn glob(&self, request: FsGlobRequest) -> Result<FsGlobResult, FsToolError> {
        let catalog = self.catalog(request.workspace_id)?;
        let revision = catalog.revision()?;
        let limit = usize::try_from(request.limit)
            .unwrap_or(self.policy.max_search_results)
            .clamp(1, self.policy.max_search_results);
        let paths = catalog
            .glob(&request.pattern)?
            .into_iter()
            .take(limit)
            .map(|entry| entry.path)
            .collect();
        Ok(FsGlobResult { paths, catalog_revision: revision })
    }

    pub fn read_text(&self, request: FsReadTextRequest) -> Result<FsReadTextResult, FsToolError> {
        let resolver = self.resolver(request.workspace_id)?;
        // The size decision uses the opened handle, never a catalog estimate.
        let stream = self.buffers().disk_text_snapshot_streaming(
            &resolver,
            &request.target.path,
            &request.range,
            request.explicit_encoding,
            self.policy.full_text_read_limit_bytes,
            aura_workspace::LOGICAL_TEXT_STREAM_CHUNK_BYTES,
        )?;
        match stream {
            DiskTextSnapshot::Stream(snapshot) => {
                // A "stream" result means the opened handle was larger than the
                // whole-read limit and no writable editor buffer owns the path, so
                // only the requested range was retained. The policy decision needs
                // nothing but "larger than the limit", which is exactly what this
                // stream observed; no whole-file byte count exists here.
                let above_limit = self
                    .policy
                    .full_text_read_limit_bytes
                    .saturating_add(1)
                    .try_into()
                    .unwrap_or(usize::MAX);
                enforce_text_policy(&self.policy, above_limit, &request.range)?;
                // The stream retained exactly the requested range, so it is already
                // selected: only the whole-file text of the in-memory path still goes
                // through `select_text_range`.
                let text = snapshot.retained_text;
                self.record_path_use(request.workspace_id, &request.target.path)?;
                Ok(FsReadTextResult {
                    path: request.target.path,
                    text,
                    encoding: snapshot.encoding,
                    line_ending: snapshot.line_ending,
                    source: snapshot.source,
                    logical_hash: snapshot.logical_hash,
                    disk_hash: snapshot.disk_hash,
                    buffer_revision: None,
                    is_dirty: false,
                    active_editor_session: snapshot.active_editor_session,
                    disk_may_be_stale: false,
                    total_characters: snapshot.total_characters,
                    total_lines: snapshot.total_lines,
                    returned_range: request.range,
                    // A workspace read is not an external target.
                    external: None,
                })
            }
            DiskTextSnapshot::UseSnapshot => {
                let snapshot = self.buffers().logical_file_snapshot(
                    &resolver,
                    request.session_id,
                    &request.target.path,
                )?;
                enforce_text_policy(&self.policy, snapshot.bytes.len(), &request.range)?;
                let decoded = decode_snapshot(&snapshot.bytes, request.explicit_encoding)?;
                let total_characters =
                    u64::try_from(decoded.text.chars().count()).unwrap_or(u64::MAX);
                let total_lines = u64::try_from(decoded.text.lines().count()).unwrap_or(u64::MAX);
                let text = select_text_range(&decoded.text, &request.range)?;
                let disk_may_be_stale = snapshot.source == FileStorageSource::Disk
                    && snapshot.active_editor_session.is_some();
                self.record_path_use(request.workspace_id, &request.target.path)?;
                Ok(FsReadTextResult {
                    path: request.target.path,
                    text,
                    encoding: decoded.encoding,
                    line_ending: decoded.line_ending,
                    source: snapshot.source,
                    logical_hash: snapshot.logical_hash,
                    disk_hash: snapshot.disk_hash,
                    buffer_revision: snapshot.buffer_revision,
                    is_dirty: snapshot.is_dirty,
                    active_editor_session: snapshot.active_editor_session,
                    disk_may_be_stale,
                    total_characters,
                    total_lines,
                    returned_range: request.range,
                    // A workspace read is not an external target.
                    external: None,
                })
            }
        }
    }

    pub fn hash(&self, request: FsHashRequest) -> Result<FsHashResult, FsToolError> {
        let resolver = self.resolver(request.workspace_id)?;
        let snapshot = self.buffers().logical_file_snapshot(
            &resolver,
            request.session_id,
            &request.target.path,
        )?;
        let disk_may_be_stale =
            snapshot.source == FileStorageSource::Disk && snapshot.active_editor_session.is_some();
        self.record_path_use(request.workspace_id, &request.target.path)?;
        Ok(FsHashResult {
            path: request.target.path,
            hash: snapshot.logical_hash,
            source: snapshot.source,
            disk_hash: snapshot.disk_hash,
            is_dirty: snapshot.is_dirty,
            active_editor_session: snapshot.active_editor_session,
            disk_may_be_stale,
        })
    }

    pub fn inspect(&self, request: FsInspectRequest) -> Result<FsInspectResult, FsToolError> {
        let resolver = self.resolver(request.workspace_id)?;
        let snapshot = self.buffers().logical_file_snapshot(
            &resolver,
            request.session_id,
            &request.target.path,
        )?;
        self.record_path_use(request.workspace_id, &request.target.path)?;
        Ok(inspect_bytes(request.target.path, &snapshot.bytes, snapshot.source, snapshot.is_dirty))
    }

    pub fn read_binary_range(
        &self,
        request: FsReadBinaryRangeRequest,
    ) -> Result<FsReadBinaryRangeResult, FsToolError> {
        let length = if request.length == 0 {
            self.policy.default_binary_range_bytes
        } else {
            request.length
        };
        if length > self.policy.max_binary_range_bytes || length > 64 * 1024 {
            return Err(FsToolError::BinaryRangeTooLarge);
        }
        let resolver = self.resolver(request.workspace_id)?;
        let snapshot = self.buffers().logical_file_snapshot(
            &resolver,
            request.session_id,
            &request.target.path,
        )?;
        let start = usize::try_from(request.offset)
            .map_err(|_| FsToolError::InvalidRequest("binary offset exceeds usize".to_owned()))?
            .min(snapshot.bytes.len());
        let end = start
            .saturating_add(usize::try_from(length).unwrap_or(usize::MAX))
            .min(snapshot.bytes.len());
        let bytes = &snapshot.bytes[start..end];
        self.record_path_use(request.workspace_id, &request.target.path)?;
        Ok(FsReadBinaryRangeResult {
            path: request.target.path,
            offset: request.offset,
            returned_bytes: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
            total_bytes: u64::try_from(snapshot.bytes.len()).unwrap_or(u64::MAX),
            format: request.format,
            data: render_binary(bytes, request.format),
            content_hash: snapshot.logical_hash,
            source: snapshot.source,
        })
    }

    pub fn search_text(
        &self,
        request: FsSearchTextRequest,
    ) -> Result<FsSearchTextResult, FsToolError> {
        self.search_text_with_cancel(request, &crate::SearchCancel::new()).map(|(result, _)| result)
    }

    pub fn search_text_with_cancel(
        &self,
        request: FsSearchTextRequest,
        cancel: &crate::SearchCancel,
    ) -> Result<(FsSearchTextResult, crate::search::EngineDiagnostics), FsToolError> {
        let resolver = self.resolver(request.workspace_id)?;
        let catalog = self.catalog(request.workspace_id)?;
        let buffers = self.buffers();
        let stages = Arc::new(crate::search::StageDiagnosticsImpl::default());
        let outcome =
            crate::search::execute(&catalog, &buffers, &resolver, &request, cancel, &stages);
        self.record_search_diagnostics(stages.snapshot());
        let (mut result, diagnostics) = outcome?;
        let skipped = crate::unindexed::skipped(&catalog, &resolver, &request.root, true)?;
        if !skipped.is_empty() {
            result.skipped_scopes.extend(skipped);
            crate::unindexed::partial(&mut result.completion);
        }
        Ok((result, diagnostics))
    }

    pub fn capture_workspace_file(
        &self,
        request: CaptureWorkspaceFileRequest,
    ) -> Result<CaptureWorkspaceFileResult, FsToolError> {
        let resolver = self.resolver(request.workspace_id)?;
        let snapshot = self.buffers().logical_file_snapshot(
            &resolver,
            request.session_id,
            &request.target.path,
        )?;
        let inspection = inspect_bytes(
            request.target.path.clone(),
            &snapshot.bytes,
            snapshot.source,
            snapshot.is_dirty,
        );
        let artifact = self.persistence.write_artifact(ArtifactWriteRequest {
            scope: ArtifactScope::Session,
            media_type: inspection.media_type,
            access: ArtifactAccess::USER_READ
                .union(ArtifactAccess::MODEL_READ)
                .union(ArtifactAccess::ATTACH),
            owner: aura_artifact_contract::ArtifactOwnerRef::Session {
                session_id: request.session_id,
            },
            bytes: snapshot.bytes,
        })?;
        self.record_path_use(request.workspace_id, &request.target.path)?;
        Ok(CaptureWorkspaceFileResult {
            artifact,
            path: request.target.path,
            source: snapshot.source,
            logical_hash: snapshot.logical_hash,
        })
    }
}

pub(crate) fn node_kind(kind: CatalogEntryKind) -> FsNodeKind {
    match kind {
        CatalogEntryKind::File => FsNodeKind::File,
        CatalogEntryKind::Directory => FsNodeKind::Directory,
        CatalogEntryKind::ReparsePoint => FsNodeKind::ReparsePoint,
    }
}

pub(crate) fn decode_snapshot(
    bytes: &[u8],
    explicit_encoding: Option<TextEncoding>,
) -> Result<DecodedText, FsToolError> {
    explicit_encoding
        .map_or_else(|| decode_text(bytes), |encoding| decode_text_with_encoding(bytes, encoding))
        .map_err(Into::into)
}

pub(crate) fn enforce_text_policy(
    policy: &crate::FsToolPolicy,
    byte_length: usize,
    range: &TextReadRange,
) -> Result<(), FsToolError> {
    let large = u64::try_from(byte_length).unwrap_or(u64::MAX) > policy.full_text_read_limit_bytes;
    if large
        && matches!(range, TextReadRange::Full)
        && policy.large_file_read_mode != LargeFileReadMode::AllowFull
    {
        return Err(FsToolError::LargeFileRangeRequired);
    }
    if policy.large_file_read_mode == LargeFileReadMode::SmallRangeOnly {
        match range {
            TextReadRange::Full => {}
            TextReadRange::Lines { count, .. } if *count > policy.small_range_line_limit => {
                return Err(FsToolError::TextRangeTooLarge);
            }
            TextReadRange::Characters { count, .. }
                if *count > policy.small_range_character_limit =>
            {
                return Err(FsToolError::TextRangeTooLarge);
            }
            TextReadRange::Lines { .. } | TextReadRange::Characters { .. } => {}
        }
    }
    Ok(())
}

fn select_text_range(text: &str, range: &TextReadRange) -> Result<String, FsToolError> {
    match range {
        TextReadRange::Full => Ok(text.to_owned()),
        TextReadRange::Lines { start_line_index, count } => {
            let start = usize::try_from(*start_line_index)
                .map_err(|_| FsToolError::InvalidRequest("line index exceeds usize".to_owned()))?;
            let count = usize::try_from(*count)
                .map_err(|_| FsToolError::InvalidRequest("line count exceeds usize".to_owned()))?;
            Ok(text.lines().skip(start).take(count).collect::<Vec<_>>().join("\n"))
        }
        TextReadRange::Characters { start_character_offset, count } => {
            let start = usize::try_from(*start_character_offset).map_err(|_| {
                FsToolError::InvalidRequest("character offset exceeds usize".to_owned())
            })?;
            let count = usize::try_from(*count).map_err(|_| {
                FsToolError::InvalidRequest("character count exceeds usize".to_owned())
            })?;
            Ok(text.chars().skip(start).take(count).collect())
        }
    }
}

#[cfg(test)]
mod tests {
    use aura_workspace_domain::TextReadRange;

    use super::select_text_range;

    #[test]
    fn text_ranges_use_explicit_zero_based_offsets() {
        assert_eq!(
            select_text_range(
                "zero\none\ntwo",
                &TextReadRange::Lines { start_line_index: 1, count: 1 },
            )
            .expect("lines"),
            "one",
        );
        assert_eq!(
            select_text_range(
                "Aura",
                &TextReadRange::Characters { start_character_offset: 1, count: 2 },
            )
            .expect("characters"),
            "ur",
        );
    }
}
