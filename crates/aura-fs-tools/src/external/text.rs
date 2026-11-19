//! Text and document reads for a verified external file handle.
use super::ensure_same_file;
use crate::{FsToolError, FsToolService};
use aura_domain_common::{ContentHash, ExternalGrantId, SessionId, WorkspacePath};
use aura_windows_platform::VerifiedFile;
use aura_workspace::stream_verified_text;
use aura_workspace_domain::{LineEnding, TextEncoding, TextReadRange};

/// One authorized external file plus the handle the permission owner verified.
pub struct ExternalTextReadTarget {
    /// The session grant that authorized this target, when a grant did. A target the
    /// existing policy allowed directly has none.
    pub grant_id: Option<ExternalGrantId>,
    pub session_id: SessionId,
    /// The absolute path as the user sees it, for the audit trail and the model's
    /// own reference. It is never used to open anything: the handle is.
    pub display_path: String,
    /// The path relative to the grant root, which is the form the grant's scope was
    /// checked against.
    pub relative_path: WorkspacePath,
    pub file: VerifiedFile,
}

impl std::fmt::Debug for ExternalTextReadTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExternalTextReadTarget")
            .field("grant_id", &self.grant_id)
            .field("session_id", &self.session_id)
            .field("display_path", &self.display_path)
            .field("relative_path", &self.relative_path)
            .finish_non_exhaustive()
    }
}

pub struct FsExternalTextReadRequest {
    pub target: ExternalTextReadTarget,
    pub range: TextReadRange,
    pub explicit_encoding: Option<TextEncoding>,
}

impl std::fmt::Debug for FsExternalTextReadRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FsExternalTextReadRequest")
            .field("target", &self.target)
            .field("range", &self.range)
            .field("explicit_encoding", &self.explicit_encoding)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FsExternalTextReadResult {
    pub grant_id: Option<ExternalGrantId>,
    /// The absolute path the caller named, echoed back so the model and the audit
    /// trail refer to the file the read really used.
    pub display_path: String,
    /// The path relative to the grant root.
    pub relative_path: WorkspacePath,
    pub text: String,
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
    pub logical_hash: ContentHash,
    pub disk_hash: Option<ContentHash>,
    pub total_characters: u64,
    pub total_lines: u64,
    pub returned_range: TextReadRange,
    pub document_format: Option<aura_workspace_domain::FsReadDocumentFormat>,
    pub document_headings: Vec<aura_workspace_domain::FsReadDocumentHeading>,
    /// Maximum bytes retained while reading, so the caller can see the read was
    /// bounded by the range and not by the file.
    pub peak_retained_bytes: usize,
}

impl FsToolService {
    /// Reads one already-authorized external file.
    ///
    /// The caller owns authorization: it resolved the grant, checked the capability
    /// and the scope, and opened this handle. This method only reads it, and it
    /// reports the same facts a workspace read of the same bytes would report.
    pub fn read_external_text(
        &self,
        request: FsExternalTextReadRequest,
    ) -> Result<FsExternalTextReadResult, FsToolError> {
        let FsExternalTextReadRequest { mut target, range, explicit_encoding } = request;
        let final_path = target.file.info().final_path.clone();
        if target.file.info().is_directory {
            return Err(FsToolError::InvalidRequest(
                "an external file target must name a file, not a directory".into(),
            ));
        }
        let identity = aura_workspace::domain_identity(&target.file.info().identity);
        if let Some(kind) = crate::documents::kind(&final_path) {
            let document = crate::documents::read(
                target.file.file_mut(),
                &final_path,
                kind,
                &range,
                &self.policy,
            )?;
            ensure_same_file(&final_path, &identity)?;
            return Ok(FsExternalTextReadResult {
                grant_id: target.grant_id,
                display_path: target.display_path,
                relative_path: target.relative_path,
                text: document.text,
                encoding: TextEncoding::Utf8,
                line_ending: LineEnding::Lf,
                logical_hash: document.logical_hash,
                disk_hash: Some(document.disk_hash),
                total_characters: document.total_characters,
                total_lines: document.total_lines,
                returned_range: range,
                document_format: Some(document.format),
                document_headings: document.headings,
                peak_retained_bytes: document.peak_retained_bytes,
            });
        }
        // The size decision uses the opened handle, never a catalog estimate, and
        // the whole-read policy applies to an external file exactly as it does to a
        // workspace file.
        let byte_length = target.file.file().metadata().map_err(|source| {
            crate::error::io_error(
                "read the authorized external file metadata",
                &final_path,
                source,
            )
        })?;
        let byte_length = usize::try_from(byte_length.len()).unwrap_or(usize::MAX);
        crate::read::enforce_text_policy(&self.policy, byte_length, &range)?;
        let snapshot = stream_verified_text(
            target.file.file_mut(),
            &final_path,
            &target.relative_path,
            &range,
            explicit_encoding,
            aura_workspace::LOGICAL_TEXT_STREAM_CHUNK_BYTES,
        )?;
        // A file replaced while it was read must not be reported as the file the
        // permission owner verified.
        ensure_same_file(&final_path, &identity)?;
        Ok(FsExternalTextReadResult {
            grant_id: target.grant_id,
            display_path: target.display_path,
            relative_path: target.relative_path,
            text: snapshot.retained_text,
            encoding: snapshot.encoding,
            line_ending: snapshot.line_ending,
            logical_hash: snapshot.logical_hash,
            disk_hash: snapshot.disk_hash,
            total_characters: snapshot.total_characters,
            total_lines: snapshot.total_lines,
            returned_range: range,
            document_format: None,
            document_headings: vec![],
            peak_retained_bytes: snapshot.peak_retained_bytes,
        })
    }
}
