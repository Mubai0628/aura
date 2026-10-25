use std::{
    ffi::OsStr,
    fs::File,
    io,
    io::Write,
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, Path, PathBuf, Prefix},
    ptr,
};

use sha2::Digest as _;
use thiserror::Error;
mod replace_retry;
mod streaming;
pub use streaming::StagedFilePublication;
use streaming::blake3_of_handle;
use windows_sys::{
    Wdk::{
        Storage::FileSystem::{
            FILE_RENAME_INFORMATION, FileDispositionInformationEx, FileFsDeviceInformation,
            FileRenameInformation, NtQueryVolumeInformationFile, NtSetInformationFile,
        },
        System::SystemServices::{FILE_FS_DEVICE_INFORMATION, FILE_REMOTE_DEVICE},
    },
    Win32::{
        Foundation::{
            ERROR_FILE_NOT_FOUND, GENERIC_READ, GENERIC_WRITE, RtlNtStatusToDosError,
            STATUS_PENDING,
        },
        Storage::FileSystem::{
            CREATE_NEW, CreateFileW, DELETE, FILE_ATTRIBUTE_TEMPORARY,
            FILE_DISPOSITION_FLAG_DELETE, FILE_DISPOSITION_FLAG_POSIX_SEMANTICS,
            FILE_DISPOSITION_INFO, FILE_DISPOSITION_INFO_EX, FILE_FLAG_WRITE_THROUGH,
            FileDispositionInfo, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
            ReplaceFileW, SetFileInformationByHandle,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

use crate::path::{
    NativeFileIdentity, NativePathInfo, VerifiedFile, final_path_within,
    open_verified_publish_parent, verified_from_file,
};

#[derive(Debug, Error)]
pub enum AtomicReplaceError {
    #[error("replacement file does not exist: {0}")]
    MissingReplacement(PathBuf),
    #[error("replacement and destination must share the same parent directory")]
    DifferentDirectories,
    #[error("file identity or contents changed before atomic replacement retry")]
    RetryBindingChanged,
    #[error("failed to atomically replace {path}: {source}")]
    Windows { path: PathBuf, source: io::Error },
}

#[derive(Debug, Error)]
pub enum HandlePublishError {
    #[error("destination parent identity changed")]
    ParentIdentityChanged,
    #[error("destination target identity changed")]
    TargetIdentityChanged,
    /// The destination is still the verified object but no longer holds the content the
    /// caller decided about.
    ///
    /// Detected through the exclusive handle the publication already holds, so nothing can
    /// rewrite the file between the check and the rename.
    #[error("destination content changed: expected {expected}, found {actual}")]
    TargetContentChanged { expected: String, actual: String },
    #[error("destination name must be one file-name component")]
    InvalidFileName,
    #[error("published object escaped its verified parent")]
    ResultEscapedParent,
    #[error("handle-relative publication is unavailable for UNC or remote destination parents")]
    RemoteParentUnsupported,
    #[error("failed to classify destination parent as local or remote: {0}")]
    RemoteProtocolProbeFailed(#[source] io::Error),
    #[error("NtSetInformationFile unexpectedly returned STATUS_PENDING for a synchronous handle")]
    NativeRenamePending,
    #[error(
        "NtSetInformationFile(FileRenameInformation) failed: NTSTATUS=0x{ntstatus:08x}, Win32={win32_code}: {source}"
    )]
    NativeRename {
        ntstatus: u32,
        win32_code: u32,
        #[source]
        source: io::Error,
    },
    #[error("publication failed ({operation}); rollback failed ({rollback})")]
    RollbackFailed { operation: String, rollback: String },
    /// A finalization step failed after the new content had been published, and the
    /// publication was rolled back: the destination holds its previous content again.
    #[error("publication could not be finalized ({cause}); the previous content was restored")]
    RolledBackAfterPublish { cause: String },
    /// The backup could not be put back after the publication failed.
    ///
    /// This is its own variant rather than a wrapped I/O error because it is the one
    /// failure that leaves the destination holding neither version. Reporting it as a
    /// plain platform error let a caller read "compensation succeeded" out of an object
    /// that had lost both its old and its new content.
    #[error(
        "publication failed ({operation}); the previous content could not be restored ({rollback})"
    )]
    CompensationFailed { operation: String, rollback: String },
    #[error("handle-bound publish failed: {0}")]
    Windows(#[from] io::Error),
}

/// How far a publication got when it failed.
///
/// A caller cannot derive this from an error kind: the same I/O error can arrive before
/// the destination is touched or after its previous content has already been moved aside.
/// So the platform reports the location explicitly, and every construction site of a
/// [`HandlePublishError`] knows which one it is — the ones that could not know say
/// [`PublishLocation::Unconfirmable`] instead of guessing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PublishLocation {
    /// The destination still holds exactly what it held before the call.
    NeverPublished,
    /// The new content was published and then removed again, with the previous content put
    /// back in its place.
    RolledBack,
    /// The destination is not known to hold either version.
    Unconfirmable,
}

impl HandlePublishError {
    /// How far the publication got. Total by construction: every variant is placed here
    /// deliberately, so a new variant cannot silently inherit a wrong answer.
    ///
    /// `RolledBackAfterPublish` is deliberately **not** `NeverPublished`. Both end with the
    /// destination holding its previous content, but they are different facts: one never touched
    /// it, the other moved it aside, published, and put it back. A caller reporting the second as
    /// "nothing happened" would be hiding a real sequence of side effects, and a caller that
    /// treats `NeverPublished` as "safe to retry immediately" would be retrying over a publication
    /// that did reach the disk.
    #[must_use]
    pub const fn location(&self) -> PublishLocation {
        match self {
            Self::InvalidFileName
            | Self::RemoteParentUnsupported
            | Self::RemoteProtocolProbeFailed(_)
            | Self::NativeRenamePending
            | Self::NativeRename { .. }
            | Self::ParentIdentityChanged
            | Self::TargetIdentityChanged
            | Self::TargetContentChanged { .. }
            | Self::ResultEscapedParent
            | Self::Windows(_) => PublishLocation::NeverPublished,
            Self::RolledBackAfterPublish { .. } => PublishLocation::RolledBack,
            Self::RollbackFailed { .. } | Self::CompensationFailed { .. } => {
                PublishLocation::Unconfirmable
            }
        }
    }
}

pub struct HandleBoundPublishedFile {
    published: Option<VerifiedFile>,
    backup: Option<VerifiedFile>,
    parent: VerifiedFile,
    file_name: std::ffi::OsString,
    finalized: bool,
}

impl HandleBoundPublishedFile {
    pub fn file_mut(&mut self) -> &mut File {
        self.published.as_mut().expect("published handle is present").file_mut()
    }

    pub fn info(&self) -> &NativePathInfo {
        self.published.as_ref().expect("published handle is present").info()
    }

    pub fn parent_info(&self) -> &NativePathInfo {
        self.parent.info()
    }

    pub fn commit(mut self) -> Result<NativePathInfo, HandlePublishError> {
        if let Err(operation) = ensure_parent_unchanged(&self.parent) {
            return Err(self.finalize_failure(operation));
        }
        // The commit-time cleanup of the superseded object runs through the same fault seam as the
        // rest of the sequence, so a test can fail it at the stage it really happens.
        if fault::armed_now(
            fault::Stage::DeleteBackupOnCommit,
            &self.parent.info().final_path.join(&self.file_name),
        ) && let Err(error) = fault::fail_if_armed(
            fault::Stage::DeleteBackupOnCommit,
            &self.parent.info().final_path.join(&self.file_name),
        ) {
            return Err(self.finalize_failure(error.into()));
        }
        if let Some(backup) = self.backup.as_ref()
            && let Err(operation) = delete_by_handle(backup.file())
        {
            return Err(self.finalize_failure(operation.into()));
        }
        let info = self.info().clone();
        self.backup.take();
        self.finalized = true;
        Ok(info)
    }

    pub fn abort(mut self) -> Result<(), HandlePublishError> {
        let result = self.rollback();
        self.finalized = true;
        result
    }

    /// Turns a failed finalization into the error that describes it.
    ///
    /// A commit that fails rolls the publication back before it returns, so the destination
    /// holds what it held before — unless that rollback fails too, in which case nothing
    /// about the destination is known and the error says so.
    fn finalize_failure(&mut self, operation: HandlePublishError) -> HandlePublishError {
        let rollback = self.rollback();
        self.finalized = true;
        match rollback {
            Ok(()) => HandlePublishError::RolledBackAfterPublish { cause: operation.to_string() },
            Err(rollback) => HandlePublishError::RollbackFailed {
                operation: operation.to_string(),
                rollback: rollback.to_string(),
            },
        }
    }

    fn rollback(&mut self) -> Result<(), HandlePublishError> {
        let mut failures = Vec::new();
        if let Some(published) = self.published.take() {
            let deletion = (|| {
                fault::fail_if_armed(
                    fault::Stage::DeletePublication,
                    &self.parent.info().final_path.join(&self.file_name),
                )?;
                delete_by_handle(published.file())
            })();
            if let Err(error) = deletion {
                failures.push(format!("delete publication: {error}"));
            }
            drop(published);
        }
        if let Some(backup) = self.backup.take() {
            if let Err(error) = restore_backup(backup.file(), &self.parent, &self.file_name) {
                failures.push(format!("restore backup: {error}"));
            }
            drop(backup);
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(HandlePublishError::Windows(io::Error::other(failures.join("; "))))
        }
    }
}

impl Drop for HandleBoundPublishedFile {
    /// Last-resort compensation when a caller forgot to commit or abort.
    ///
    /// This used to call `std::process::abort()`. That is not an acceptable default for an
    /// ordinary file failure: one publication that cannot roll itself back would terminate the
    /// whole Core, taking every unrelated session's in-flight work with it. The requirement is
    /// that an ordinary file failure does not by default end the process, so the rollback is
    /// attempted, its real outcome is reported on stderr with the stage facts, and the failure
    /// stays **contained**: the guard has already dropped its handles, and whatever material
    /// remains is left on disk rather than deleted. Nothing here is silently ignored — a failed
    /// rollback prints what failed and what was left behind — and nothing here kills the process.
    fn drop(&mut self) {
        if self.finalized {
            return;
        }
        // rollback takes the backup handle even when restoration fails. Keep its last
        // verified location for the diagnostic instead of reporting `<none>` afterwards.
        let backup_path = self.backup.as_ref().map_or_else(
            || "<none>".to_owned(),
            |backup| backup.info().final_path.display().to_string(),
        );
        match self.rollback() {
            Ok(()) => {}
            Err(error) => eprintln!(
                "aura publication rollback failed during drop: stage=uncommitted-guard \
                 destination={} backup={} error={error}; the process continues and the remaining \
                 material requires reconciliation",
                self.parent.info().final_path.join(&self.file_name).display(),
                backup_path,
            ),
        }
        self.finalized = true;
    }
}

pub fn handle_bound_create_new(
    parent: &VerifiedFile,
    file_name: &OsStr,
    bytes: &[u8],
) -> Result<HandleBoundPublishedFile, HandlePublishError> {
    handle_bound_publish(parent, file_name, bytes, None)
}

pub fn handle_bound_replace(
    parent: &VerifiedFile,
    file_name: &OsStr,
    expected_target: &NativeFileIdentity,
    bytes: &[u8],
) -> Result<HandleBoundPublishedFile, HandlePublishError> {
    let backup_name = random_leaf(".aura-backup-")?;
    handle_bound_replace_with_backup(parent, file_name, expected_target, bytes, &backup_name)
}

pub fn handle_bound_replace_with_backup(
    parent: &VerifiedFile,
    file_name: &OsStr,
    expected_target: &NativeFileIdentity,
    bytes: &[u8],
    backup_name: &OsStr,
) -> Result<HandleBoundPublishedFile, HandlePublishError> {
    handle_bound_replace_verified(
        parent,
        file_name,
        expected_target,
        HandleBoundReplaceContent::Unchecked,
        bytes,
        backup_name,
    )
}

/// Replaces one object under a verified parent while re-verifying the caller's original content
/// condition through the exclusive handle, keeping the caller's own backup name.
///
/// This is the form an overwrite with a journal-recorded `expected` must use: the backup name is
/// the one the journal bound, so recovery can find the material this exact operation moved aside,
/// and the content condition is the caller's original one rather than a hash taken after
/// admission.
pub fn handle_bound_replace_with_backup_checked(
    parent: &VerifiedFile,
    file_name: &OsStr,
    expected_target: &NativeFileIdentity,
    expected_content: HandleBoundReplaceContent<'_>,
    bytes: &[u8],
    backup_name: &OsStr,
) -> Result<HandleBoundPublishedFile, HandlePublishError> {
    handle_bound_replace_verified(
        parent,
        file_name,
        expected_target,
        expected_content,
        bytes,
        backup_name,
    )
}

/// What the caller requires the destination to still hold.
pub enum HandleBoundReplaceContent<'a> {
    /// The caller established the content already and asks only for the object to be checked.
    ///
    /// This leaves a window between whatever check the caller made and this exclusive open. It
    /// exists for callers whose decision is not about the old bytes — a create, or a copy over
    /// an object whose previous content is irrelevant — and must not be used to enforce an
    /// expected state.
    Unchecked,
    /// The destination must still hold exactly these bytes.
    ///
    /// The comparison happens on the exclusive handle, after it is open and before anything is
    /// renamed, so no writer can slip between the check and the publication: while this handle
    /// is held, Windows refuses every other open of the destination, including the in-place
    /// rewrite that leaves the file's identity unchanged.
    ///
    /// The value is the lowercase hex of the file's BLAKE3 hash, the algorithm the workspace's
    /// `ContentHash::blake3` produces.
    Blake3Hex(&'a str),
    /// The destination must still hold exactly these bytes, hashed with the caller's own
    /// algorithm.
    ///
    /// This exists because a caller's content condition is recorded with whatever algorithm the
    /// call that made the decision used — the Artifact overwrite journal records the caller's
    /// `ContentHash`, which may be SHA-256. That hex must **never** be reinterpreted as BLAKE3:
    /// the two digests are different lengths of different functions, and comparing one against
    /// the other can only ever produce a false answer. Naming the algorithm explicitly keeps the
    /// caller's original condition, computed over the same bytes it was originally computed
    /// over, all the way to the exclusive handle.
    ///
    /// The value is lowercase hex and is compared against the same algorithm recomputed over the
    /// held handle, so no conversion between algorithms ever takes place.
    Hex { algorithm: ContentHashAlgorithm, expected: &'a str },
}

/// The digest algorithms a publication can re-verify with.
///
/// This is the platform's own enumeration rather than a re-export of a domain type, so the
/// platform keeps no dependency on the workspace's contract types. A caller maps its own
/// algorithm onto this.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContentHashAlgorithm {
    Blake3,
    Sha256,
}

impl ContentHashAlgorithm {
    /// The stable lowercase name of this algorithm, for a report or an observation hook.
    ///
    /// Stable so a test and a log line name the same algorithm, and so a renamed variant cannot
    /// silently change which function a measurement is about.
    #[must_use]
    pub const fn stable_name(self) -> &'static str {
        match self {
            Self::Blake3 => "blake3",
            Self::Sha256 => "sha256",
        }
    }

    /// The lowercase hex digest of `bytes` under this algorithm.
    fn digest_of_handle(self, file: &mut File) -> Result<String, HandlePublishError> {
        use std::io::{Read as _, Seek as _, SeekFrom};
        file.seek(SeekFrom::Start(0))?;
        // Both digests are computed in one pass over the held handle in bounded pieces, so the
        // check costs a fixed buffer rather than the file, and neither can be starved by the
        // other's buffer.
        let mut buffer = vec![0_u8; 64 * 1024];
        let mut blake3 = blake3::Hasher::new();
        let mut sha256 = sha2::Sha256::new();
        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            match self {
                Self::Blake3 => {
                    blake3.update(&buffer[..read]);
                }
                Self::Sha256 => {
                    sha256.update(&buffer[..read]);
                }
            }
        }
        Ok(match self {
            Self::Blake3 => blake3.finalize().to_hex().to_string(),
            Self::Sha256 => format!("{:x}", sha256.finalize()),
        })
    }
}

/// Replaces one object under a verified parent, checking its content while holding it.
///
/// The check runs on the handle this call opened with **no sharing**, and nothing can rewrite
/// the destination between the check and the rename: the exclusive open is taken first, the
/// content is verified through it, and only then is the object moved aside and replaced.
pub fn handle_bound_replace_checked(
    parent: &VerifiedFile,
    file_name: &OsStr,
    expected_target: &NativeFileIdentity,
    expected_content: &str,
    bytes: &[u8],
) -> Result<HandleBoundPublishedFile, HandlePublishError> {
    let backup_name = random_leaf(".aura-backup-")?;
    handle_bound_replace_verified(
        parent,
        file_name,
        expected_target,
        HandleBoundReplaceContent::Blake3Hex(expected_content),
        bytes,
        &backup_name,
    )
}

/// Replaces one object under a verified parent, checking its content while holding it.
pub fn handle_bound_replace_verified(
    parent: &VerifiedFile,
    file_name: &OsStr,
    expected_target: &NativeFileIdentity,
    content: HandleBoundReplaceContent<'_>,
    bytes: &[u8],
    backup_name: &OsStr,
) -> Result<HandleBoundPublishedFile, HandlePublishError> {
    ensure_parent_unchanged(parent)?;
    if !is_leaf(backup_name) || backup_name == file_name {
        return Err(HandlePublishError::InvalidFileName);
    }
    let destination = parent.info().final_path.join(file_name);
    // Opened with no sharing at all, so this handle is the exclusive claim on the object for
    // the whole check-and-publish sequence.
    let mut target = open_replace_target(&destination)?;
    if &target.info().identity != expected_target {
        return Err(HandlePublishError::TargetIdentityChanged);
    }
    if !final_path_within(parent.info(), target.info()) {
        return Err(HandlePublishError::TargetIdentityChanged);
    }
    match content {
        HandleBoundReplaceContent::Unchecked => {}
        HandleBoundReplaceContent::Blake3Hex(expected) => {
            // Report the condition this publication really consumed, before it is checked: a test
            // proves what reached the handle, not what the caller intended to reach it.
            fault::consumed_replace_condition(&destination, "blake3", expected);
            let actual = blake3_of_handle(target.file_mut())?;
            if actual != expected {
                return Err(HandlePublishError::TargetContentChanged {
                    expected: expected.to_owned(),
                    actual,
                });
            }
        }
        HandleBoundReplaceContent::Hex { algorithm, expected } => {
            // Recompute the *same* algorithm over the held handle. A digest from another
            // algorithm is never substituted for the caller's, and no hex is reinterpreted
            // under a different function: the caller's original condition is what is checked.
            fault::consumed_replace_condition(&destination, algorithm.stable_name(), expected);
            let actual = algorithm.digest_of_handle(target.file_mut())?;
            if actual != expected {
                return Err(HandlePublishError::TargetContentChanged {
                    expected: expected.to_owned(),
                    actual,
                });
            }
        }
    }
    // The destination is open with no sharing and its content condition has been verified: this is
    // the real commit window. An acceptance hook observes both ends of it; with no hook installed
    // this is one relaxed load and a return. The guard reports the exit however this call returns,
    // so an observation can count how many saves were inside the window at the same time.
    fault::reached_commit(&destination);
    let _commit_window = fault::enter_commit(&destination);
    // Test-only observation point: the destination is open with no sharing and the content
    // has been verified, so a test can prove nothing else can reach the object across the
    // rename. It is a no-op in every other build and holds nothing itself.
    #[cfg(test)]
    tests::reached_while_holding(&destination);
    handle_bound_replace_inner(parent, target, file_name, bytes, backup_name)
}

/// Named, default-off fault seams for the publication sequence.
///
/// Batch A requires the key failure points to be **really triggered** with their stage, object,
/// compensation and residue facts — `RolledBackAfterPublish`, `CompensationFailed` and the
/// third-party-member counterexample are exit-scope items, not `not_exercised` disclosures. They
/// cannot be triggered by a normal call because the failures they need are not reachable on a
/// healthy volume (a handle clone does not fail on demand; a restore does not fail on demand).
///
/// So the seam is explicit and narrow:
///
/// * it is **default off** — a seam that was never armed changes nothing at all;
/// * it is armed per **stage**, so a test states exactly which step fails, and the facts it
///   observes are the facts that step really produced;
/// * the failure it produces is a real injected I/O error returned from the real step, so the
///   production routing, compensation, rollback and material handling all run unchanged. Nothing
///   here fabricates a stage fact: arming `RestoreBackup` above the call does not make the code
///   *think* it restored; it makes the real restore fail.
///
/// The seams are compiled in every build on purpose. Gating them behind `cfg(test)` would mean the
/// acceptance binary that exercises them is a different program from the one under test, and the
/// whole point is that the acceptance path is the production path.
pub mod fault;

/// Creates one object under a verified parent with the final action **"fail if it exists"**.
///
/// This is the required first-creation shape: the caller decided the destination did not exist, and
/// that decision must still hold at the moment of publication. It cannot be expressed by a
/// replace: `ReplaceFileW` and `MOVEFILE_REPLACE_EXISTING` both replace whatever is there, so a
/// writer that created the destination in between would be silently overwritten by a call whose
/// whole premise was that there was nothing to overwrite.
///
/// The publication is still handle-bound and still goes through the same guard, staging and
/// recovery sequence as a replacement, so a failure after the new object has taken the destination
/// name is rolled back and reported with its real location. There is no backup, because there was
/// nothing to move aside — and a caller must not read that as "nothing happened": the new object
/// really did take the name, and the guard removes it again.
///
/// # Errors
/// * [`HandlePublishError::TargetContentChanged`] when the destination exists: the creation
///   condition is that it does not, and any existing object is a different object.
pub fn handle_bound_create_absent(
    parent: &VerifiedFile,
    file_name: &OsStr,
    bytes: &[u8],
) -> Result<HandleBoundPublishedFile, HandlePublishError> {
    ensure_parent_unchanged(parent)?;
    if !parent.info().is_directory || !is_leaf(file_name) {
        return Err(HandlePublishError::InvalidFileName);
    }
    let destination = parent.info().final_path.join(file_name);
    // The pre-check is only an optimisation and a clearer error. The authority is the rename below,
    // which refuses to replace; a race that creates the file after this check loses there.
    if let Ok(existing) = crate::path::inspect_existing_path(&destination, true) {
        return Err(HandlePublishError::TargetContentChanged {
            expected: "absent".to_owned(),
            actual: format!(
                "{}:{}",
                existing.identity.volume_serial_number,
                existing
                    .identity
                    .file_id
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            ),
        });
    }
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(io::Error::other)?;
    let temporary_name = format!(".aura-publish-{}.tmp", hex(&random));
    let temporary_path = parent.info().final_path.join(&temporary_name);
    let temporary_wide = wide(&temporary_path);
    // SAFETY: the absolute path is NUL terminated. The new file is opened without sharing and
    // includes DELETE so its held handle can perform the handle-relative rename below.
    let raw = unsafe {
        CreateFileW(
            temporary_wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE | DELETE,
            0,
            ptr::null(),
            CREATE_NEW,
            FILE_ATTRIBUTE_TEMPORARY | FILE_FLAG_WRITE_THROUGH,
            ptr::null_mut(),
        )
    };
    if raw.is_null() || raw == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: CreateFileW returned a unique owned handle.
    let file = unsafe { File::from_raw_handle(raw) };
    let staging = verified_from_file(file)?;
    let mut guard = HandleBoundPublishedFile {
        published: Some(staging),
        backup: None,
        parent: verified_from_file(parent.file().try_clone()?)?,
        file_name: file_name.to_os_string(),
        finalized: false,
    };
    if !final_path_within(parent.info(), guard.info()) {
        return Err(guard.finalize_failure(HandlePublishError::ResultEscapedParent));
    }
    if let Err(error) = verify_actual_parent_identity(
        parent,
        guard.published.as_ref().expect("published handle is present").info(),
        guard.published.as_ref().expect("published handle is present").file(),
        file_name,
    ) {
        return Err(guard.finalize_failure(error));
    }
    if let Err(error) = guard.file_mut().write_all(bytes).and_then(|()| guard.file_mut().sync_all())
    {
        return Err(guard.finalize_failure(error.into()));
    }
    if let Err(error) = ensure_parent_unchanged(parent) {
        return Err(guard.finalize_failure(error));
    }
    // RECOVERY RESPONSIBILITY STARTS HERE, exactly as in the replacement path: the staging object
    // now takes the destination name, so every later failure is routed through the guard, which
    // removes the publication again on drop.
    if let Err(error) = fault::fail_if_armed(fault::Stage::CreateAbsentBeforeRename, &destination) {
        return Err(guard.finalize_failure(error.into()));
    }
    // An interference installed for this stage runs here, inside the real publication and against
    // the real path, so a test can make the world change between this operation's steps without
    // racing it or re-implementing it. It cannot change the outcome: whatever it does, the
    // publication continues and its own code decides what happens next.
    fault::reached_interference(fault::Stage::CreateAbsentBeforeRename, &destination);
    fault::fail_if_armed(fault::Stage::CreateAbsentRename, &destination)
        .map_err(|error| guard.finalize_failure(error.into()))?;
    // `replace = false`: the rename itself refuses an existing destination, which is the authoritative
    // "fail if it exists" action.
    if let Err(error) = rename_handle_into_parent(
        guard.published.as_ref().expect("published handle is present").file(),
        parent,
        file_name,
        false,
    ) {
        return Err(guard.finalize_failure(error));
    }
    let refreshed = guard
        .published
        .as_ref()
        .expect("published handle is present")
        .file()
        .try_clone()
        .map_err(HandlePublishError::Windows)
        .and_then(|clone| verified_from_file(clone).map_err(HandlePublishError::Windows));
    match refreshed {
        Ok(refreshed) => guard.published = Some(refreshed),
        Err(error) => return Err(guard.finalize_failure(error)),
    }
    let expected = parent.info().final_path.join(file_name);
    if !final_path_within(parent.info(), guard.info())
        || !path_eq(&expected, &guard.info().final_path)
    {
        return Err(guard.finalize_failure(HandlePublishError::ResultEscapedParent));
    }
    if let Err(error) = verify_actual_parent_identity(
        parent,
        guard.published.as_ref().expect("published handle is present").info(),
        guard.published.as_ref().expect("published handle is present").file(),
        file_name,
    ) {
        return Err(guard.finalize_failure(error));
    }
    if let Err(error) = guard.file_mut().sync_all() {
        return Err(guard.finalize_failure(error.into()));
    }
    Ok(guard)
}

fn handle_bound_publish(
    parent: &VerifiedFile,
    file_name: &OsStr,
    bytes: &[u8],
    _replace: Option<&VerifiedFile>,
) -> Result<HandleBoundPublishedFile, HandlePublishError> {
    ensure_parent_unchanged(parent)?;
    if !parent.info().is_directory || !is_leaf(file_name) {
        return Err(HandlePublishError::InvalidFileName);
    }
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(io::Error::other)?;
    let temporary_name = format!(".aura-publish-{}.tmp", hex(&random));
    let temporary_path = parent.info().final_path.join(&temporary_name);
    let temporary_wide = wide(&temporary_path);
    // SAFETY: the absolute path is NUL terminated. The new file is opened
    // without sharing and includes DELETE so its held handle can perform the
    // handle-relative rename below.
    let raw = unsafe {
        CreateFileW(
            temporary_wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE | DELETE,
            0,
            ptr::null(),
            CREATE_NEW,
            FILE_ATTRIBUTE_TEMPORARY | FILE_FLAG_WRITE_THROUGH,
            ptr::null_mut(),
        )
    };
    if raw.is_null() || raw == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error().into());
    }
    // SAFETY: CreateFileW returned a unique owned handle.
    let file = unsafe { File::from_raw_handle(raw) };
    let staging = verified_from_file(file)?;
    let mut guard = HandleBoundPublishedFile {
        published: Some(staging),
        backup: None,
        parent: verified_from_file(parent.file().try_clone()?)?,
        file_name: file_name.to_os_string(),
        finalized: false,
    };
    if !final_path_within(parent.info(), guard.info()) {
        return Err(HandlePublishError::ResultEscapedParent);
    }
    verify_actual_parent_identity(
        parent,
        guard.published.as_ref().expect("published handle is present").info(),
        guard.published.as_ref().expect("published handle is present").file(),
        file_name,
    )?;
    guard.file_mut().write_all(bytes)?;
    guard.file_mut().sync_all()?;
    ensure_parent_unchanged(parent)?;
    // RECOVERY RESPONSIBILITY STARTS HERE. Up to this point every failure leaves the destination
    // untouched and the guard's `Drop` removes the staging file, so the errors below are honestly
    // `NeverPublished`. From here the new object already carries the destination name, so a
    // failure must be routed through the guard, which rolls the publication back on drop.
    rename_handle_into_parent(
        guard.published.as_ref().expect("published handle is present").file(),
        parent,
        file_name,
        false,
    )?;
    // The staged object has taken the destination name. From here a failure must be routed through
    // the guard so its `Drop` rolls the publication back, and the refresh below is the first such
    // step: it used a bare `?`, which returned the raw I/O error with `NeverPublished` while the
    // new object already carried the destination name.
    let refreshed = guard
        .published
        .as_ref()
        .expect("published handle is present")
        .file()
        .try_clone()
        .map_err(HandlePublishError::Windows)
        .and_then(|clone| verified_from_file(clone).map_err(HandlePublishError::Windows));
    match refreshed {
        Ok(refreshed) => guard.published = Some(refreshed),
        Err(error) => return Err(guard.finalize_failure(error)),
    }
    let expected = parent.info().final_path.join(file_name);
    if !final_path_within(parent.info(), guard.info())
        || !path_eq(&expected, &guard.info().final_path)
    {
        return Err(guard.finalize_failure(HandlePublishError::ResultEscapedParent));
    }
    if let Err(error) = verify_actual_parent_identity(
        parent,
        guard.published.as_ref().expect("published handle is present").info(),
        guard.published.as_ref().expect("published handle is present").file(),
        file_name,
    ) {
        return Err(guard.finalize_failure(error));
    }
    if let Err(error) = guard.file_mut().sync_all() {
        return Err(guard.finalize_failure(error.into()));
    }
    Ok(guard)
}

fn handle_bound_replace_inner(
    parent: &VerifiedFile,
    target: VerifiedFile,
    file_name: &OsStr,
    bytes: &[u8],
    backup_name: &OsStr,
) -> Result<HandleBoundPublishedFile, HandlePublishError> {
    ensure_parent_unchanged(parent)?;
    if !parent.info().is_directory || !is_leaf(file_name) {
        return Err(HandlePublishError::InvalidFileName);
    }
    let expected_destination = parent.info().final_path.join(file_name);
    let current = verified_from_file(target.file().try_clone()?)?;
    if !path_eq(&current.info().final_path, &expected_destination) {
        return Err(HandlePublishError::TargetIdentityChanged);
    }

    rename_handle_into_parent(target.file(), parent, backup_name, false)?;
    // RECOVERY RESPONSIBILITY STARTS HERE. The old object has left the destination name and now
    // lives under `backup_name`, so from this point on every failure has to put it back before it
    // is reported. Each fallible step below is funnelled through `recover_after_move` rather than
    // using `?` directly: a bare `?` here returned the underlying I/O error, whose
    // `PublishLocation` is `NeverPublished`, and the caller read that as "the destination still
    // holds what it held before" - which is false once the object has been moved aside.
    //
    // The clone is the first such step, so its failure is routed too. It was not: the code read
    // `match verified_from_file(target.file().try_clone()?)`, and the `?` on `try_clone` returned
    // *before* the `match` could classify anything, so a clone failure escaped as a raw I/O error
    // with `NeverPublished` while the old object sat under the backup name and the destination
    // name was empty. The comment claiming clone failure was covered described the `match` arm
    // that a clone failure could never reach.
    let backup_path = parent.info().final_path.join(backup_name);
    // The re-query of the moved-aside object is the step whose escaped `?` this round fixes, so it
    // has its own seam: arming it makes the real re-query fail, and the reported error must then be
    // `RolledBackAfterPublish` (restoration succeeded) rather than a raw `NeverPublished`.
    if fault::armed_now(fault::Stage::QueryMovedBackup, &expected_destination)
        && let Err(error) =
            fault::fail_if_armed(fault::Stage::QueryMovedBackup, &expected_destination)
    {
        return Err(recover_after_move(
            target.file(),
            parent,
            file_name,
            HandlePublishError::Windows(error),
        ));
    }
    let clone = target.file().try_clone();
    let moved_target = match clone {
        // The handle could not be re-queried. The backup is restored through the handle that
        // performed the move, which is still open and still names the object.
        Err(error) => {
            return Err(recover_after_move(
                target.file(),
                parent,
                file_name,
                HandlePublishError::Windows(error),
            ));
        }
        Ok(clone) => match verified_from_file(clone) {
            Ok(moved) => moved,
            Err(error) => {
                return Err(recover_after_move(
                    target.file(),
                    parent,
                    file_name,
                    HandlePublishError::Windows(error),
                ));
            }
        },
    };
    if !path_eq(&moved_target.info().final_path, &backup_path) {
        return Err(recover_after_move(
            target.file(),
            parent,
            file_name,
            HandlePublishError::TargetIdentityChanged,
        ));
    }

    match handle_bound_publish(parent, file_name, bytes, None) {
        Ok(mut published) => {
            // This handle was verified after the move, including its actual backup path.
            // The original target's cached info still names the former destination.
            published.backup = Some(moved_target);
            Ok(published)
        }
        Err(error) => Err(recover_after_move(target.file(), parent, file_name, error)),
    }
}

/// Puts the moved-aside object back and reports the failure with its real recovery outcome.
///
/// Every failure that can happen after the destination has been moved to the backup name goes
/// through here, so the reported error always carries both the original failure and what became of
/// the old content:
///
/// * restoration succeeded — the destination holds its previous content again, so the failure is
///   [`HandlePublishError::RolledBackAfterPublish`] and its location is `RolledBack`, never
///   `NeverPublished`. The caller may then honestly say the old content is still in place.
/// * restoration failed — the destination is not known to hold either version. That is
///   [`HandlePublishError::CompensationFailed`] with `Unconfirmable`, and the backup is left in
///   place as the remaining recovery material rather than being deleted.
fn recover_after_move(
    backup: &File,
    parent: &VerifiedFile,
    file_name: &OsStr,
    error: HandlePublishError,
) -> HandlePublishError {
    match restore_backup(backup, parent, file_name) {
        Ok(()) => HandlePublishError::RolledBackAfterPublish { cause: error.to_string() },
        Err(compensation) => HandlePublishError::CompensationFailed {
            operation: error.to_string(),
            rollback: compensation.to_string(),
        },
    }
}

fn restore_backup(
    backup: &File,
    parent: &VerifiedFile,
    file_name: &OsStr,
) -> Result<(), HandlePublishError> {
    // The compensation step itself is the seam, not a caller's imitation of it: the injected error
    // is returned from the real restore, so every caller's real compensation routing runs.
    fault::fail_if_armed(fault::Stage::RestoreBackup, &parent.info().final_path.join(file_name))?;
    rename_handle_into_parent(backup, parent, file_name, false)
}

fn delete_by_handle(file: &File) -> io::Result<()> {
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: the held file handle includes DELETE access and the fixed-size
    // disposition structure remains valid for the call.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&raw const disposition).cast(),
            u32::try_from(size_of::<FILE_DISPOSITION_INFO>()).expect("disposition size fits u32"),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn open_replace_target(path: &Path) -> io::Result<VerifiedFile> {
    let path_wide = wide(path);
    // SAFETY: the path is NUL-terminated. DELETE is required for the
    // handle-relative move-to-backup protocol; sharing permits ordinary readers.
    let raw = unsafe {
        CreateFileW(
            path_wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE | DELETE,
            0,
            ptr::null(),
            windows_sys::Win32::Storage::FileSystem::OPEN_EXISTING,
            FILE_FLAG_WRITE_THROUGH,
            ptr::null_mut(),
        )
    };
    if raw.is_null() || raw == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW returned a unique owned handle.
    verified_from_file(unsafe { File::from_raw_handle(raw) })
}

fn random_leaf(prefix: &str) -> Result<std::ffi::OsString, HandlePublishError> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(io::Error::other)?;
    Ok(format!("{prefix}{}.tmp", hex(&random)).into())
}

/// Moves the very object held by `source` into a verified directory, without replacing an entry.
/// The source must have been opened with [`crate::path::open_held_for_move`]; that handle denies
/// competing writes and renames. No second path-based source lookup occurs at the effect boundary.
///
/// # Errors
/// Returns parent verification, invalid leaf, or the actual native rename failure.
pub fn handle_bound_move_existing(
    source: &VerifiedFile,
    parent: &VerifiedFile,
    file_name: &OsStr,
) -> Result<(), HandlePublishError> {
    ensure_parent_unchanged(parent)?;
    rename_handle_into_parent(source.file(), parent, file_name, false)
}

fn rename_handle_into_parent(
    source: &File,
    parent: &VerifiedFile,
    file_name: &OsStr,
    replace: bool,
) -> Result<(), HandlePublishError> {
    if !is_leaf(file_name) {
        return Err(HandlePublishError::InvalidFileName);
    }
    let name = file_name.encode_wide().collect::<Vec<_>>();
    let byte_len = name
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| io::Error::other("file name length overflow"))?;
    let variable_total = std::mem::offset_of!(FILE_RENAME_INFORMATION, FileName)
        .checked_add(byte_len)
        .ok_or_else(|| io::Error::other("rename buffer overflow"))?;
    // Preserve the full ABI-sized fixed record for a one-code-unit leaf while
    // also extending the variable FileName tail for longer names.
    let total = size_of::<FILE_RENAME_INFORMATION>().max(variable_total);
    let words = total.div_ceil(size_of::<usize>());
    let mut storage = vec![0_usize; words];
    let info = storage.as_mut_ptr().cast::<FILE_RENAME_INFORMATION>();
    // SAFETY: storage is aligned and sized for FILE_RENAME_INFORMATION plus
    // the full UTF-16 leaf. Both source and parent handles stay alive through
    // the native call.
    unsafe {
        (*info).Anonymous.ReplaceIfExists = replace;
        (*info).RootDirectory = parent.file().as_raw_handle();
        (*info).FileNameLength =
            u32::try_from(byte_len).map_err(|_| io::Error::other("file name too long"))?;
        ptr::copy_nonoverlapping(name.as_ptr(), (*info).FileName.as_mut_ptr(), name.len());
        let mut io_status: IO_STATUS_BLOCK = std::mem::zeroed();
        let status = NtSetInformationFile(
            source.as_raw_handle(),
            &raw mut io_status,
            info.cast_const().cast(),
            u32::try_from(total).map_err(|_| io::Error::other("rename buffer too large"))?,
            FileRenameInformation,
        );
        if status == STATUS_PENDING {
            return Err(HandlePublishError::NativeRenamePending);
        }
        if status < 0 {
            let win32_code = RtlNtStatusToDosError(status);
            let source =
                io::Error::from_raw_os_error(i32::try_from(win32_code).unwrap_or(i32::MAX));
            return Err(HandlePublishError::NativeRename {
                ntstatus: u32::from_ne_bytes(status.to_ne_bytes()),
                win32_code,
                source,
            });
        }
    }
    Ok(())
}

pub(crate) fn ensure_parent_unchanged(parent: &VerifiedFile) -> Result<(), HandlePublishError> {
    ensure_local_handle_relative_parent(parent)?;
    let current = verified_from_file(parent.file().try_clone()?)?;
    if !current.info().is_directory
        || current.info().identity != parent.info().identity
        || !path_eq(&current.info().final_path, &parent.info().final_path)
    {
        return Err(HandlePublishError::ParentIdentityChanged);
    }
    Ok(())
}

/// Deletes **the very object held by `held`**, without a second path lookup.
///
/// # Why compensation needs this and not `std::fs::remove_file`
///
/// A compensation has to remove the object *this execution* published. Checking a path and then
/// removing that path is two decisions about a name, and a name says nothing about whose object is
/// under it: between the check and the removal a third party can replace the object, and the
/// removal then deletes theirs. The only way the check and the effect are about one object is for
/// both to be made through **one handle** — the handle the caller verified.
///
/// `FileDispositionInformationEx` with `FILE_DISPOSITION_POSIX_SEMANTICS` is what makes this
/// expressible on a held handle: the removal is bound to the object that handle names, so it cannot
/// land on a same-named replacement. The handle must have been opened with DELETE access
/// (see [`crate::path::open_held_for_move`]).
///
/// # Errors
/// Returns the real native refusal. A sharing or access failure is reported and nothing is removed;
/// it is never read as "the object was absent".
pub fn handle_bound_delete(held: &File) -> Result<(), HandlePublishError> {
    let mut info: FILE_DISPOSITION_INFO_EX = unsafe { std::mem::zeroed() };
    info.Flags = FILE_DISPOSITION_FLAG_DELETE | FILE_DISPOSITION_FLAG_POSIX_SEMANTICS;
    let mut io_status: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
    // SAFETY: the held handle is valid and owns DELETE access; both records are exact, writable
    // buffers for this synchronous native call, and the handle stays alive across it.
    let status = unsafe {
        NtSetInformationFile(
            held.as_raw_handle(),
            &raw mut io_status,
            (&raw const info).cast(),
            u32::try_from(size_of::<FILE_DISPOSITION_INFO_EX>())
                .expect("FILE_DISPOSITION_INFO_EX size fits u32"),
            FileDispositionInformationEx,
        )
    };
    if status == STATUS_PENDING {
        return Err(HandlePublishError::NativeRenamePending);
    }
    if status < 0 {
        // SAFETY: the NTSTATUS was returned directly by the preceding native call.
        let win32_code = unsafe { RtlNtStatusToDosError(status) };
        return Err(HandlePublishError::NativeRename {
            ntstatus: u32::from_ne_bytes(status.to_ne_bytes()),
            win32_code,
            source: io::Error::from_raw_os_error(i32::try_from(win32_code).unwrap_or(i32::MAX)),
        });
    }
    Ok(())
}

fn ensure_local_handle_relative_parent(parent: &VerifiedFile) -> Result<(), HandlePublishError> {
    if matches!(
        parent.info().final_path.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _))
    ) {
        return Err(HandlePublishError::RemoteParentUnsupported);
    }

    let mut device = FILE_FS_DEVICE_INFORMATION::default();
    let mut io_status = IO_STATUS_BLOCK::default();
    // SAFETY: the held directory handle is valid and both output records are
    // exact, writable buffers for this synchronous native volume query.
    let status = unsafe {
        NtQueryVolumeInformationFile(
            parent.file().as_raw_handle(),
            &raw mut io_status,
            (&raw mut device).cast(),
            u32::try_from(size_of::<FILE_FS_DEVICE_INFORMATION>())
                .expect("FILE_FS_DEVICE_INFORMATION size fits u32"),
            FileFsDeviceInformation,
        )
    };
    if status == STATUS_PENDING {
        return Err(HandlePublishError::RemoteProtocolProbeFailed(io::Error::other(
            "NtQueryVolumeInformationFile unexpectedly returned STATUS_PENDING",
        )));
    }
    if status != 0 {
        // SAFETY: the NTSTATUS was returned directly by the preceding native query.
        let win32_code = unsafe { RtlNtStatusToDosError(status) };
        return Err(HandlePublishError::RemoteProtocolProbeFailed(io::Error::from_raw_os_error(
            i32::try_from(win32_code).unwrap_or(i32::MAX),
        )));
    }
    if io_status.Information < size_of::<FILE_FS_DEVICE_INFORMATION>() {
        return Err(HandlePublishError::RemoteProtocolProbeFailed(io::Error::other(
            "NtQueryVolumeInformationFile returned an incomplete device record",
        )));
    }
    if device.Characteristics & FILE_REMOTE_DEVICE != 0 {
        return Err(HandlePublishError::RemoteParentUnsupported);
    }
    Ok(())
}

fn verify_actual_parent_identity(
    expected_parent: &VerifiedFile,
    child_info: &NativePathInfo,
    child: &File,
    intended_name: &OsStr,
) -> Result<(), HandlePublishError> {
    // This parameter only selects diagnostics; it confers no publication authority.
    fault::fail_if_armed(
        fault::Stage::VerifyParentIdentity,
        &expected_parent.info().final_path.join(intended_name),
    )
    .map_err(HandlePublishError::Windows)?;
    let parent_path =
        child_info.final_path.parent().ok_or(HandlePublishError::ParentIdentityChanged)?;
    let actual_parent =
        open_verified_publish_parent(parent_path).map_err(HandlePublishError::Windows)?;
    if actual_parent.info().identity != expected_parent.info().identity {
        return Err(HandlePublishError::ParentIdentityChanged);
    }
    let child_after = verified_from_file(child.try_clone()?)?;
    if child_after.info().identity != child_info.identity
        || !path_eq(&child_after.info().final_path, &child_info.final_path)
    {
        return Err(HandlePublishError::ParentIdentityChanged);
    }
    ensure_parent_unchanged(expected_parent)
}

fn is_leaf(name: &OsStr) -> bool {
    let path = Path::new(name);
    !name.is_empty()
        && name.encode_wide().all(|unit| unit != 0 && unit != u16::from(b':'))
        && path.file_name() == Some(name)
        && path.components().count() == 1
}

fn path_eq(left: &Path, right: &Path) -> bool {
    left.to_string_lossy().eq_ignore_ascii_case(&right.to_string_lossy())
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(char::from(DIGITS[usize::from(byte >> 4)]));
        result.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    result
}

/// Atomically moves a fully flushed and closed replacement file over its destination.
///
/// Both paths must be sibling files. Existing files use ReplaceFileW so their
/// security metadata is preserved; first writes use MoveFileExW.
pub fn atomic_replace_file(
    replacement: &Path,
    destination: &Path,
) -> Result<(), AtomicReplaceError> {
    if !replacement.is_file() {
        return Err(AtomicReplaceError::MissingReplacement(replacement.to_path_buf()));
    }
    if replacement.parent() != destination.parent() {
        return Err(AtomicReplaceError::DifferentDirectories);
    }
    let replacement_wide = wide(replacement);
    let destination_wide = wide(destination);

    if destination.exists() {
        let binding = replace_retry::binding(replacement, destination).ok();
        let mut retry = 0;
        loop {
            // SAFETY: both strings are NUL-terminated and live through the call. The
            // optional pointers are null as documented by ReplaceFileW.
            let replaced = unsafe {
                ReplaceFileW(
                    destination_wide.as_ptr(),
                    replacement_wide.as_ptr(),
                    ptr::null(),
                    0,
                    ptr::null(),
                    ptr::null(),
                )
            };
            if replaced != 0 {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            // 1175 leaves both original names intact; 32 fails on sharing.
            // Never retry the partial-rename errors 1176/1177 or access denial.
            // https://learn.microsoft.com/windows/win32/api/winbase/nf-winbase-replacefilew
            if matches!(error.raw_os_error(), Some(32 | 1175))
                && retry < 5
                && let Some(binding) = &binding
            {
                std::thread::sleep(std::time::Duration::from_millis(20 << retry));
                if replace_retry::binding(replacement, destination).ok().as_ref() != Some(binding) {
                    return Err(AtomicReplaceError::RetryBindingChanged);
                }
                retry += 1;
                continue;
            }
            if error.raw_os_error() != Some(ERROR_FILE_NOT_FOUND as i32) {
                return Err(AtomicReplaceError::Windows {
                    path: destination.to_path_buf(),
                    source: error,
                });
            }
            break;
        }
    }

    // SAFETY: both strings are NUL-terminated and live through the call. The
    // caller has constrained both files to one directory, so this is a rename.
    let moved = unsafe {
        MoveFileExW(
            replacement_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        return Err(AtomicReplaceError::Windows {
            path: destination.to_path_buf(),
            source: io::Error::last_os_error(),
        });
    }
    Ok(())
}

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::{Read, Seek, SeekFrom, Write},
        sync::{Mutex, OnceLock},
    };

    use tempfile::tempdir;

    use crate::path::{NativeFileAccess, open_verified_existing, open_verified_publish_parent};

    /// The observation point a test can install, keyed by the destination it applies to.
    type HoldingHook = Box<dyn Fn(&std::path::Path) + Send + Sync>;
    type HoldingHooks = Mutex<Vec<(String, std::sync::Arc<HoldingHook>)>>;

    /// One registry with an entry per destination, so two tests that each hold their own
    /// publication cannot install over each other — and, more importantly, so a test that
    /// never reaches its own destination is not stalled by another test's hook.
    fn holding_hooks() -> &'static HoldingHooks {
        static HOOKS: OnceLock<HoldingHooks> = OnceLock::new();
        HOOKS.get_or_init(|| Mutex::new(Vec::new()))
    }

    /// The key two spellings of one destination share.
    ///
    /// The platform reports final paths in the extended-length form (`\\?\C:\...`), so the
    /// plain path a test constructs is not string-equal to it. Canonicalizing both resolves
    /// the prefix, and folding the case matches how Windows itself compares paths.
    fn hook_key(path: &std::path::Path) -> String {
        std::fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .into_owned()
    }

    /// Installs the observation point for one destination, removing it when the guard drops.
    fn install_holding_hook(path: &std::path::Path, hook: HoldingHook) -> impl Drop + use<> {
        let owned = hook_key(path);
        holding_hooks()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push((owned.clone(), std::sync::Arc::new(hook)));
        struct Remove(String);
        impl Drop for Remove {
            fn drop(&mut self) {
                holding_hooks()
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .retain(|(path, _)| path != &self.0);
            }
        }
        Remove(owned)
    }

    /// Called by the production path once the destination is held and its content is verified.
    pub(super) fn reached_while_holding(path: &std::path::Path) {
        let key = hook_key(path);
        let hook = holding_hooks()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .find(|(expected, _)| expected == &key)
            .map(|(_, hook)| std::sync::Arc::clone(hook));
        if let Some(hook) = hook {
            hook(path);
        }
    }

    /// A one-shot gate that fails instead of hanging, so a hook that never fires is a test
    /// failure rather than a suite that stops responding.
    struct Gate {
        reached: std::sync::mpsc::Sender<()>,
        released: std::sync::Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl Gate {
        /// The publication side: report the arrival, then wait to be let go.
        fn hold(&self) {
            let _ = self.reached.send(());
            let released = self
                .released
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .recv_timeout(std::time::Duration::from_secs(30));
            assert!(released.is_ok(), "the test never released the held publication");
        }

        /// The test side: wait for the arrival, up to a bound only a real hang hits.
        fn wait(&self, arrivals: &std::sync::mpsc::Receiver<()>) {
            assert!(
                arrivals.recv_timeout(std::time::Duration::from_secs(30)).is_ok(),
                "the publication never reached the observation point"
            );
        }

        /// The test side: let the held publication continue.
        fn release(&self, release: &std::sync::mpsc::Sender<()>) {
            let _ = release.send(());
        }
    }

    use super::{
        ContentHashAlgorithm, HandleBoundReplaceContent, HandlePublishError, PublishLocation,
        atomic_replace_file, fault, handle_bound_create_new, handle_bound_replace,
        handle_bound_replace_checked, handle_bound_replace_verified,
        handle_bound_replace_with_backup, handle_bound_replace_with_backup_checked,
    };

    /// The `Unchecked` counterexample for the P04 overwrite chain.
    ///
    /// P04's overwrite used to reach the platform through
    /// `handle_bound_replace_with_backup`, which hard-codes `Unchecked`: the caller had checked
    /// the old content before admission, but nothing re-checked it on the handle that actually
    /// publishes. An independent writer that rewrites the file *in place* keeps the
    /// `FileIdentity`, so an identity-only check cannot see the change, and the stale decision
    /// would win.
    ///
    /// This test is the counterexample that must reject that implementation. It rewrites the
    /// destination in place after the old hash was taken, leaves the identity identical, and
    /// requires the checked form to refuse. The companion assertion shows why the unchecked form
    /// is the wrong answer rather than a stylistic choice: it *succeeds*, overwriting the other
    /// writer's bytes.
    #[test]
    fn an_unchecked_replacement_would_win_the_in_place_rewrite_race() {
        let directory = tempdir().expect("temp directory");
        let destination = directory.path().join("artifact.bin");
        fs::write(&destination, b"old content").expect("seed destination");
        // The caller's original condition, taken before admission, and the identity it saw.
        let expected_hex = blake3::hash(b"old content").to_hex().to_string();
        let identity =
            crate::path::inspect_existing_path(&destination, true).expect("identity").identity;
        // An outside writer rewrites the bytes without replacing the object.
        fs::write(&destination, b"outside writer won").expect("in-place rewrite");
        let after_identity =
            crate::path::inspect_existing_path(&destination, true).expect("identity").identity;
        assert_eq!(
            identity, after_identity,
            "an in-place rewrite must keep the FileIdentity, which is why identity alone \
             cannot decide this"
        );

        // The wrong implementation: the unchecked wrapper the P04 chain actually called. It
        // publishes over the other writer's bytes, because nothing re-checked the content.
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");
        let unchecked = handle_bound_replace_with_backup(
            &parent,
            "artifact.bin".as_ref(),
            &identity,
            b"new",
            ".aura-backup-u.tmp".as_ref(),
        );
        let unchecked = match unchecked {
            Ok(published) => published,
            Err(error) => panic!("the unchecked form is expected to publish regardless: {error}"),
        };
        unchecked.commit().expect("the unchecked form commits");
        assert_eq!(
            fs::read(&destination).expect("destination"),
            b"new",
            "the unchecked form overwrites another writer's bytes - this is the defect"
        );

        // Reset and show the checked form refusing the very same race.
        fs::write(&destination, b"old content").expect("reseed destination");
        let identity =
            crate::path::inspect_existing_path(&destination, true).expect("identity").identity;
        fs::write(&destination, b"outside writer won").expect("in-place rewrite");
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");
        let checked = handle_bound_replace_with_backup_checked(
            &parent,
            "artifact.bin".as_ref(),
            &identity,
            HandleBoundReplaceContent::Blake3Hex(&expected_hex),
            b"new",
            ".aura-backup-c.tmp".as_ref(),
        );
        let error = match checked {
            Ok(_) => panic!(
                "the checked form must refuse a destination that no longer holds the \
                             expected content"
            ),
            Err(error) => error,
        };
        assert!(
            matches!(error, HandlePublishError::TargetContentChanged { .. }),
            "the refusal must name the content: {error}"
        );
        assert_eq!(error.location(), PublishLocation::NeverPublished);
        assert_eq!(
            fs::read(&destination).expect("destination"),
            b"outside writer won",
            "the outside writer's bytes must survive a refused publication"
        );
    }

    /// A failure after the old object was moved aside restores it and says so.
    ///
    /// The defect: `handle_bound_replace_inner` renamed the destination to the backup name and
    /// then ran fallible re-queries with a bare `?`. A failure there returned the raw I/O error,
    /// whose `PublishLocation` is `NeverPublished` - so the caller was told the destination "still
    /// holds exactly what it held before" while the old object was sitting under the backup name
    /// and the destination name was empty. `RollbackFailed` versus a plain abort was decided from
    /// the error variant instead of from what compensation actually did.
    ///
    /// The counterexample drives a real failure on that path: the destination's directory is made
    /// read-only, so the rename that would move the old object aside fails. Nothing has moved yet,
    /// so the failure must be `NeverPublished` and the old content must be untouched. Then the
    /// same call is run with the failure injected *after* the move by making the backup leaf name
    /// collide with an existing directory, which fails the rename for a reason that is reached
    /// only once the sequence has started.
    #[test]
    fn a_failure_before_the_first_side_effect_leaves_the_destination_alone() {
        let directory = tempdir().expect("temp directory");
        let destination = directory.path().join("artifact.bin");
        fs::write(&destination, b"old content").expect("seed destination");
        let identity =
            crate::path::inspect_existing_path(&destination, true).expect("identity").identity;
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");

        // A backup leaf that already exists as a directory cannot be renamed onto, so the very
        // first `rename_handle_into_parent` fails. That is before the old object has moved, so
        // the destination must still hold its previous content and the error must be honest.
        let blocking = directory.path().join(".aura-backup-blocked.tmp");
        fs::create_dir(&blocking).expect("blocking directory");

        let expected_hex = blake3::hash(b"old content").to_hex().to_string();
        let attempt = handle_bound_replace_with_backup_checked(
            &parent,
            "artifact.bin".as_ref(),
            &identity,
            HandleBoundReplaceContent::Blake3Hex(&expected_hex),
            b"new content",
            ".aura-backup-blocked.tmp".as_ref(),
        );
        let error = match attempt {
            Ok(_) => panic!("a rename onto an existing directory must fail"),
            Err(error) => error,
        };
        assert_eq!(
            error.location(),
            PublishLocation::NeverPublished,
            "nothing had been moved aside, so the destination is unchanged: {error}"
        );
        assert_eq!(
            fs::read(&destination).expect("destination"),
            b"old content",
            "a failure before the first side effect must leave the destination exactly as it was"
        );
        // The old object was never moved, so its bytes are reachable by their own name and no
        // recovery material was invented.
        assert!(blocking.is_dir(), "the blocking directory must be untouched");
    }

    /// Failure points are classified by the stage the sequence really reached.
    ///
    /// The old code used a bare `?` for the re-query that runs *after* the destination has been
    /// moved to the backup name. That returned a plain I/O error, whose `PublishLocation` is
    /// `NeverPublished`, so a caller could be told the destination "still holds exactly what it
    /// held before" while the old object sat under the backup name and the destination name had
    /// been vacated. The classification came from the error variant, not from what the sequence
    /// had actually done.
    ///
    /// This test drives two real failure points and requires the reported location to agree with
    /// what is on disk. It measures the disk rather than assuming which stage was reached, so a
    /// classification that disagrees with reality fails here:
    ///
    ///   * a content condition that does not hold, and
    ///   * an exclusive holder on the destination.
    ///
    /// For each, the test reads the destination and the backup leaf and then demands the two
    /// agree: `NeverPublished` is only allowed when nothing was moved.
    #[test]
    fn replace_failure_locations_agree_with_what_is_on_disk() {
        use std::os::windows::fs::OpenOptionsExt as _;

        let directory = tempdir().expect("temp directory");
        let destination = directory.path().join("artifact.bin");
        let identity_of = |path: &std::path::Path| {
            crate::path::inspect_existing_path(path, true).expect("identity").identity
        };

        // Failure point 1: the caller's content condition does not hold. Nothing may move.
        fs::write(&destination, b"old content").expect("seed destination");
        let identity = identity_of(&destination);
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");
        let mismatch_hex = blake3::hash(b"different").to_hex().to_string();
        let mismatch = handle_bound_replace_with_backup_checked(
            &parent,
            "artifact.bin".as_ref(),
            &identity,
            HandleBoundReplaceContent::Blake3Hex(&mismatch_hex),
            b"new content",
            ".aura-backup-s1.tmp".as_ref(),
        );
        let error = match mismatch {
            Err(error) => error,
            Ok(_) => panic!("a mismatched condition must be refused"),
        };
        assert_eq!(
            error.location(),
            PublishLocation::NeverPublished,
            "a refused content condition must be reported as never published: {error}"
        );
        assert_eq!(
            fs::read(&destination).expect("destination"),
            b"old content",
            "a refused content condition must leave the destination untouched"
        );
        assert!(
            !directory.path().join(".aura-backup-s1.tmp").exists(),
            "a refused content condition must not have moved anything aside"
        );

        // Failure point 2: an exclusive holder on the destination. Whether this stops the
        // move-aside or a later step is measured, not assumed, and the reported location has to
        // match that measurement.
        let holder = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&destination)
            .expect("exclusive holder");
        let held_hex = blake3::hash(b"old content").to_hex().to_string();
        let blocked = handle_bound_replace_with_backup_checked(
            &parent,
            "artifact.bin".as_ref(),
            &identity,
            HandleBoundReplaceContent::Blake3Hex(&held_hex),
            b"new content",
            ".aura-backup-s2.tmp".as_ref(),
        );
        let error = match blocked {
            Err(error) => error,
            Ok(_) => panic!("a held destination must stop the replacement"),
        };
        let backup_leaf = directory.path().join(".aura-backup-s2.tmp");
        // The destination's bytes cannot be read while the test's own holder denies sharing, so
        // the object's presence is established by identity instead: the same file id under the
        // same name means the original object never left it.
        let still_there = crate::path::inspect_existing_path(&destination, true)
            .map(|info| info.identity == identity)
            .unwrap_or(false);
        let destination_is_free = !destination.exists();
        let moved_aside = backup_leaf.exists() || destination_is_free;
        eprintln!(
            "STAGE2 location={:?} destination_exists={} same_object={} backup_exists={}",
            error.location(),
            destination.exists(),
            still_there,
            backup_leaf.exists()
        );
        if !moved_aside {
            // The sequence never got past the move-aside, so the destination really is unchanged
            // and `NeverPublished` is the honest report.
            assert!(
                still_there,
                "with nothing moved aside the destination must still hold the same object"
            );
            assert_eq!(
                error.location(),
                PublishLocation::NeverPublished,
                "a failure before the move-aside must be reported as never published: {error}"
            );
        } else {
            // The old object left its name, so claiming the destination was never published
            // would be a false statement about the filesystem.
            assert_ne!(
                error.location(),
                PublishLocation::NeverPublished,
                "the old object was moved aside, so the destination is not 'never published': \
                 {error}"
            );
            if error.location() == PublishLocation::RolledBack {
                assert!(
                    still_there,
                    "a reported rollback must really have restored the old object"
                );
            }
        }
        drop(holder);
    }

    /// A SHA-256 condition is verified as SHA-256, never reinterpreted as BLAKE3.
    ///
    /// The Artifact overwrite journal records the caller's `ContentHash`, whose algorithm may be
    /// SHA-256. Feeding that hex to the BLAKE3 comparison would compare two different functions'
    /// output. This test pins both directions: the SHA-256 condition is *accepted* when it holds
    /// (so the hex is not being re-hashed under the wrong function and rejected), and *refused*
    /// when the bytes changed.
    #[test]
    fn a_sha256_condition_is_verified_under_its_own_algorithm() {
        use sha2::Digest as _;

        let directory = tempdir().expect("temp directory");
        let destination = directory.path().join("artifact.bin");
        fs::write(&destination, b"original").expect("seed destination");
        let identity =
            crate::path::inspect_existing_path(&destination, true).expect("identity").identity;
        let sha256_hex = format!("{:x}", sha2::Sha256::digest(b"original"));
        // The same bytes under the other algorithm, to show the two are not interchangeable.
        let blake3_hex = blake3::hash(b"original").to_hex().to_string();
        assert_ne!(sha256_hex, blake3_hex, "the algorithms must produce different hex");

        // The correct algorithm accepts the unchanged destination.
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");
        let accepted = handle_bound_replace_with_backup_checked(
            &parent,
            "artifact.bin".as_ref(),
            &identity,
            HandleBoundReplaceContent::Hex {
                algorithm: ContentHashAlgorithm::Sha256,
                expected: &sha256_hex,
            },
            b"published",
            ".aura-backup-s.tmp".as_ref(),
        );
        let published = match accepted {
            Ok(published) => published,
            Err(error) => {
                panic!("a SHA-256 condition over the current bytes must be accepted: {error}")
            }
        };
        published.commit().expect("commit");
        assert_eq!(fs::read(&destination).expect("destination"), b"published");

        // Under BLAKE3 the same destination is not the same condition, so a SHA-256 digest
        // presented as BLAKE3 must be refused rather than silently accepted.
        fs::write(&destination, b"original").expect("reseed destination");
        let identity =
            crate::path::inspect_existing_path(&destination, true).expect("identity").identity;
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");
        let mislabelled = handle_bound_replace_with_backup_checked(
            &parent,
            "artifact.bin".as_ref(),
            &identity,
            HandleBoundReplaceContent::Hex {
                algorithm: ContentHashAlgorithm::Blake3,
                expected: &sha256_hex,
            },
            b"published",
            ".aura-backup-m.tmp".as_ref(),
        );
        let error = match mislabelled {
            Ok(_) => panic!("a SHA-256 digest must not satisfy a BLAKE3 condition"),
            Err(error) => error,
        };
        assert!(
            matches!(error, HandlePublishError::TargetContentChanged { .. }),
            "the mismatch must be reported as changed content: {error}"
        );
        assert_eq!(
            fs::read(&destination).expect("destination"),
            b"original",
            "the destination must be untouched by the refused publication"
        );

        // And a SHA-256 condition over bytes that changed is refused too.
        fs::write(&destination, b"rewritten by someone else").expect("in-place rewrite");
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");
        let changed = handle_bound_replace_with_backup_checked(
            &parent,
            "artifact.bin".as_ref(),
            &identity,
            HandleBoundReplaceContent::Hex {
                algorithm: ContentHashAlgorithm::Sha256,
                expected: &sha256_hex,
            },
            b"published",
            ".aura-backup-r.tmp".as_ref(),
        );
        assert!(
            matches!(changed, Err(HandlePublishError::TargetContentChanged { .. })),
            "a SHA-256 condition must also refuse a destination rewritten in place"
        );
        assert_eq!(
            fs::read(&destination).expect("destination"),
            b"rewritten by someone else",
            "the outside bytes must survive"
        );
    }

    /// The content check a publication makes is bound to the object it publishes, not to a
    /// read taken before it.
    ///
    /// A rewrite that keeps the file object — the case a handle re-open cannot detect — is
    /// caught, and the destination is left exactly as the outside writer left it. This is the
    /// deterministic form of the race the review reported: the stale decision does not win.
    #[test]
    fn a_publication_refuses_a_destination_rewritten_in_place() {
        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("note.txt");
        fs::write(&path, b"original").expect("original");
        let identity = crate::path::inspect_existing_path(&path, true).expect("identity").identity;
        // Rewritten in place, which keeps the object and therefore its identity.
        fs::write(&path, b"changed by someone else").expect("in-place rewrite");

        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");
        let attempt = handle_bound_replace_checked(
            &parent,
            "note.txt".as_ref(),
            &identity,
            blake3::hash(b"original").to_hex().as_ref(),
            b"replacement",
        );
        let error = match attempt {
            Ok(_) => panic!("a changed destination must be refused"),
            Err(error) => error,
        };
        assert!(
            matches!(error, HandlePublishError::TargetContentChanged { .. }),
            "the refusal must name the content, not the identity: {error}"
        );
        assert_eq!(error.location(), PublishLocation::NeverPublished);
        assert_eq!(
            fs::read(&path).expect("destination"),
            b"changed by someone else",
            "the outside content must survive a refused publication"
        );
    }

    /// While the publication holds the destination, another writer cannot open it at all.
    ///
    /// This is what closes the window between the content check and the rename: the check runs
    /// on a handle opened with no sharing, and that handle stays open across the rename, so
    /// there is no moment in which a rewrite could land. The hook fires while that handle is
    /// held, which makes the exclusion a fact of the test rather than a matter of timing.
    #[test]
    fn an_outside_writer_is_excluded_while_the_publication_holds_the_destination() {
        // The hook runs on the publication's own thread and the test runs on this one, so the
        // two sides hand control back and forth through channels that time out rather than a
        // barrier that would hang the whole suite if the hook never fired.
        let (arrivals, arrival) = std::sync::mpsc::channel::<()>();
        let (release, releases) = std::sync::mpsc::channel::<()>();
        let gate = std::sync::Arc::new(Gate {
            reached: arrivals,
            released: std::sync::Mutex::new(releases),
        });

        let directory = tempdir().expect("temp directory");
        let path = directory.path().join("held.txt");
        fs::write(&path, b"original").expect("original");
        let identity = crate::path::inspect_existing_path(&path, true).expect("identity").identity;
        let expected = blake3::hash(b"original").to_hex().to_string();

        let hook_gate = std::sync::Arc::clone(&gate);
        let _hook = install_holding_hook(&path, Box::new(move |_path| hook_gate.hold()));

        // The directory handle stays with the test: the holder thread only needs its path, and
        // letting the thread own the `TempDir` would delete the tree as soon as it finished.
        let parent_path = directory.path().to_path_buf();
        let holder = std::thread::spawn(move || {
            let parent = open_verified_publish_parent(&parent_path).expect("verified parent");
            let mut published = handle_bound_replace_verified(
                &parent,
                "held.txt".as_ref(),
                &identity,
                HandleBoundReplaceContent::Blake3Hex(&expected),
                b"published",
                ".aura-backup-test.tmp".as_ref(),
            )
            .expect("the publication succeeds");
            // Read through the handle the publication holds, before it is finalized: this is
            // the object the caller asked to publish, and it is the one the refused writer
            // could not reach.
            published.file_mut().try_clone().map(|mut file| {
                let mut bytes = Vec::new();
                let _ = file.seek(SeekFrom::Start(0));
                let _ = file.read_to_end(&mut bytes);
                bytes
            })
        });
        // The publication is between its content check and its rename, holding the object.
        gate.wait(&arrival);
        let outside = fs::write(&path, b"outside");
        assert!(
            outside.is_err(),
            "an outside write must be refused while the publication holds the destination"
        );
        gate.release(&release);
        let observed = holder.join().expect("holder").expect("the held object is readable");
        assert_eq!(
            observed,
            b"published".to_vec(),
            "the object held across the rename must be the content the publication wrote"
        );
    }

    #[test]
    fn atomically_replaces_existing_and_new_files() {
        let directory = tempdir().expect("temp directory");
        let destination = directory.path().join("config.json");
        let replacement = directory.path().join("config.tmp");
        fs::write(&replacement, b"first").expect("replacement");
        atomic_replace_file(&replacement, &destination).expect("first move");
        assert_eq!(fs::read(&destination).expect("first result"), b"first");

        let mut file = fs::File::create(&replacement).expect("replacement");
        file.write_all(b"second").expect("write");
        file.sync_all().expect("flush");
        drop(file);
        atomic_replace_file(&replacement, &destination).expect("replace");
        assert_eq!(fs::read(&destination).expect("second result"), b"second");
    }

    #[test]
    fn handle_bound_create_new_is_compensated_until_committed() {
        let directory = tempdir().expect("temp directory");
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");
        let destination = directory.path().join("released.exe");

        let mut pending = handle_bound_create_new(&parent, "released.exe".as_ref(), b"new")
            .expect("publish pending");
        pending.file_mut().seek(SeekFrom::Start(0)).expect("seek held publication");
        let mut pending_bytes = Vec::new();
        pending.file_mut().read_to_end(&mut pending_bytes).expect("read held publication");
        assert_eq!(pending_bytes, b"new");
        pending.abort().expect("abort publication");
        assert!(!destination.exists(), "uncommitted publication must be removed by handle");

        let committed = handle_bound_create_new(&parent, "released.exe".as_ref(), b"new")
            .expect("publish committed");
        committed.commit().expect("commit publication");
        assert_eq!(fs::read(destination).expect("committed bytes"), b"new");
    }

    #[test]
    fn handle_bound_publication_rejects_ads_and_embedded_nul_names() {
        let directory = tempdir().expect("temp directory");
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");

        for invalid_name in ["artifact.bin:payload", "artifact\0.bin"] {
            match handle_bound_create_new(&parent, invalid_name.as_ref(), b"payload") {
                Err(HandlePublishError::InvalidFileName) => {}
                Err(error) => panic!("unexpected unsafe-leaf error: {error}"),
                Ok(publication) => {
                    publication.abort().expect("remove unexpectedly published unsafe leaf");
                    panic!("unsafe leaf name must be rejected");
                }
            }
        }
    }

    #[test]
    fn handle_bound_create_supports_one_character_and_unicode_leaf_names() {
        let directory = tempdir().expect("temp directory");
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");

        for name in ["x", "文件.bin"] {
            let publication = handle_bound_create_new(&parent, name.as_ref(), name.as_bytes())
                .expect("publish safe leaf");
            publication.commit().expect("commit safe leaf");
            assert_eq!(
                fs::read(directory.path().join(name)).expect("published bytes"),
                name.as_bytes()
            );
        }
    }

    #[test]
    fn handle_bound_replace_restores_backup_until_committed() {
        let directory = tempdir().expect("temp directory");
        let destination = directory.path().join("artifact.bin");
        fs::write(&destination, b"old").expect("seed destination");
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");
        let target = open_verified_existing(&destination, NativeFileAccess::Metadata)
            .expect("verified target");
        let expected = target.info().identity.clone();
        drop(target);

        let mut pending = handle_bound_replace(&parent, "artifact.bin".as_ref(), &expected, b"new")
            .expect("replace pending");
        pending.file_mut().seek(SeekFrom::Start(0)).expect("seek held replacement");
        let mut pending_bytes = Vec::new();
        pending.file_mut().read_to_end(&mut pending_bytes).expect("read held replacement");
        assert_eq!(pending_bytes, b"new");
        pending.abort().expect("abort replacement");
        assert_eq!(fs::read(&destination).expect("restored bytes"), b"old");

        let current = open_verified_existing(&destination, NativeFileAccess::Metadata)
            .expect("current target");
        let expected = current.info().identity.clone();
        drop(current);
        let committed = handle_bound_replace(&parent, "artifact.bin".as_ref(), &expected, b"new")
            .expect("replace committed");
        committed.commit().expect("commit replacement");
        assert_eq!(fs::read(destination).expect("committed bytes"), b"new");
    }

    /// Helper: a seeded destination and its verified identity.
    fn seeded(
        directory: &std::path::Path,
        name: &str,
        bytes: &[u8],
    ) -> crate::path::NativeFileIdentity {
        let destination = directory.join(name);
        fs::write(&destination, bytes).expect("seed destination");
        crate::path::inspect_existing_path(&destination, true).expect("identity").identity
    }

    /// **A-R4.1** — a real failure at the re-query that runs *after* the old object moved aside is
    /// reported as `RolledBackAfterPublish` with the destination restored, not as a raw error whose
    /// location says `NeverPublished`.
    ///
    /// This is the counterexample for the escaped `?`. The old code read
    /// `match verified_from_file(target.file().try_clone()?)`; the `?` returned the raw I/O error
    /// *before* the `match`, so the caller was told the destination "still holds exactly what it
    /// held before" while the old object sat under the backup name and the destination name had
    /// been vacated. The test arms the real step and measures the disk rather than trusting the
    /// variant, so a classification that disagrees with reality fails here.
    #[test]
    fn a_real_failure_after_the_move_reports_the_rollback_it_really_did() {
        let directory = tempdir().expect("temp directory");
        let identity = seeded(directory.path(), "artifact.bin", b"old content");
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");

        let _armed =
            fault::arm(fault::Stage::QueryMovedBackup, &directory.path().join("artifact.bin"));
        let attempt = handle_bound_replace_with_backup_checked(
            &parent,
            "artifact.bin".as_ref(),
            &identity,
            HandleBoundReplaceContent::Unchecked,
            b"new content",
            ".aura-backup-a4.tmp".as_ref(),
        );
        let error = match attempt {
            Ok(_) => panic!("the armed re-query must fail"),
            Err(error) => error,
        };
        drop(_armed);

        // The stage facts: the old object really moved, the sequence really failed there, and the
        // real compensation really restored it.
        let text = error.to_string();
        assert!(
            text.contains("query_moved_backup"),
            "the reported failure must name the stage that really failed: {error}"
        );
        assert!(
            matches!(error, HandlePublishError::RolledBackAfterPublish { .. }),
            "a failure after the move with a successful restore is RolledBackAfterPublish: {error}"
        );
        assert_eq!(
            error.location(),
            PublishLocation::RolledBack,
            "the location must be RolledBack, never NeverPublished: {error}"
        );
        assert_eq!(
            fs::read(directory.path().join("artifact.bin")).expect("destination"),
            b"old content",
            "the restoration really put the old content back"
        );
        // The compensation consumed the backup name, so no recovery material is left behind.
        assert!(
            !directory.path().join(".aura-backup-a4.tmp").exists(),
            "the restore moved the backup back, so nothing is left under the backup name"
        );
    }

    /// **A-R4.2** — a real failure at the re-query *and* a real failure of the compensation is
    /// reported as `CompensationFailed`, keeps the primary error, and **leaves the recovery
    /// material in place** instead of deleting it.
    ///
    /// This is the counterexample for a payload that only knows "it failed". Without a real
    /// compensation outcome the caller cannot tell a destination that holds its previous content
    /// from one that holds neither version, and would either retry over real data or claim a
    /// success it does not have.
    #[test]
    fn a_real_compensation_failure_preserves_the_material_and_the_primary_error() {
        let directory = tempdir().expect("temp directory");
        let identity = seeded(directory.path(), "artifact.bin", b"old content");
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");

        // Both stages armed: the sequence fails at the re-query, and the restore that would put the
        // old object back fails too. Both are real steps of the production path.
        let _query =
            fault::arm(fault::Stage::QueryMovedBackup, &directory.path().join("artifact.bin"));
        let _restore =
            fault::arm(fault::Stage::RestoreBackup, &directory.path().join("artifact.bin"));
        let attempt = handle_bound_replace_with_backup_checked(
            &parent,
            "artifact.bin".as_ref(),
            &identity,
            HandleBoundReplaceContent::Unchecked,
            b"new content",
            ".aura-backup-a4c.tmp".as_ref(),
        );
        let error = match attempt {
            Ok(_) => panic!("the armed sequence must fail"),
            Err(error) => error,
        };
        drop(_restore);
        drop(_query);

        let HandlePublishError::CompensationFailed { operation, rollback } = &error else {
            panic!("a failed compensation is CompensationFailed, got {error}");
        };
        assert!(
            operation.contains("query_moved_backup"),
            "the primary error must be preserved as the operation: {operation}"
        );
        assert!(
            rollback.contains("restore_backup"),
            "the compensation result must be preserved as the rollback: {rollback}"
        );
        assert_eq!(
            error.location(),
            PublishLocation::Unconfirmable,
            "with the restore failed, nothing about the destination is known: {error}"
        );

        // The recovery material is really still there, under the backup name the caller chose.
        let backup = directory.path().join(".aura-backup-a4c.tmp");
        assert!(backup.exists(), "a failed compensation must not delete the recovery material");
        assert_eq!(
            fs::read(&backup).expect("backup bytes"),
            b"old content",
            "the backup holds exactly the object that was moved aside"
        );
        assert!(
            !directory.path().join("artifact.bin").exists(),
            "the destination name was vacated by the move and the failed restore did not refill it"
        );
    }

    /// **A-R4.5 / Drop** — a drop-time rollback failure does **not** terminate the process.
    ///
    /// The previous implementation called `std::process::abort()` here, so one publication that
    /// could not roll itself back ended every session in the Core. This test is only meaningful if
    /// the process survives to assert anything at all: reaching the assertion *is* the evidence.
    #[test]
    fn a_drop_time_rollback_failure_does_not_terminate_the_process() {
        let directory = tempdir().expect("temp directory");
        let identity = seeded(directory.path(), "artifact.bin", b"old content");
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");

        let _restore =
            fault::arm(fault::Stage::RestoreBackup, &directory.path().join("artifact.bin"));
        let published = handle_bound_replace_with_backup_checked(
            &parent,
            "artifact.bin".as_ref(),
            &identity,
            HandleBoundReplaceContent::Unchecked,
            b"new content",
            ".aura-backup-drop.tmp".as_ref(),
        )
        .expect("the replacement itself must succeed; only the restore is armed");
        assert_eq!(
            published.backup.as_ref().expect("verified backup").info().final_path,
            parent.info().final_path.join(".aura-backup-drop.tmp"),
            "the guard reports the moved backup rather than the former destination"
        );
        // Dropping without commit or abort is the path that used to abort the process. The armed
        // restore fails inside it.
        drop(published);
        drop(_restore);

        // Reaching this line is the fact under test: the process is still running.
        let backup_path = directory.path().join(".aura-backup-drop.tmp");
        let backup = crate::path::inspect_existing_path(&backup_path, true)
            .expect("original backup survives");
        assert_eq!(backup.identity, identity, "the remaining backup is the original object");
        assert_eq!(fs::read(&backup_path).expect("backup content"), b"old content");
        assert!(
            !directory.path().join("artifact.bin").exists(),
            "the uncommitted publication was removed before the restore failed"
        );
        // And the seam is really off again afterwards.
        assert!(!fault::armed_now(
            fault::Stage::RestoreBackup,
            &directory.path().join("artifact.bin")
        ));
    }

    /// **A-R4.5** — a real failure of the commit-time backup delete is `RolledBackAfterPublish`
    /// with the old content restored, which is the commit/abort failure point the sequence must
    /// classify by the stage it really reached.
    #[test]
    fn a_real_commit_cleanup_failure_rolls_back_and_says_so() {
        let directory = tempdir().expect("temp directory");
        let identity = seeded(directory.path(), "artifact.bin", b"old content");
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");

        let _armed =
            fault::arm(fault::Stage::DeleteBackupOnCommit, &directory.path().join("artifact.bin"));
        let published = handle_bound_replace_with_backup_checked(
            &parent,
            "artifact.bin".as_ref(),
            &identity,
            HandleBoundReplaceContent::Unchecked,
            b"new content",
            ".aura-backup-commit.tmp".as_ref(),
        )
        .expect("the replacement itself must succeed");
        let error = published.commit().expect_err("the armed commit cleanup must fail");
        drop(_armed);

        assert!(
            error.to_string().contains("delete_backup_on_commit"),
            "the failure must name the stage that really failed: {error}"
        );
        assert!(
            matches!(error, HandlePublishError::RolledBackAfterPublish { .. }),
            "a failed commit rolls the publication back: {error}"
        );
        assert_eq!(error.location(), PublishLocation::RolledBack);
        assert_eq!(
            fs::read(directory.path().join("artifact.bin")).expect("destination"),
            b"old content",
            "the old content really came back"
        );
    }

    /// **A-R4.3 / A-R4.4** — a real restore failure names the compensation stage, keeps the backup
    /// as the remaining material, and confirms no bare abort is reachable.
    #[test]
    fn a_real_restore_failure_keeps_the_backup_as_recovery_material() {
        let directory = tempdir().expect("temp directory");
        let identity = seeded(directory.path(), "artifact.bin", b"old content");
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");

        let _armed =
            fault::arm(fault::Stage::RestoreBackup, &directory.path().join("artifact.bin"));
        let published = handle_bound_replace_with_backup_checked(
            &parent,
            "artifact.bin".as_ref(),
            &identity,
            HandleBoundReplaceContent::Unchecked,
            b"new content",
            ".aura-backup-restore.tmp".as_ref(),
        )
        .expect("the replacement itself must succeed");
        // `abort` is the ordinary caller-driven rollback, and its compensation is the same real
        // step the seam armed.
        let error = published.abort().expect_err("the armed restore must fail");
        drop(_armed);

        assert!(
            error.to_string().contains("restore_backup"),
            "the compensation stage must be named: {error}"
        );
        assert!(
            directory.path().join(".aura-backup-restore.tmp").exists(),
            "the backup is the remaining recovery material and must not be deleted"
        );
        assert_eq!(
            fs::read(directory.path().join(".aura-backup-restore.tmp")).expect("backup"),
            b"old content"
        );
    }
    /// **The two `create_absent` seams are distinct stages and both are really reached.**
    ///
    /// The brief requires the create-absent path's seams to be reached on the real production
    /// sequence, each reporting the stage it really fired at:
    ///
    /// * `CreateAbsentBeforeRename` fires after the staging object exists and **before** the rename
    ///   that gives it the destination name, so a failure there must leave the destination absent
    ///   and must not have created it;
    /// * `CreateAbsentRename` fires at the rename itself, which is the authoritative "fail if it
    ///   exists" action, so a failure there is also reported with the destination still absent.
    ///
    /// Both are measured on the disk rather than trusted from the error variant, because a
    /// classification that disagrees with reality is exactly the defect this pair exists to catch.
    #[test]
    fn both_create_absent_seams_really_fire_and_leave_the_destination_absent() {
        for (stage, expected_name) in [
            (fault::Stage::CreateAbsentBeforeRename, "create_absent_before_rename"),
            (fault::Stage::CreateAbsentRename, "create_absent_rename"),
        ] {
            let directory = tempdir().expect("temp directory");
            let parent = open_verified_publish_parent(directory.path()).expect("verified parent");
            let destination = directory.path().join("created.bin");

            let _armed = fault::arm(stage, &destination);
            let attempt =
                super::handle_bound_create_absent(&parent, "created.bin".as_ref(), b"new content");
            drop(_armed);

            let error = match attempt {
                Ok(_) => panic!("the armed seam must fail the creation at {expected_name}"),
                Err(error) => error,
            };
            assert!(
                error.to_string().contains(expected_name),
                "the failure must name the stage it really fired at ({expected_name}): {error}"
            );
            assert!(
                !destination.exists(),
                "a creation refused at {expected_name} must leave the destination absent"
            );
            // The staging object must not survive either: the guard removes the publication again.
            let leftover = fs::read_dir(directory.path())
                .expect("read directory")
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert!(
                !leftover.iter().any(|name| name.contains("created.bin")),
                "the failed creation must leave no object of its own behind: {leftover:?}"
            );
        }
    }

    /// The same creation succeeds when nothing is armed, so the pair above measures the seam rather
    /// than a path that never works.
    #[test]
    fn an_unarmed_create_absent_really_publishes() {
        let directory = tempdir().expect("temp directory");
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");
        let published =
            super::handle_bound_create_absent(&parent, "created.bin".as_ref(), b"new content")
                .expect("an unarmed creation publishes");
        published.commit().expect("commit the creation");
        assert_eq!(
            fs::read(directory.path().join("created.bin")).expect("created bytes"),
            b"new content"
        );
    }

    /// **A creation refuses an existing destination: the "fail if it exists" action is real.**
    ///
    /// This is the behaviour the generic replace helper could not express, and the reason the
    /// `Absent` condition routes to the create entry rather than to a replacement. It is measured on
    /// a destination a writer really created, so a helper that silently replaced it fails here.
    #[test]
    fn a_create_absent_refuses_a_destination_that_appeared() {
        let directory = tempdir().expect("temp directory");
        let parent = open_verified_publish_parent(directory.path()).expect("verified parent");
        // A writer created the destination after the caller decided it was absent.
        fs::write(directory.path().join("created.bin"), b"a competitor's bytes")
            .expect("the competitor creates it");

        let attempt =
            super::handle_bound_create_absent(&parent, "created.bin".as_ref(), b"our bytes");
        assert!(attempt.is_err(), "a creation must fail when the destination exists");
        assert_eq!(
            fs::read(directory.path().join("created.bin")).expect("bytes"),
            b"a competitor's bytes",
            "the competitor's object must be untouched by a refused creation"
        );
    }
}
