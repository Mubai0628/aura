//! Reading one file through an already-authorized external target.
//!
//! Authorization is not decided here. The permission owner resolves the session's
//! ExternalGrant, checks the capability, the scope and the file identity, and then
//! hands over the verified handle it opened. This module performs the read on that
//! handle with exactly the decoding, range, hashing and line-ending logic the
//! workspace read uses, and re-checks the file identity afterwards so a file
//! replaced while it was read cannot be reported as the file that was verified.
//!
//! Two properties are deliberate:
//!
//! - the target is never widened: everything happens on the handle the caller
//!   verified, so no respelling, link or directory swap can move the read;
//! - the read stays bounded: the opened handle's size decides the whole-read
//!   policy, and a range read retains only the requested range.
mod listing;
mod publication;
mod traversal;
use publication::replace_through_parent;
use std::path::{Path, PathBuf};
use traversal::push_children;

use aura_domain_common::{
    ContentHash, ExternalGrantId, FileIdentity, Revision, SessionId, WorkspacePath,
};
use aura_windows_platform::{VerifiedFile, final_path_within};
use aura_workspace::stream_verified_text;
use aura_workspace_domain::{
    FsExpectedState, FsNodeKind, FsSearchPattern, LineEnding, TextEncoding, TextReadRange,
};

use crate::FsToolError;
use crate::FsToolService;
use crate::error::{ExternalPublishStage, ExternalWriteError};
use crate::mutation::{ExistingState, validate_expected};

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
    /// Maximum bytes retained while reading, so the caller can see the read was
    /// bounded by the range and not by the file.
    pub peak_retained_bytes: usize,
}

/// The identity check that has to hold after a read: the path must still name the
/// file identity the permission owner verified.
pub(crate) fn ensure_same_file(path: &Path, expected: &FileIdentity) -> Result<(), FsToolError> {
    let current = aura_windows_platform::inspect_existing_path(path, true).map_err(|source| {
        crate::error::io_error("re-verify the authorized external file", path, source)
    })?;
    if &aura_workspace::domain_identity(&current.identity) != expected {
        return Err(FsToolError::ExternalFileReplaced { path: path.to_path_buf() });
    }
    Ok(())
}

/// One authorized external directory plus the handle the permission owner verified.
pub struct ExternalListTarget {
    /// The session grant that authorized this target, when a grant did.
    pub grant_id: Option<ExternalGrantId>,
    pub session_id: SessionId,
    /// The absolute directory path as the user sees it.
    pub display_path: String,
    /// The directory relative to the grant root.
    pub relative_path: WorkspacePath,
    pub directory: VerifiedFile,
}

impl std::fmt::Debug for ExternalListTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExternalListTarget")
            .field("grant_id", &self.grant_id)
            .field("session_id", &self.session_id)
            .field("display_path", &self.display_path)
            .field("relative_path", &self.relative_path)
            .finish_non_exhaustive()
    }
}

pub struct FsExternalListRequest {
    pub target: ExternalListTarget,
    pub limit: u32,
    /// The last name the previous page returned, inside this same directory.
    pub after_name: Option<String>,
}

impl std::fmt::Debug for FsExternalListRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FsExternalListRequest")
            .field("target", &self.target)
            .field("limit", &self.limit)
            .field("after_name", &self.after_name)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FsExternalListEntry {
    /// The entry relative to the grant root.
    pub relative_path: WorkspacePath,
    pub name: String,
    pub kind: FsNodeKind,
    pub byte_length: Option<u64>,
    pub reparse_tag: Option<u32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FsExternalListResult {
    pub grant_id: Option<ExternalGrantId>,
    pub display_path: String,
    pub relative_path: WorkspacePath,
    pub entries: Vec<FsExternalListEntry>,
    /// A digest of the directory's live contents at this read, so a caller can tell
    /// whether the next page still describes the same state.
    pub scope_revision: Revision,
    pub truncated: bool,
    /// The name the next page continues after, when this page was truncated.
    pub next_after_name: Option<String>,
}

/// The number of directory entries one external listing reads.
///
/// An external tree has no index, so a listing reads the disk; the bound keeps one
/// call from reading an arbitrarily large directory.
const MAX_EXTERNAL_LIMIT: u32 = 100;

/// The position a continuation token names, in either spelling the caller holds.
///
/// A listing returns both the entry name and its grant-relative path, and a caller
/// may resume with either. They are the same position inside this directory, so the
/// grant-relative form is reduced to the name before it is compared; anything else is
/// already a name and is used as it stands.
fn resume_name(directory: &WorkspacePath, after: &str) -> String {
    let trimmed = after.trim_matches('/');
    if directory.is_root() {
        return trimmed.to_owned();
    }
    let prefix = format!("{}/", directory.as_str());
    trimmed.strip_prefix(&prefix).unwrap_or(trimmed).to_owned()
}

/// The grant-relative path of one child of an authorized directory.
fn child_relative(directory: &WorkspacePath, name: &str) -> Result<WorkspacePath, FsToolError> {
    let text = if directory.is_root() { name.to_owned() } else { format!("{directory}/{name}") };
    WorkspacePath::parse(text).map_err(|error| FsToolError::InvalidRequest(error.to_string()))
}

/// The version token of one external listing: a stable projection of the entry
/// facts that were returned and whether more exist. It is only ever compared for
/// equality, which is exactly how a caller decides whether a continuation still
/// describes the same directory state.
pub(crate) fn external_scope_version(
    facts: &[(String, bool, u64, bool)],
) -> Result<Revision, FsToolError> {
    let digest = aura_domain_common::jcs::canonical_sha256(facts)
        .map_err(|_| FsToolError::Internal("external listing digest".into()))?;
    let mut token = 0_u64;
    for character in digest.hex().chars().take(16) {
        token = (token << 4) | u64::from(character.to_digit(16).unwrap_or(0));
    }
    Ok(Revision::new(token))
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
            peak_retained_bytes: snapshot.peak_retained_bytes,
        })
    }

    /// Writes one already-authorized external file.
    ///
    /// Authorization is not decided here either: the permission owner resolved the
    /// grant, checked the lifecycle, the capability and the scope, and then either
    /// opened the existing file for writing or handed over a verified parent
    /// directory for a file that does not exist yet.
    ///
    /// Three properties are deliberate:
    ///
    /// - the expected-state and encoding decisions are the workspace ones
    ///   ([`crate::mutation::validate_expected`] / [`crate::mutation::prepare_write_bytes`]),
    ///   so `expected_state` cannot mean something weaker outside the workspace and
    ///   an overwritten file keeps its own encoding and line endings;
    /// - the bytes are published **handle-relative** to the verified parent
    ///   directory, with the target identity compared first when replacing. Nothing
    ///   is respelled by path and the workspace staging area is not used at all: an
    ///   external file is not part of the workspace transaction, its journal or its
    ///   index;
    /// - a publication that cannot be committed is rolled back by the platform, and
    ///   a rollback that itself fails is reported as requiring reconciliation
    ///   instead of as an ordinary rejection, because in that case what the
    ///   destination holds is genuinely not known.
    ///
    /// A replacement releases the verified handle before publishing: the publication
    /// opens the destination exclusively and compares it against the same identity,
    /// so holding the handle would only block the write it authorized.
    pub fn write_external(
        &self,
        request: FsExternalWriteRequest,
    ) -> Result<FsExternalWriteResult, ExternalWriteError> {
        let FsExternalWriteRequest { target, content, expected, encoding, line_ending } = request;
        let ExternalWriteTarget { grant_id, session_id, display_path, relative_path, kind } =
            target;
        match kind {
            ExternalWriteKind::Existing(file) => {
                let final_path = file.info().final_path.clone();
                if file.info().is_directory {
                    return Err(ExternalWriteError::refused(FsToolError::InvalidRequest(
                        "an external write target must name a file, not a directory".into(),
                    )));
                }
                let target_info = file.info().clone();
                // The bytes are read through the verified handle, and then the handle
                // is released. The publication opens the destination exclusively
                // (denying sharing) and compares it against this exact identity
                // before it moves anything, so keeping the handle would only block
                // the write it is supposed to authorize.
                let state = {
                    let mut file = file;
                    ExistingState::from_bytes(
                        read_verified_bytes(&mut file, &final_path)
                            .map_err(ExternalWriteError::refused)?,
                    )
                };
                let observed_previous_hash = state.hash();
                validate_expected(&relative_path, &state, &expected)
                    .map_err(|error| refused(external_path_error(error, &display_path)))?;
                let encoded = crate::mutation::prepare_write_bytes(
                    &relative_path,
                    &state,
                    &content,
                    encoding,
                    line_ending,
                )
                .map_err(|error| refused(external_path_error(error, &display_path)))?;
                let written =
                    crate::mutation::write_encoding(&relative_path, &state, encoding, line_ending)
                        .map_err(ExternalWriteError::refused)?;
                let previous_hash = state.hash();
                let disk_hash = observed_previous_hash.clone().ok_or_else(|| {
                    ExternalWriteError::refused(FsToolError::Internal(
                        "an existing external target must have an observed hash".into(),
                    ))
                })?;
                replace_through_parent(
                    &final_path,
                    &target_info,
                    &disk_hash,
                    &encoded,
                    &display_path,
                )?;
                Ok(FsExternalWriteResult {
                    grant_id,
                    session_id,
                    display_path,
                    relative_path,
                    created: false,
                    encoding: written.0,
                    line_ending: written.1,
                    logical_hash: ContentHash::blake3(&encoded),
                    previous_hash,
                    observed_previous_hash: Some(disk_hash),
                    bytes_written: encoded.len() as u64,
                })
            }
            ExternalWriteKind::New { parent, leaf_name, final_path } => {
                // A create is only ever a create: the caller asked for a file that
                // does not exist, so something appearing between the parent check and
                // this call is a conflict, never an overwrite.
                validate_expected(&relative_path, &ExistingState::Absent, &expected)
                    .map_err(|error| refused(external_path_error(error, &display_path)))?;
                match aura_windows_platform::inspect_existing_path(&final_path, true) {
                    Ok(info) => {
                        let reason = if info.is_directory {
                            "a directory with that name already exists"
                        } else {
                            "the file already exists"
                        };
                        return Err(ExternalWriteError::refused(
                            FsToolError::ExternalExpectedState {
                                path: final_path,
                                reason: reason.to_owned(),
                            },
                        ));
                    }
                    Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
                    Err(source) => {
                        return Err(ExternalWriteError::refused(crate::error::io_error(
                            "check the authorized external create target",
                            &final_path,
                            source,
                        )));
                    }
                }
                let encoded = crate::mutation::prepare_write_bytes(
                    &relative_path,
                    &ExistingState::Absent,
                    &content,
                    encoding,
                    line_ending,
                )
                .map_err(|error| refused(external_path_error(error, &display_path)))?;
                let written = crate::mutation::write_encoding(
                    &relative_path,
                    &ExistingState::Absent,
                    encoding,
                    line_ending,
                )
                .map_err(ExternalWriteError::refused)?;
                let published =
                    aura_windows_platform::handle_bound_create_new(&parent, &leaf_name, &encoded)
                        .map_err(|error| publish_failure(error, &display_path))?;
                let info =
                    published.commit().map_err(|error| publish_failure(error, &display_path))?;
                ensure_same_file(&final_path, &aura_workspace::domain_identity(&info.identity))
                    .map_err(|error| ExternalWriteError {
                        stage: ExternalPublishStage::PublishedUnconfirmed,
                        error: Box::new(error),
                        observed_previous_hash: None,
                    })?;
                crate::publication_receipt::published(&parent, &leaf_name, None, &info);
                Ok(FsExternalWriteResult {
                    grant_id,
                    session_id,
                    display_path,
                    relative_path,
                    created: true,
                    encoding: written.0,
                    line_ending: written.1,
                    logical_hash: ContentHash::blake3(&encoded),
                    previous_hash: None,
                    observed_previous_hash: None,
                    bytes_written: encoded.len() as u64,
                })
            }
        }
    }

    /// Applies a unified diff to one already-authorized external file.
    ///
    /// The diff is applied to the file the permission owner verified, decoded and
    /// re-encoded in that file's own convention. A header naming a different path
    /// cannot redirect it — the target is the verified handle, not the header — so
    /// only `/dev/null` (the create/delete form this path does not implement) is
    /// refused. Hunks that do not match the real bytes fail without writing.
    pub fn patch_external(
        &self,
        request: FsExternalPatchRequest,
    ) -> Result<FsExternalWriteResult, ExternalWriteError> {
        let FsExternalPatchRequest { target, unified_diff, expected_hash } = request;
        let ExternalWriteTarget { grant_id, session_id, display_path, relative_path, kind } =
            target;
        let ExternalWriteKind::Existing(file) = kind else {
            return Err(ExternalWriteError::refused(FsToolError::InvalidRequest(
                "a patch applies to an existing external file, not to a create".into(),
            )));
        };
        let final_path = file.info().final_path.clone();
        if file.info().is_directory {
            return Err(ExternalWriteError::refused(FsToolError::InvalidRequest(
                "an external patch target must name a file, not a directory".into(),
            )));
        }
        let target_info = file.info().clone();
        let state = {
            let mut file = file;
            ExistingState::from_bytes(
                read_verified_bytes(&mut file, &final_path).map_err(ExternalWriteError::refused)?,
            )
        };
        let observed_previous_hash = state.hash();
        if state.hash().as_ref() != Some(&expected_hash) {
            return Err(ExternalWriteError::refused(FsToolError::ExternalExpectedState {
                path: PathBuf::from(&display_path),
                reason: format!(
                    "the file no longer has the hash this patch was built from ({expected_hash})"
                ),
            }));
        }
        let prepared =
            crate::mutation::prepare_diff(&unified_diff).map_err(ExternalWriteError::refused)?;
        let patch =
            crate::mutation::parse_patch(&prepared.text).map_err(ExternalWriteError::refused)?;
        crate::mutation::refuse_create_or_delete(&patch).map_err(ExternalWriteError::refused)?;
        let encoded =
            crate::mutation::patched_bytes(&relative_path, &state, &patch, prepared.unplaced)
                .map_err(ExternalWriteError::refused)?;
        let (encoding, line_ending) =
            crate::mutation::write_encoding(&relative_path, &state, None, None)
                .map_err(ExternalWriteError::refused)?;
        let previous_hash = state.hash();
        let disk_hash = observed_previous_hash.clone().ok_or_else(|| {
            ExternalWriteError::refused(FsToolError::Internal(
                "an existing external target must have an observed hash".into(),
            ))
        })?;
        replace_through_parent(&final_path, &target_info, &disk_hash, &encoded, &display_path)?;
        Ok(FsExternalWriteResult {
            grant_id,
            session_id,
            display_path,
            relative_path,
            created: false,
            encoding,
            line_ending,
            logical_hash: ContentHash::blake3(&encoded),
            previous_hash,
            observed_previous_hash: Some(disk_hash),
            bytes_written: encoded.len() as u64,
        })
    }
}

/// The facts every authorized external write needs in order to report itself.
pub struct ExternalWriteTarget {
    /// The session grant that authorized this target, when a grant did.
    pub grant_id: Option<ExternalGrantId>,
    pub session_id: SessionId,
    /// The absolute path as the user sees it.
    pub display_path: String,
    /// The path relative to the grant root.
    pub relative_path: WorkspacePath,
    /// `Existing` replaces this file; `New` creates the named file in a directory
    /// the permission owner validated. A create is never silently turned into an
    /// overwrite.
    pub kind: ExternalWriteKind,
}

pub enum ExternalWriteKind {
    Existing(VerifiedFile),
    New {
        parent: VerifiedFile,
        leaf_name: std::ffi::OsString,
        /// The absolute path the new file will have.
        final_path: PathBuf,
    },
}

/// Where an external write is really going, for a caller that has to coordinate two calls
/// against the same destination before it resolves the handles they publish through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExternalWriteDestination {
    /// The call replaces this file. The path is the object's own final path, so two
    /// spellings of the same file reduce to one destination.
    Existing(PathBuf),
    /// The call creates `leaf_name` inside this verified parent directory.
    New { parent: PathBuf, leaf_name: std::ffi::OsString },
}

impl ExternalWriteTarget {
    /// The destination this target publishes to, without opening anything.
    #[must_use]
    pub fn destination(&self) -> ExternalWriteDestination {
        match &self.kind {
            ExternalWriteKind::Existing(file) => {
                ExternalWriteDestination::Existing(file.info().final_path.clone())
            }
            ExternalWriteKind::New { parent, leaf_name, .. } => ExternalWriteDestination::New {
                parent: parent.info().final_path.clone(),
                leaf_name: leaf_name.clone(),
            },
        }
    }
}

/// One unauthorized-agnostic external write: the target, the text and the same
/// expected-state/encoding inputs `fs.write` uses inside the workspace.
pub struct FsExternalWriteRequest {
    pub target: ExternalWriteTarget,
    pub content: String,
    pub expected: FsExpectedState,
    pub encoding: Option<TextEncoding>,
    pub line_ending: Option<LineEnding>,
}

pub struct FsExternalPatchRequest {
    pub target: ExternalWriteTarget,
    pub unified_diff: String,
    /// The hash the caller built the diff from, checked before anything is written.
    pub expected_hash: ContentHash,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FsExternalWriteResult {
    pub grant_id: Option<ExternalGrantId>,
    pub session_id: SessionId,
    pub display_path: String,
    pub relative_path: WorkspacePath,
    /// True when this call created the file, false when it replaced one.
    pub created: bool,
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
    /// Hash of the bytes this call wrote.
    pub logical_hash: ContentHash,
    /// Hash the file had before the write; absent for a create.
    pub previous_hash: Option<ContentHash>,
    /// Hash the file layer last saw on disk before it published, when it got far enough to
    /// look. This is the observation a later reconciliation compares against: it is a fact
    /// about the object, not the caller's expected value, and it is what makes "the file
    /// changed under this call" a conflict instead of a silent overwrite.
    pub observed_previous_hash: Option<ContentHash>,
    pub bytes_written: u64,
}

/// The one file-name component of an absolute path.
fn leaf_name(final_path: &Path) -> Result<std::ffi::OsString, FsToolError> {
    final_path.file_name().map(ToOwned::to_owned).ok_or_else(|| {
        FsToolError::InvalidRequest(format!("{} does not name a file", final_path.display()))
    })
}

fn read_verified_bytes(file: &mut VerifiedFile, final_path: &Path) -> Result<Vec<u8>, FsToolError> {
    let mut bytes = Vec::new();
    use std::io::Read as _;
    file.file_mut().read_to_end(&mut bytes).map_err(|source| {
        crate::error::io_error("read the authorized external file for writing", final_path, source)
    })?;
    Ok(bytes)
}

/// The bytes of one searched file, read through the handle the platform verified.
struct CandidateBytes {
    bytes: Vec<u8>,
    bytes_read: u64,
}

/// Reads at most the size the listing reported, through the verified handle.
///
/// The listing's size is the read bound, so a file that grows while it is being searched
/// cannot turn one entry into an unbounded read: reading one byte more than the listing
/// reported is enough to notice, and the caller treats a short or over-long read as a
/// file that changed rather than as content.
fn read_via_handle(file: &mut VerifiedFile, size: u64) -> Option<CandidateBytes> {
    use std::io::Read as _;
    let want = usize::try_from(size).ok()?.saturating_add(1);
    let mut bytes = Vec::with_capacity(want.min(1 << 20));
    let mut handle = file.file_mut().take(u64::try_from(want).ok()?);
    let read = handle.read_to_end(&mut bytes).ok()?;
    Some(CandidateBytes { bytes, bytes_read: u64::try_from(read).unwrap_or(u64::MAX) })
}

/// The verified parent directory a replacement is published through.
///
/// The platform's publication re-checks that this parent really contains the target
/// before it publishes, so opening the parent by path cannot move the write: a
/// directory swapped in the meantime fails the identity comparison instead.
fn open_publish_parent(
    final_path: &Path,
    target: &aura_windows_platform::NativePathInfo,
) -> Result<VerifiedFile, FsToolError> {
    let parent = final_path.parent().ok_or_else(|| {
        FsToolError::InvalidRequest(format!("{} has no parent directory", final_path.display()))
    })?;
    let directory =
        aura_windows_platform::open_verified_publish_parent(parent).map_err(|source| {
            crate::error::io_error("open the authorized external parent directory", parent, source)
        })?;
    if !directory.info().is_directory || !final_path_within(directory.info(), target) {
        return Err(FsToolError::ExternalFileReplaced { path: final_path.to_path_buf() });
    }
    Ok(directory)
}

/// A refusal raised before the destination could be touched.
fn refused(error: FsToolError) -> ExternalWriteError {
    ExternalWriteError::refused(error)
}

/// Reports a failed publication with the stage the platform really reached.
///
/// The stage is taken from the platform's own report. Nothing here looks at which error
/// variant arrived: the same I/O error means "never touched" or "already renamed and rolled
/// back" depending on where it was raised, and the platform is the only place that knows.
///
/// The three outcomes stay three outcomes. A publication that never landed and one that landed
/// and was then rolled back are different facts about the object — the second one really did
/// write the new content before removing it again — so they are reported as different stages
/// all the way to the persisted fact rather than collapsed into "nothing happened".
fn publish_failure(
    error: aura_windows_platform::HandlePublishError,
    display_path: &str,
) -> ExternalWriteError {
    use aura_windows_platform::{HandlePublishError as Publish, PublishLocation};
    // A check that refused the call is not "the platform never got there": nothing was
    // attempted. The distinction matters to the persisted fact, because `NeverPublished`
    // means the publication sequence started and did not complete.
    let refused = matches!(
        error,
        Publish::TargetIdentityChanged
            | Publish::ParentIdentityChanged
            | Publish::TargetContentChanged { .. }
            | Publish::InvalidFileName
            | Publish::RemoteParentUnsupported
    ) || matches!(&error, Publish::Windows(source) if is_sharing_violation(source));
    let stage = if refused {
        ExternalPublishStage::Refused
    } else {
        match error.location() {
            PublishLocation::NeverPublished => ExternalPublishStage::NeverPublished,
            PublishLocation::RolledBack => ExternalPublishStage::PublishedThenRolledBack,
            PublishLocation::Unconfirmable => ExternalPublishStage::PublishedUnreconciled,
        }
    };
    let kind = match error {
        Publish::TargetIdentityChanged | Publish::ParentIdentityChanged => {
            FsToolError::ExternalFileReplaced { path: PathBuf::from(display_path) }
        }
        // The destination is the verified object but not the content the call decided about.
        // Reported as the expected-state conflict it is, because that is what the caller can
        // act on: re-read the file and decide again.
        Publish::TargetContentChanged { expected, actual } => FsToolError::ExternalExpectedState {
            path: PathBuf::from(display_path),
            reason: format!(
                "the file changed after it was read for this call: expected {expected}, \
                     found {actual}"
            ),
        },
        Publish::RollbackFailed { operation, rollback }
        | Publish::CompensationFailed { operation, rollback } => {
            FsToolError::ReconciliationRequired(format!(
                "publishing {display_path} failed ({operation}) and the previous content could \
                 not be restored ({rollback}); what the destination holds is not known"
            ))
        }
        // Another writer holding the destination is contention, not a fault: nothing was
        // published and the same call can be made again once the writer is done, so it is
        // reported as the state conflict it is rather than as a raw platform error.
        Publish::Windows(source) if is_sharing_violation(&source) => {
            FsToolError::ExternalExpectedState {
                path: PathBuf::from(display_path),
                reason: "another writer holds the destination right now; nothing was published \
                         and the call can be retried once it is free"
                    .to_owned(),
            }
        }
        other => FsToolError::ExternalPublishRolledBack {
            path: PathBuf::from(display_path),
            reason: other.to_string(),
        },
    };
    ExternalWriteError { stage, error: Box::new(kind), observed_previous_hash: None }
}

/// Whether a platform failure is another writer holding the destination.
///
/// Windows reports this as a sharing or lock violation depending on which exclusive open
/// lost, and both mean the same thing to the caller.
fn is_sharing_violation(source: &std::io::Error) -> bool {
    const ERROR_SHARING_VIOLATION: i32 = 32;
    const ERROR_LOCK_VIOLATION: i32 = 33;
    matches!(source.raw_os_error(), Some(ERROR_SHARING_VIOLATION | ERROR_LOCK_VIOLATION))
}

/// Reports an expected-state or encoding rejection against the path the caller
/// named rather than the grant-relative one, since that is the path a person and a
/// model can act on.
fn external_path_error(error: FsToolError, display_path: &str) -> FsToolError {
    match error {
        FsToolError::ExpectedAbsent { .. } => FsToolError::ExternalExpectedState {
            path: PathBuf::from(display_path),
            reason: "the file already exists".to_owned(),
        },
        FsToolError::ExpectedHash { expected, actual, .. } => FsToolError::ExternalExpectedState {
            path: PathBuf::from(display_path),
            reason: format!("content hash mismatch: expected {expected}, actual {actual}"),
        },
        FsToolError::ExpectedDirectory { .. } => FsToolError::ExternalExpectedState {
            path: PathBuf::from(display_path),
            reason: "the path is not the expected directory".to_owned(),
        },
        other => other,
    }
}

/// How one requested file name is compared against an external entry's name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalNameMatch {
    /// The whole name must equal the requested text.
    Exact,
    /// The whole name must match a `*`/`?` pattern.
    Glob,
}

/// One authorized external directory to search for names.
pub struct ExternalFindTarget {
    /// The session grant that authorized this target, when a grant did.
    pub grant_id: Option<ExternalGrantId>,
    pub session_id: SessionId,
    pub display_path: String,
    /// The directory relative to the grant root.
    pub relative_path: WorkspacePath,
    pub directory: VerifiedFile,
}

impl std::fmt::Debug for ExternalFindTarget {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExternalFindTarget")
            .field("grant_id", &self.grant_id)
            .field("session_id", &self.session_id)
            .field("display_path", &self.display_path)
            .field("relative_path", &self.relative_path)
            .finish_non_exhaustive()
    }
}

pub struct FsExternalFindRequest {
    pub target: ExternalFindTarget,
    pub file_names: Vec<String>,
    /// Descend into subdirectories. A reparse point is never followed, so a link
    /// cannot loop the walk or leave the authorized root.
    pub recursive: bool,
    pub match_mode: ExternalNameMatch,
    pub case_sensitive: bool,
    /// Continue after this path, relative to the grant root.
    ///
    /// Used when [`Self::consumed_names`] is empty, which is an older cursor. A page that
    /// stopped inside a directory continues by name instead, so an unordered listing cannot
    /// drop a smaller path that had not been read yet.
    pub after: Option<WorkspacePath>,
    /// Names already taken from [`Self::listing_directory`]. A later page on the same
    /// service resumes the directory iterator after these names. A page that has to
    /// open the directory again counts every entry it reads, including these names.
    pub consumed_names: Vec<String>,
    /// The directory `consumed_names` belongs to. `None` means the walk root.
    pub listing_directory: Option<WorkspacePath>,
    /// Entries already listed and not yet examined. They are restored before any new
    /// directory read, so a page that stopped at its result limit does not drop them.
    pub pending: Vec<FsExternalPending>,
    /// Directories whose children have all been listed into the walk.
    pub finished_directories: Vec<WorkspacePath>,
    /// Directories whose listing stopped before every child name was read.
    pub partial_listings: Vec<FsExternalPartialListing>,
    pub limit: u32,
    /// How many entries one call may examine.
    pub scan_budget: u64,
}

impl std::fmt::Debug for FsExternalFindRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FsExternalFindRequest")
            .field("target", &self.target)
            .field("file_names", &self.file_names)
            .field("recursive", &self.recursive)
            .field("match_mode", &self.match_mode)
            .field("case_sensitive", &self.case_sensitive)
            .field("after", &self.after)
            .field("limit", &self.limit)
            .field("scan_budget", &self.scan_budget)
            .finish()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FsExternalFindMatch {
    pub relative_path: WorkspacePath,
    pub name: String,
    pub kind: FsNodeKind,
    pub byte_length: Option<u64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FsExternalFindResult {
    pub grant_id: Option<ExternalGrantId>,
    pub display_path: String,
    pub relative_path: WorkspacePath,
    pub matches: Vec<FsExternalFindMatch>,
    /// The version of what this walk observed: the examined entry facts and whether
    /// the walk finished. Two calls that saw the same state produce the same token.
    pub scope_revision: Revision,
    /// True only when the walk reached the end of the authorized scope, so a name
    /// absent from this page is genuinely absent.
    pub complete: bool,
    pub truncated: bool,
    pub scanned_entries: u64,
    /// The last path the walk examined, which is where an older cursor continues.
    pub resume_after: Option<WorkspacePath>,
    /// Names this page has now taken from `listing_directory`, including the ones it was given.
    pub consumed_names: Vec<String>,
    /// The directory the consumed names belong to when the walk stopped inside it.
    pub listing_directory: Option<WorkspacePath>,
    /// Listed entries this page did not examine. The next page examines these before
    /// reading any further names.
    pub pending: Vec<FsExternalPending>,
    /// Directories fully listed by the end of this page.
    pub finished_directories: Vec<WorkspacePath>,
    /// Directories still only partly listed. Each keeps the names already read.
    pub partial_listings: Vec<FsExternalPartialListing>,
}

/// Names already read from one directory whose listing has not finished.
#[derive(Clone, Debug, PartialEq)]
pub struct FsExternalPartialListing {
    pub directory: WorkspacePath,
    pub consumed_names: Vec<String>,
}

/// One external entry the walk has listed and has not examined yet.
#[derive(Clone, Debug, PartialEq)]
pub struct FsExternalPending {
    pub relative_path: WorkspacePath,
    pub name: String,
    pub kind: FsNodeKind,
    pub byte_length: Option<u64>,
}

/// One compiled external name matcher, using the same pattern language the
/// workspace filename lookup accepts.
enum ExternalPattern {
    Exact { needle: String, case_sensitive: bool },
    Glob { pattern: globset::GlobMatcher },
}

impl ExternalPattern {
    fn compile(
        text: &str,
        mode: ExternalNameMatch,
        case_sensitive: bool,
    ) -> Result<Self, FsToolError> {
        match mode {
            ExternalNameMatch::Exact => Ok(Self::Exact { needle: text.to_owned(), case_sensitive }),
            ExternalNameMatch::Glob => {
                let pattern = globset::GlobBuilder::new(text)
                    .literal_separator(true)
                    .backslash_escape(false)
                    .case_insensitive(!case_sensitive)
                    .build()
                    .map(|glob| glob.compile_matcher())
                    .map_err(|error| {
                        FsToolError::InvalidRequest(format!(
                            "file_names contains {text:?}, which is not a valid glob: {error}"
                        ))
                    })?;
                Ok(Self::Glob { pattern })
            }
        }
    }

    fn matches(&self, name: &str) -> bool {
        match self {
            Self::Exact { needle, case_sensitive } => {
                if *case_sensitive {
                    name == needle
                } else {
                    name.eq_ignore_ascii_case(needle)
                }
            }
            Self::Glob { pattern, .. } => pattern.is_match(name),
        }
    }
}

impl FsToolService {
    /// Finds names under one already-authorized external directory.
    ///
    /// An external tree has no index, so this walks the directory itself. The walk is
    /// bounded twice: it stops when the page is full or when it has examined
    /// `scan_budget` entries, and `complete` is only true when it reached the end of
    /// the authorized scope. Entries are examined in path order, so a continuation
    /// resumes exactly where the previous call stopped examining and never skips a
    /// candidate. A reparse point is reported but never followed: a link can neither
    /// loop the walk nor reach outside the root the user authorized.
    pub fn find_external(
        &self,
        request: FsExternalFindRequest,
        cancel: Option<&crate::search::SearchCancel>,
    ) -> Result<FsExternalFindResult, FsToolError> {
        let FsExternalFindRequest {
            target,
            file_names,
            recursive,
            match_mode,
            case_sensitive,
            after,
            consumed_names,
            listing_directory,
            pending: restored_pending,
            finished_directories,
            partial_listings,
            limit,
            scan_budget,
        } = request;
        if !(1..=MAX_EXTERNAL_LIMIT).contains(&limit) {
            return Err(FsToolError::InvalidRequest(format!(
                "limit must be in 1..={MAX_EXTERNAL_LIMIT}"
            )));
        }
        let patterns = file_names
            .iter()
            .map(|name| ExternalPattern::compile(name, match_mode, case_sensitive))
            .collect::<Result<Vec<_>, _>>()?;
        let final_path = target.directory.info().final_path.clone();
        if !target.directory.info().is_directory {
            return Err(FsToolError::InvalidRequest(
                "an external search target must name a directory".into(),
            ));
        }
        let identity = aura_workspace::domain_identity(&target.directory.info().identity);
        let root_info = target.directory.info().clone();
        if root_info.is_reparse_point {
            return Err(FsToolError::ExternalFileReplaced { path: final_path });
        }
        let root_key = identity_key(&root_info.identity);
        let mut finished_directories = finished_directories;
        let token = take_find_query_token(&mut finished_directories)?;
        let continuation = token.is_some()
            || !restored_pending.is_empty()
            || !finished_directories.is_empty()
            || !partial_listings.is_empty()
            || !consumed_names.is_empty()
            || listing_directory.is_some();
        let session_key = target.session_id.to_string();
        let incoming_key = find_cursor_key(&FindCursorParts {
            session: &session_key,
            relative: target.relative_path.as_str(),
            file_names: &file_names,
            recursive,
            glob: matches!(match_mode, ExternalNameMatch::Glob),
            case_sensitive,
            after: after.as_ref(),
            consumed: &consumed_names,
            listing: listing_directory.as_ref(),
            pending: &restored_pending,
            finished: &finished_directories,
            partials: &partial_listings,
        });
        let (query_id, generation, mut walk) = {
            let mut queries = self.find_queries.lock().unwrap_or_else(|error| error.into_inner());
            if let Some((id, generation)) = token {
                let memory =
                    queries.take_issued(id, generation, incoming_key).ok_or_else(|| {
                        FsToolError::InvalidRequest(
                            "external find continuation is not a page this service issued".into(),
                        )
                    })?;
                (id, generation, memory)
            } else if continuation {
                return Err(FsToolError::InvalidRequest(
                    "external find continuation is not a page this service issued".into(),
                ));
            } else {
                (queries.allocate(), 0, ExternalWalkMemory::default())
            }
        };
        if let Some(seen) = walk.recall(&root_key, &target.relative_path) {
            let modified = path_modified(&final_path)?;
            if seen.identity != root_info.identity || seen.modified != modified {
                return Err(FsToolError::ExternalFileReplaced { path: final_path.clone() });
            }
        }
        if let Ok(modified) = path_modified(&final_path) {
            walk.remember(
                &root_key,
                &target.relative_path,
                RememberedObject {
                    identity: root_info.identity.clone(),
                    modified,
                    is_reparse: false,
                    unread: None,
                },
            );
        }
        // A min-heap keyed by the grant-relative path makes the walk visit entries in
        // path order. Cursor names and kinds are not disk facts: each restored entry
        // is opened again and rejected when it is missing, a reparse point, or a
        // different object from the one this service listed.
        let mut pending = std::collections::BinaryHeap::new();
        let mut restored = std::collections::HashSet::new();
        for item in restored_pending {
            let relative = item.relative_path.clone();
            restored.insert(relative);
            pending.push(materialize_pending(
                &mut walk,
                &root_key,
                &root_info,
                &target.relative_path,
                &final_path,
                item,
            )?);
        }
        let mut finished = finished_directories;
        let mut read_budget = scan_budget;
        let mut charged = 0_u64;
        let root = target.relative_path.clone();
        let mut partials = partial_listings;
        if !consumed_names.is_empty() {
            let directory = listing_directory.unwrap_or_else(|| root.clone());
            if partials.iter().all(|item| item.directory != directory) {
                partials.push(FsExternalPartialListing { directory, consumed_names });
            }
        }
        // A directory that was only partly listed has to be entered again. Recording
        // its names without putting it back on the walk reports the page complete
        // and drops every name that was not read yet.
        let already_queued =
            pending.iter().map(|entry| entry.relative_path.clone()).collect::<Vec<_>>();
        let mut resume_directories = Vec::new();
        for partial in &partials {
            if partial.directory == root
                || finished.iter().any(|directory| directory == &partial.directory)
                || already_queued.iter().any(|path| path == &partial.directory)
            {
                continue;
            }
            let name = partial.directory.as_str().rsplit('/').next().unwrap_or_default().to_owned();
            resume_directories.push(FsExternalPending {
                relative_path: partial.directory.clone(),
                name,
                kind: FsNodeKind::Directory,
                byte_length: None,
            });
        }
        for item in resume_directories {
            pending.push(materialize_pending(
                &mut walk,
                &root_key,
                &root_info,
                &root,
                &final_path,
                item,
            )?);
        }
        let mut read_truncated = false;
        if !finished.iter().any(|directory| directory == &root) && pending.is_empty() {
            let listing_names = partial_names(&partials, &root);
            let listed = push_children(&mut ChildListing {
                memory: &mut walk,
                root_key: &root_key,
                root_info: &root_info,
                resume: continuation,
                pending: &mut pending,
                directory: &root,
                absolute: &final_path,
                budget: read_budget,
                consumed: &listing_names,
                cancel,
            })?;
            charged = charged.saturating_add(listed.progress);
            read_budget = read_budget.saturating_sub(listed.progress);
            read_truncated = listed.truncated;
            if listed.truncated {
                remember_partial(
                    &mut partials,
                    root.clone(),
                    merge_consumed(&listing_names, &listed.accepted),
                );
            } else {
                finished.push(root.clone());
                forget_partial(&mut partials, &root);
            }
        }
        let mut matches = Vec::new();
        let mut examined_to: Option<WorkspacePath> = None;
        #[allow(unused_assignments)]
        let mut complete = false;
        loop {
            if cancel.is_some_and(crate::search::SearchCancel::is_cancelled) {
                return Err(FsToolError::Cancelled);
            }
            if pending.is_empty()
                && !finished.iter().any(|directory| directory == &root)
                && read_budget > 0
            {
                let listing_names = partial_names(&partials, &root);
                let listed = push_children(&mut ChildListing {
                    memory: &mut walk,
                    root_key: &root_key,
                    root_info: &root_info,
                    resume: continuation,
                    pending: &mut pending,
                    directory: &root,
                    absolute: &final_path,
                    budget: read_budget,
                    consumed: &listing_names,
                    cancel,
                })?;
                charged = charged.saturating_add(listed.progress);
                read_budget = read_budget.saturating_sub(listed.progress);
                read_truncated = read_truncated || listed.truncated;
                if listed.truncated {
                    remember_partial(
                        &mut partials,
                        root.clone(),
                        merge_consumed(&listing_names, &listed.accepted),
                    );
                } else {
                    finished.push(root.clone());
                    forget_partial(&mut partials, &root);
                }
                if pending.is_empty() {
                    complete =
                        finished.iter().any(|directory| directory == &root) && !read_truncated;
                    break;
                }
            }
            let Some(candidate) = pending.pop() else {
                complete = finished.iter().any(|directory| directory == &root) && !read_truncated;
                break;
            };
            let relative = candidate.relative_path.clone();
            let restored_entry = restored.contains(&relative);
            if recursive
                && candidate.kind == FsNodeKind::Directory
                && !finished.iter().any(|directory| directory == &relative)
            {
                if read_budget == 0 {
                    pending.push(candidate);
                    complete = false;
                    break;
                }
                let child_names = partial_names(&partials, &relative);
                let child_absolute = candidate.absolute.clone();
                let listed = push_children(&mut ChildListing {
                    memory: &mut walk,
                    root_key: &root_key,
                    root_info: &root_info,
                    resume: continuation,
                    pending: &mut pending,
                    directory: &relative,
                    absolute: &child_absolute,
                    budget: read_budget,
                    consumed: &child_names,
                    cancel,
                })?;
                charged = charged.saturating_add(listed.progress);
                read_budget = read_budget.saturating_sub(listed.progress);
                read_truncated = read_truncated || listed.truncated;
                if listed.truncated {
                    remember_partial(
                        &mut partials,
                        relative.clone(),
                        merge_consumed(&child_names, &listed.accepted),
                    );
                } else {
                    finished.push(relative.clone());
                    forget_partial(&mut partials, &relative);
                }
            }
            if !restored_entry && after.as_ref().is_some_and(|after| relative <= *after) {
                continue;
            }
            if patterns.iter().any(|pattern| pattern.matches(&candidate.name)) {
                matches.push(FsExternalFindMatch {
                    relative_path: relative.clone(),
                    name: candidate.name.clone(),
                    kind: candidate.kind,
                    byte_length: candidate.byte_length,
                });
            }
            examined_to = Some(relative);
            if matches.len() >= limit as usize {
                complete = false;
                break;
            }
        }
        if read_truncated
            || !pending.is_empty()
            || partials
                .iter()
                .any(|item| !finished.iter().any(|directory| directory == &item.directory))
        {
            complete = false;
        }
        let partial_names_len: usize = partials.iter().map(|item| item.consumed_names.len()).sum();
        if partial_names_len > 262_144 || pending.len() > 262_144 {
            return Err(FsToolError::InvalidRequest(
                "the external find listing cursor is full; narrow the directory".into(),
            ));
        }
        ensure_same_file(&final_path, &identity)?;
        let pending_left = pending
            .into_vec()
            .into_iter()
            .map(|entry| FsExternalPending {
                relative_path: entry.relative_path,
                name: entry.name,
                kind: entry.kind,
                byte_length: entry.byte_length,
            })
            .collect::<Vec<_>>();
        let mut result = FsExternalFindResult {
            grant_id: target.grant_id,
            display_path: target.display_path,
            relative_path: target.relative_path,
            matches,
            scope_revision: external_scope_revision(
                &identity,
                modified_unix_ms(target.directory.file()),
            )?,
            complete,
            truncated: !complete,
            scanned_entries: charged,
            resume_after: (!complete)
                .then(|| examined_to.clone())
                .flatten()
                .or_else(|| after.clone()),
            consumed_names: if complete {
                Vec::new()
            } else {
                partials
                    .iter()
                    .find(|item| item.directory == root)
                    .or_else(|| partials.last())
                    .map(|item| item.consumed_names.clone())
                    .unwrap_or_default()
            },
            listing_directory: if complete {
                None
            } else {
                partials
                    .iter()
                    .find(|item| item.directory == root)
                    .or_else(|| partials.last())
                    .map(|item| item.directory.clone())
            },
            pending: if complete { Vec::new() } else { pending_left },
            finished_directories: if complete { Vec::new() } else { finished },
            partial_listings: if complete { Vec::new() } else { partials },
        };
        if !result.complete {
            let outgoing = find_cursor_key(&FindCursorParts {
                session: &session_key,
                relative: result.relative_path.as_str(),
                file_names: &file_names,
                recursive,
                glob: matches!(match_mode, ExternalNameMatch::Glob),
                case_sensitive,
                after: result.resume_after.as_ref(),
                consumed: &result.consumed_names,
                listing: result.listing_directory.as_ref(),
                pending: &result.pending,
                finished: &result.finished_directories,
                partials: &result.partial_listings,
            });
            let generation = generation.saturating_add(1);
            result.finished_directories.push(find_query_token(query_id, generation)?);
            self.find_queries
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .store_issued(query_id, generation, outgoing, walk);
        }
        Ok(result)
    }
}

pub fn external_identity_revision(
    identity: &aura_domain_common::FileIdentity,
) -> Result<Revision, FsToolError> {
    let digest = aura_domain_common::jcs::canonical_sha256(identity)
        .map_err(|_| FsToolError::Internal("external root identity digest".into()))?;
    let mut token = 0_u64;
    for character in digest.hex().chars().take(16) {
        token = (token << 4) | u64::from(character.to_digit(16).unwrap_or(0));
    }
    Ok(Revision::new(token))
}

/// Root identity mixed with the directory's own modification time.
///
/// Two pages of an unchanged directory share one token. A directory whose
/// entries were added, removed, or replaced reports a different token, so a
/// continuation of the old token is rejected instead of being treated as the
/// same scope.
pub fn external_scope_revision(
    identity: &aura_domain_common::FileIdentity,
    modified_unix_ms: u64,
) -> Result<Revision, FsToolError> {
    let base = external_identity_revision(identity)?.value();
    Ok(Revision::new(base ^ modified_unix_ms.wrapping_mul(0x9E37_79B9_7F4A_7C15)))
}

fn modified_unix_ms(file: &std::fs::File) -> u64 {
    file.metadata()
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

fn path_modified(path: &Path) -> Result<std::time::SystemTime, FsToolError> {
    std::fs::metadata(path).and_then(|meta| meta.modified()).map_err(|source| {
        crate::error::io_error("read the authorized external directory time", path, source)
    })
}

/// Object identities and unfinished directory iterators for one process.
///
/// A continuation cursor can be echoed or edited by the model. This memory is
/// the service's own record of what it listed, so a later page can resume a
/// directory without reading its prefix again and can reject an object that
/// was replaced between pages.
#[derive(Default)]
pub(crate) struct ExternalWalkMemory {
    entries: std::collections::HashMap<String, RememberedObject>,
}

/// One issued find. The same request started twice gets two ids, and a page
/// token continues only that id's current generation.
struct IssuedFind {
    generation: u64,
    body: u64,
    memory: ExternalWalkMemory,
}

/// Iterators that belong to one issued query, not to a stack of anonymous cursors.
///
/// Two lookups of the same directory are two queries. Replaying a page that was
/// already continued does not take the other query's iterator. `finished`,
/// `pending` and `consumed` do not by themselves prove that a directory was read.
#[derive(Default)]
pub(crate) struct FindQueryTable {
    next_id: u64,
    slots: std::collections::HashMap<u64, IssuedFind>,
    recent: std::collections::VecDeque<u64>,
}

impl FindQueryTable {
    const CAPACITY: usize = 64;

    fn allocate(&mut self) -> u64 {
        self.next_id = self.next_id.saturating_add(1);
        self.next_id
    }

    /// Removes the page only when the id, generation and echoed body all match.
    /// A mismatch leaves the issued page in place for its own continuation.
    fn take_issued(&mut self, id: u64, generation: u64, body: u64) -> Option<ExternalWalkMemory> {
        let slot = self.slots.get(&id)?;
        if slot.generation != generation || slot.body != body {
            return None;
        }
        let slot = self.slots.remove(&id)?;
        self.recent.retain(|item| *item != id);
        Some(slot.memory)
    }

    fn store_issued(&mut self, id: u64, generation: u64, body: u64, memory: ExternalWalkMemory) {
        let replacing = self.slots.contains_key(&id);
        if !replacing
            && self.slots.len() >= Self::CAPACITY
            && let Some(old) = self.recent.pop_front()
        {
            self.slots.remove(&old);
        }
        self.recent.retain(|item| *item != id);
        self.recent.push_back(id);
        self.slots.insert(id, IssuedFind { generation, body, memory });
    }
}

const FIND_QUERY_PREFIX: &str = "aura-find-query";

fn find_query_token(id: u64, generation: u64) -> Result<WorkspacePath, FsToolError> {
    WorkspacePath::parse(format!("{FIND_QUERY_PREFIX}/{id}/{generation}")).map_err(|_| {
        FsToolError::InvalidRequest(
            "external find continuation is not a page this service issued".into(),
        )
    })
}

/// Pulls the service-issued query identity out of the echoed cursor.
///
/// The token rides in `finished_directories` because that field is already part
/// of the cursor a continuation copies back. It is not a directory the walk
/// finished, and a cursor that carries cursor fields without a token is not a
/// page this process issued.
fn take_find_query_token(
    finished: &mut Vec<WorkspacePath>,
) -> Result<Option<(u64, u64)>, FsToolError> {
    let mut token = None;
    let mut rejected = false;
    finished.retain(|path| {
        let mut parts = path.as_str().split('/');
        if parts.next() != Some(FIND_QUERY_PREFIX) {
            return true;
        }
        let id = parts.next().and_then(|value| value.parse::<u64>().ok());
        let generation = parts.next().and_then(|value| value.parse::<u64>().ok());
        if parts.next().is_some() {
            rejected = true;
            return false;
        }
        match (id, generation) {
            (Some(id), Some(generation)) if token.is_none() => token = Some((id, generation)),
            _ => rejected = true,
        }
        false
    });
    if rejected {
        return Err(FsToolError::InvalidRequest(
            "external find continuation is not a page this service issued".into(),
        ));
    }
    Ok(token)
}

struct FindCursorParts<'a> {
    session: &'a str,
    relative: &'a str,
    file_names: &'a [String],
    recursive: bool,
    glob: bool,
    case_sensitive: bool,
    after: Option<&'a WorkspacePath>,
    consumed: &'a [String],
    listing: Option<&'a WorkspacePath>,
    pending: &'a [FsExternalPending],
    finished: &'a [WorkspacePath],
    partials: &'a [FsExternalPartialListing],
}

fn mix_cursor(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    *hash ^= 0xff;
    *hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
}

fn find_cursor_key(parts: &FindCursorParts<'_>) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    mix_cursor(&mut hash, parts.session.as_bytes());
    mix_cursor(&mut hash, parts.relative.as_bytes());
    for name in parts.file_names {
        mix_cursor(&mut hash, name.as_bytes());
    }
    mix_cursor(
        &mut hash,
        &[u8::from(parts.recursive), u8::from(parts.glob), u8::from(parts.case_sensitive)],
    );
    mix_cursor(&mut hash, parts.after.map(WorkspacePath::as_str).unwrap_or("").as_bytes());
    for name in parts.consumed {
        mix_cursor(&mut hash, name.as_bytes());
    }
    mix_cursor(&mut hash, parts.listing.map(WorkspacePath::as_str).unwrap_or("").as_bytes());
    for item in parts.pending {
        mix_cursor(&mut hash, item.relative_path.as_str().as_bytes());
        mix_cursor(&mut hash, item.name.as_bytes());
        mix_cursor(
            &mut hash,
            &[match item.kind {
                FsNodeKind::File => 1,
                FsNodeKind::Directory => 2,
                FsNodeKind::ReparsePoint => 3,
            }],
        );
        mix_cursor(
            &mut hash,
            item.byte_length.map(|len| len.to_string()).unwrap_or_default().as_bytes(),
        );
    }
    for path in parts.finished {
        mix_cursor(&mut hash, path.as_str().as_bytes());
    }
    for partial in parts.partials {
        mix_cursor(&mut hash, partial.directory.as_str().as_bytes());
        for name in &partial.consumed_names {
            mix_cursor(&mut hash, name.as_bytes());
        }
    }
    hash
}

struct RememberedObject {
    identity: aura_windows_platform::NativeFileIdentity,
    modified: std::time::SystemTime,
    is_reparse: bool,
    unread: Option<std::fs::ReadDir>,
}

impl ExternalWalkMemory {
    fn key(root_key: &str, relative: &WorkspacePath) -> String {
        format!("{root_key}\n{}", relative.as_str())
    }

    fn recall(&self, root_key: &str, relative: &WorkspacePath) -> Option<&RememberedObject> {
        self.entries.get(&Self::key(root_key, relative))
    }

    fn remember(&mut self, root_key: &str, relative: &WorkspacePath, object: RememberedObject) {
        let key = Self::key(root_key, relative);
        if let Some(existing) = self.entries.get_mut(&key) {
            let unread = object.unread.or_else(|| existing.unread.take());
            *existing = RememberedObject { unread, ..object };
            return;
        }
        self.entries.insert(key, object);
    }

    fn take_unread(
        &mut self,
        root_key: &str,
        relative: &WorkspacePath,
    ) -> Option<std::fs::ReadDir> {
        self.entries.get_mut(&Self::key(root_key, relative)).and_then(|entry| entry.unread.take())
    }

    fn store_unread(&mut self, root_key: &str, relative: &WorkspacePath, unread: std::fs::ReadDir) {
        if let Some(entry) = self.entries.get_mut(&Self::key(root_key, relative)) {
            entry.unread = Some(unread);
        }
    }

    fn drop_unread(&mut self, root_key: &str, relative: &WorkspacePath) {
        if let Some(entry) = self.entries.get_mut(&Self::key(root_key, relative)) {
            entry.unread = None;
        }
    }
}

fn identity_key(identity: &aura_windows_platform::NativeFileIdentity) -> String {
    let mut text = format!("{:x}:", identity.volume_serial_number);
    for byte in identity.file_id {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn kind_of(info: &aura_windows_platform::NativePathInfo) -> FsNodeKind {
    if info.is_reparse_point {
        FsNodeKind::ReparsePoint
    } else if info.is_directory {
        FsNodeKind::Directory
    } else {
        FsNodeKind::File
    }
}

fn contained_path(
    root_relative: &WorkspacePath,
    root_absolute: &Path,
    relative: &WorkspacePath,
) -> Result<PathBuf, FsToolError> {
    let root = root_relative.as_str().trim_matches('/');
    let full = relative.as_str().trim_matches('/');
    let rest = if root.is_empty() {
        full
    } else {
        full.strip_prefix(root).ok_or_else(|| {
            FsToolError::InvalidRequest(
                "external find continuation names a path outside the authorized directory".into(),
            )
        })?
    }
    .trim_start_matches('/');
    if rest
        .split('/')
        .any(|component| component.is_empty() || component == "." || component == "..")
        && !rest.is_empty()
    {
        return Err(FsToolError::InvalidRequest(
            "external find continuation names a path outside the authorized directory".into(),
        ));
    }
    Ok(if rest.is_empty() { root_absolute.to_path_buf() } else { root_absolute.join(rest) })
}

/// One entry the external walk has produced but not yet examined.
struct PendingExternalEntry {
    relative_path: WorkspacePath,
    absolute: PathBuf,
    name: String,
    kind: FsNodeKind,
    byte_length: Option<u64>,
}

impl PartialEq for PendingExternalEntry {
    fn eq(&self, other: &Self) -> bool {
        self.relative_path == other.relative_path
    }
}
impl Eq for PendingExternalEntry {}
impl PartialOrd for PendingExternalEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for PendingExternalEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // `BinaryHeap` is a max-heap, so the ordering is reversed to pop the smallest
        // path first.
        other.relative_path.cmp(&self.relative_path)
    }
}

/// Lists one external directory into the walk's pending set. Directories are
/// reported as entries; whether they are descended is the caller's decision.
/// Reads one authorized external directory into the walk's pending heap.
///
/// The budget is spent on what the walk really does — one unit per directory entry
/// read, including the metadata that decides its kind and size — instead of on the
/// number of entries a page happens to return. A directory wider than what is left of
/// the budget stops there and says so: the caller must then treat the walk as
/// incomplete, because the entries it did not read were never examined. Reporting
/// "no match" after a partial directory read is exactly the false absence this
/// prevents.
///
/// An entry that cannot be represented or read is an error, not a silent skip: a name
/// the platform cannot hand over as UTF-8, or metadata that cannot be read, means the
/// walk does not know what is there, and a model that is told "complete" would treat a
/// file as absent.
struct DirectoryListing {
    progress: u64,
    truncated: bool,
    accepted: Vec<String>,
}

fn partial_names(partials: &[FsExternalPartialListing], directory: &WorkspacePath) -> Vec<String> {
    partials
        .iter()
        .find(|item| &item.directory == directory)
        .map(|item| item.consumed_names.clone())
        .unwrap_or_default()
}

fn remember_partial(
    partials: &mut Vec<FsExternalPartialListing>,
    directory: WorkspacePath,
    names: Vec<String>,
) {
    if let Some(existing) = partials.iter_mut().find(|item| item.directory == directory) {
        existing.consumed_names = names;
    } else if !names.is_empty() {
        partials.push(FsExternalPartialListing { directory, consumed_names: names });
    }
}

fn forget_partial(partials: &mut Vec<FsExternalPartialListing>, directory: &WorkspacePath) {
    partials.retain(|item| &item.directory != directory);
}

fn merge_consumed(previous: &[String], accepted: &[String]) -> Vec<String> {
    let mut names = previous.to_vec();
    for name in accepted {
        if !names.iter().any(|existing| existing == name) {
            names.push(name.clone());
        }
    }
    names
}

/// The names a page may take from one unordered listing.
///
/// Already-consumed names are not progress: the next page has to get past them to new
/// names. Stopping after `budget` new names does not use the largest path as a cursor,
/// so a later smaller name is still taken on a later page.
#[cfg(test)]
pub(crate) fn take_new_names<'a>(
    names: impl Iterator<Item = &'a str>,
    consumed: &std::collections::HashSet<&str>,
    budget: u64,
) -> (Vec<&'a str>, bool) {
    let mut accepted = Vec::new();
    let mut truncated = false;
    for name in names {
        if consumed.contains(name) {
            continue;
        }
        if accepted.len() as u64 >= budget {
            truncated = true;
            break;
        }
        accepted.push(name);
    }
    (accepted, truncated)
}

struct ChildListing<'a> {
    memory: &'a mut ExternalWalkMemory,
    root_key: &'a str,
    root_info: &'a aura_windows_platform::NativePathInfo,
    /// When set, resume a directory iterator this service already opened and
    /// reject the directory if its object or modification time changed.
    resume: bool,
    pending: &'a mut std::collections::BinaryHeap<PendingExternalEntry>,
    directory: &'a WorkspacePath,
    absolute: &'a Path,
    budget: u64,
    consumed: &'a [String],
    cancel: Option<&'a crate::search::SearchCancel>,
}

fn inspect_contained(
    root: &aura_windows_platform::NativePathInfo,
    absolute: &Path,
) -> Result<aura_windows_platform::NativePathInfo, FsToolError> {
    let info = aura_windows_platform::inspect_existing_path(absolute, false).map_err(|source| {
        if source.kind() == std::io::ErrorKind::NotFound {
            FsToolError::InvalidRequest(
                "external find continuation names an entry that is not in the authorized directory"
                    .into(),
            )
        } else {
            crate::error::io_error("open an authorized external entry", absolute, source)
        }
    })?;
    if !final_path_within(root, &info) {
        return Err(FsToolError::ExternalFileReplaced { path: absolute.to_path_buf() });
    }
    Ok(info)
}

fn materialize_pending(
    memory: &mut ExternalWalkMemory,
    root_key: &str,
    root_info: &aura_windows_platform::NativePathInfo,
    root_relative: &WorkspacePath,
    root_absolute: &Path,
    item: FsExternalPending,
) -> Result<PendingExternalEntry, FsToolError> {
    let absolute = contained_path(root_relative, root_absolute, &item.relative_path)?;
    let info = inspect_contained(root_info, &absolute)?;
    let matches_seen = memory
        .recall(root_key, &item.relative_path)
        .map(|seen| seen.identity == info.identity && seen.is_reparse == info.is_reparse_point);
    if matches_seen == Some(false) {
        memory.drop_unread(root_key, &item.relative_path);
        return Err(FsToolError::ExternalFileReplaced { path: absolute });
    }
    if matches_seen.is_none() && info.is_reparse_point && item.kind == FsNodeKind::Directory {
        return Err(FsToolError::ExternalFileReplaced { path: absolute });
    }
    if item.kind == FsNodeKind::Directory && (info.is_reparse_point || !info.is_directory) {
        return Err(FsToolError::ExternalFileReplaced { path: absolute });
    }
    let kind = kind_of(&info);
    let byte_length = (kind == FsNodeKind::File)
        .then(|| std::fs::metadata(&absolute).map(|meta| meta.len()).unwrap_or(0));
    let name = absolute
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(item.name.as_str())
        .to_owned();
    if !info.is_reparse_point
        && memory.recall(root_key, &item.relative_path).is_none()
        && let Ok(modified) = path_modified(&absolute)
    {
        memory.remember(
            root_key,
            &item.relative_path,
            RememberedObject {
                identity: info.identity.clone(),
                modified,
                is_reparse: info.is_reparse_point,
                unread: None,
            },
        );
    }
    Ok(PendingExternalEntry {
        relative_path: item.relative_path,
        absolute,
        name,
        kind,
        byte_length,
    })
}

/// One authorized external directory to search for text.
pub struct ExternalSearchTarget {
    /// The session grant that authorized this target, when a grant did.
    pub grant_id: Option<ExternalGrantId>,
    pub session_id: SessionId,
    /// The absolute directory path as the user sees it.
    pub display_path: String,
    /// The directory relative to the grant root.
    pub relative_path: WorkspacePath,
    pub directory: VerifiedFile,
}

pub struct FsExternalSearchRequest {
    pub target: ExternalSearchTarget,
    pub pattern: FsSearchPattern,
    pub recursive: bool,
    pub limit: u32,
    /// How many directory entries this call may examine in total.
    pub scan_budget: u64,
    /// Files larger than this are not read, and the result says it is incomplete.
    pub max_file_bytes: u64,
    /// How many bytes of file content this call may read in total.
    pub byte_budget: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FsExternalSearchMatch {
    pub relative_path: WorkspacePath,
    pub name: String,
    /// One-based line number, as a reader counts lines.
    pub line_number: u64,
    /// Where the match starts inside the line, and how many characters it covers.
    pub column_character_offset: u64,
    pub match_character_length: u64,
    pub line_text: String,
    /// True when the line was longer than the excerpt kept here.
    pub line_truncated: bool,
    /// Hash of the file content this match came from.
    pub logical_hash: ContentHash,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FsExternalSearchResult {
    pub grant_id: Option<ExternalGrantId>,
    pub display_path: String,
    pub matches: Vec<FsExternalSearchMatch>,
    pub scanned_entries: u64,
    /// Files whose content was actually read.
    pub scanned_file_count: u64,
    pub scanned_bytes: u64,
    /// True only when the walk finished and every file it saw was searched.
    pub complete: bool,
    pub skipped_too_large: u64,
    pub skipped_not_text: u64,
}

impl FsToolService {
    /// Searches text under one already-authorized external directory.
    ///
    /// An external tree has no index, so this reads real files: a bounded walk in path
    /// order that never follows a reparse point, and a bounded read of each file it
    /// examines. Every bound is real — entries examined, bytes read — so a wide or deep
    /// tree cannot turn one call into an unbounded scan, and `complete` is true only when
    /// the walk finished and every file it saw was searched. A file that is too large,
    /// or that is not text, is counted and reported instead of being silently skipped:
    /// "no match" must never be claimed for content nobody read.
    pub fn search_external(
        &self,
        request: FsExternalSearchRequest,
        cancel: Option<&crate::search::SearchCancel>,
    ) -> Result<FsExternalSearchResult, FsToolError> {
        let FsExternalSearchRequest {
            target,
            pattern,
            recursive,
            limit,
            scan_budget,
            max_file_bytes,
            byte_budget,
        } = request;
        let final_path = target.directory.info().final_path.clone();
        if !target.directory.info().is_directory {
            return Err(FsToolError::InvalidRequest(
                "an external search root must name a directory".into(),
            ));
        }
        let identity = aura_workspace::domain_identity(&target.directory.info().identity);
        let matcher = match &pattern {
            FsSearchPattern::Literal { text, case_sensitive } => {
                let mut builder = regex::RegexBuilder::new(&regex::escape(text));
                builder.case_insensitive(!case_sensitive);
                builder.build().map_err(|error| FsToolError::InvalidRequest(error.to_string()))?
            }
            FsSearchPattern::Regex { pattern, case_sensitive } => {
                let mut builder = regex::RegexBuilder::new(pattern);
                builder.case_insensitive(!case_sensitive);
                builder.build().map_err(|error| FsToolError::InvalidRequest(error.to_string()))?
            }
        };
        let mut pending = std::collections::BinaryHeap::new();
        let mut read_budget = scan_budget;
        let root_info = target.directory.info().clone();
        let root_key = identity_key(&root_info.identity);
        let mut listings = ExternalWalkMemory::default();
        let listed = push_children(&mut ChildListing {
            memory: &mut listings,
            root_key: &root_key,
            root_info: &root_info,
            resume: false,
            pending: &mut pending,
            directory: &target.relative_path,
            absolute: &final_path,
            budget: read_budget,
            consumed: &[],
            cancel,
        })?;
        read_budget = read_budget.saturating_sub(listed.progress);
        let mut read_truncated = listed.truncated;
        let mut scanned_entries = 0_u64;
        let mut scanned_file_count = 0_u64;
        let mut scanned_bytes = 0_u64;
        let mut matches = Vec::new();
        let mut skipped_too_large = 0_u64;
        let mut skipped_not_text = 0_u64;
        let mut complete = true;
        while let Some(candidate) = pending.pop() {
            if cancel.is_some_and(crate::search::SearchCancel::is_cancelled) {
                return Err(FsToolError::Cancelled);
            }
            if candidate.kind == FsNodeKind::Directory {
                if recursive {
                    let child_absolute = candidate.absolute.clone();
                    let listed = push_children(&mut ChildListing {
                        memory: &mut listings,
                        root_key: &root_key,
                        root_info: &root_info,
                        resume: false,
                        pending: &mut pending,
                        directory: &candidate.relative_path,
                        absolute: &child_absolute,
                        budget: read_budget,
                        consumed: &[],
                        cancel,
                    })?;
                    read_budget = read_budget.saturating_sub(listed.progress);
                    read_truncated = read_truncated || listed.truncated;
                }
                continue;
            }
            scanned_entries = scanned_entries.saturating_add(1);
            if scanned_entries > scan_budget {
                complete = false;
                break;
            }
            // A reparse point is reported but never followed or read.
            if candidate.kind == FsNodeKind::ReparsePoint {
                continue;
            }
            let size = candidate.byte_length.unwrap_or(0);
            if size > max_file_bytes {
                skipped_too_large = skipped_too_large.saturating_add(1);
                complete = false;
                continue;
            }
            if scanned_bytes.saturating_add(size) > byte_budget {
                complete = false;
                break;
            }
            // The bytes come through the verified handle the platform opened, not from the
            // path the listing spelled: a name that was replaced between the listing and
            // this read is reported as an unfinished search instead of being searched as
            // if it were the file the walk saw.
            let Ok(mut file) = aura_windows_platform::open_verified_existing(
                &candidate.absolute,
                aura_windows_platform::NativeFileAccess::Read,
            ) else {
                // A file that cannot be read is not a file that does not match.
                complete = false;
                continue;
            };
            if file.info().is_directory
                || !final_path_within(&root_info, file.info())
                || !listings
                    .recall(&root_key, &candidate.relative_path)
                    .is_some_and(|seen| seen.identity == file.info().identity)
                || !crate::read_scope::allows(file.info())
            {
                complete = false;
                continue;
            }
            let Some(read) = read_via_handle(&mut file, size) else {
                complete = false;
                continue;
            };
            // The bytes really read are billed whether or not they decode, so a search
            // full of non-text files cannot read them for free.
            scanned_bytes = scanned_bytes.saturating_add(read.bytes_read);
            if read.bytes_read != size {
                // The file is not the size the listing saw: it changed while it was
                // searched, so nothing about it is settled.
                complete = false;
                continue;
            }
            let Some(decoded) = crate::read::decode_snapshot(&read.bytes, None).ok() else {
                skipped_not_text = skipped_not_text.saturating_add(1);
                complete = false;
                continue;
            };
            scanned_file_count = scanned_file_count.saturating_add(1);
            let text = decoded.text;
            let logical_hash = ContentHash::blake3(&read.bytes);
            for (index, line) in text.lines().enumerate() {
                let spans: Vec<(usize, usize)> =
                    matcher.find_iter(line).map(|found| (found.start(), found.end())).collect();
                if spans.is_empty() {
                    continue;
                }
                // One ordered walk resolves every boundary on the line, and the excerpt is
                // the window around the match, so a hit far into a long line is projected
                // with its own text instead of the first bytes of the line.
                let boundaries = crate::search::excerpt::char_boundaries(line, &spans);
                let total_chars = line.chars().count();
                for ((start, end), (start_chars, end_chars)) in spans.iter().zip(&boundaries) {
                    let length = end_chars.saturating_sub(*start_chars);
                    let excerpt = crate::search::excerpt::excerpt_around_bytes(
                        line,
                        *start,
                        *end,
                        *start_chars,
                        length,
                        total_chars,
                    );
                    matches.push(FsExternalSearchMatch {
                        relative_path: candidate.relative_path.clone(),
                        name: candidate.name.clone(),
                        line_number: u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1),
                        column_character_offset: u64::try_from(*start_chars).unwrap_or(u64::MAX),
                        match_character_length: u64::try_from(length).unwrap_or(u64::MAX),
                        line_truncated: excerpt.truncated_before || excerpt.truncated_after,
                        line_text: excerpt.text,
                        logical_hash: logical_hash.clone(),
                    });
                    if matches.len() >= limit as usize {
                        // The page is full; the walk did not finish.
                        complete = false;
                        break;
                    }
                }
                if matches.len() >= limit as usize {
                    break;
                }
            }
            if matches.len() >= limit as usize {
                break;
            }
        }
        if read_truncated {
            complete = false;
        }
        ensure_same_file(&final_path, &identity)?;
        Ok(FsExternalSearchResult {
            grant_id: target.grant_id,
            display_path: target.display_path,
            matches,
            scanned_entries,
            scanned_file_count,
            scanned_bytes,
            complete,
            skipped_too_large,
            skipped_not_text,
        })
    }
}

#[cfg(test)]
mod listing_cursor_tests {
    use super::take_new_names;

    #[test]
    fn an_unordered_listing_keeps_smaller_names_for_the_next_page() {
        let names = ["z", "a", "m"];
        let (first, truncated) =
            take_new_names(names.into_iter(), &std::collections::HashSet::new(), 1);
        assert!(truncated);
        assert_eq!(first, ["z"]);
        let consumed = std::collections::HashSet::from(["z"]);
        let (second, _) = take_new_names(names.into_iter(), &consumed, 1);
        assert_eq!(second, ["a"], "the next page takes the unread smaller name");
        let consumed = std::collections::HashSet::from(["z", "a"]);
        let (third, done) = take_new_names(names.into_iter(), &consumed, 1);
        assert!(!done);
        assert_eq!(third, ["m"]);
    }
}
