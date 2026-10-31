use std::{
    cmp::Ordering,
    ffi::OsStr,
    fs::File,
    io,
    mem::{size_of, zeroed},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, Path, PathBuf},
};

use windows_sys::Win32::{
    Foundation::{ERROR_SHARING_VIOLATION, GENERIC_READ, GENERIC_WRITE},
    Globalization::{CSTR_EQUAL, CSTR_GREATER_THAN, CSTR_LESS_THAN, CompareStringOrdinal},
    Storage::FileSystem::{
        CreateFileW, DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
        FILE_ATTRIBUTE_TAG_INFO, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_ID_INFO, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE,
        FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_STANDARD_INFO, FILE_TRAVERSE, FileAttributeTagInfo,
        FileIdInfo, FileStandardInfo, GetFileInformationByHandleEx, GetFinalPathNameByHandleW,
        OPEN_EXISTING, VOLUME_NAME_DOS,
    },
};

use crate::handle::{BorrowedHandle, OwnedHandle};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NativeFileAccess {
    Metadata,
    Read,
    ReadWrite,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeFileIdentity {
    pub volume_serial_number: u64,
    pub file_id: [u8; 16],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativePathInfo {
    pub final_path: PathBuf,
    pub identity: NativeFileIdentity,
    pub is_directory: bool,
    pub is_reparse_point: bool,
    pub reparse_tag: u32,
    /// Number of hard links reported by `FILE_STANDARD_INFO` for this exact
    /// handle. Tool output import requires exactly one link so an Attempt file
    /// cannot alias an object outside its private output root.
    pub number_of_links: u32,
    /// `FILE_STANDARD_INFO::DeletePending` for this exact handle: the volume has
    /// marked the object for deletion, so it will exist at no path once the last
    /// handle closes. Measured on NTFS (Windows 11 26100, this machine): an object
    /// deleted while a handle is still held reports this set and `number_of_links`
    /// 0, and `OpenFileById` on its recorded identity still *succeeds*, answering
    /// with the `$Extend\$Deleted` residue instead of a live object. An object that
    /// is genuinely live reports it clear, including one whose count of live names
    /// is not 1 (a hard-linked or renamed object). It is carried here because it is
    /// the volume's own statement that the object is on its way out, which a caller
    /// deciding "does this object still exist" has to be able to read.
    pub delete_pending: bool,
}

#[derive(Debug)]
pub struct VerifiedFile {
    file: File,
    info: NativePathInfo,
}

/// Handle-bound view of one Attempt output. Root and intermediate-directory
/// handles are retained so none of the verified path components can be
/// replaced before the caller finishes reading and importing the file.
pub struct VerifiedToolOutputFile {
    root: VerifiedFile,
    parents: Vec<VerifiedFile>,
    file: VerifiedFile,
}

impl VerifiedToolOutputFile {
    #[must_use]
    pub fn root_info(&self) -> &NativePathInfo {
        self.root.info()
    }

    #[must_use]
    pub fn info(&self) -> &NativePathInfo {
        self.file.info()
    }

    #[must_use]
    pub fn file(&self) -> &File {
        self.file.file()
    }

    #[must_use]
    pub fn file_mut(&mut self) -> &mut File {
        self.file.file_mut()
    }

    #[must_use]
    pub fn held_parent_count(&self) -> usize {
        self.parents.len()
    }
}

impl VerifiedFile {
    #[must_use]
    pub fn info(&self) -> &NativePathInfo {
        &self.info
    }

    #[must_use]
    pub fn file(&self) -> &File {
        &self.file
    }

    #[must_use]
    pub fn file_mut(&mut self) -> &mut File {
        &mut self.file
    }

    #[must_use]
    pub fn into_file(self) -> File {
        self.file
    }
}

pub fn inspect_existing_path(path: &Path, follow_reparse: bool) -> io::Result<NativePathInfo> {
    open_native(path, NativeFileAccess::Metadata, follow_reparse).map(|verified| verified.info)
}

/// The component of `path` that names its volume root (`C:\`, `\\server\share\`), if there is one.
///
/// The pre-fix walk treated *any* trailing separator as a volume root, which is wrong: the
/// separator is directory syntax, not a volume statement. The component that really names the root
/// is a `Prefix` (a drive letter or a UNC share), optionally followed by the root directory. A
/// relative path states no volume at all and answers `None`.
#[must_use]
pub fn volume_root_of(path: &Path) -> Option<PathBuf> {
    let mut components = path.components();
    match components.next()? {
        Component::Prefix(prefix) => {
            let mut root = PathBuf::from(prefix.as_os_str());
            // `C:` alone names a drive-relative path; `C:\` names the volume root. Only the latter
            // is a place a volume's flags can be asked about, so the separator is added only when
            // the path really carries it.
            if matches!(components.next(), Some(Component::RootDir)) {
                root.push(std::path::MAIN_SEPARATOR_STR);
            }
            Some(root)
        }
        Component::RootDir => Some(PathBuf::from(std::path::MAIN_SEPARATOR_STR)),
        Component::CurDir | Component::ParentDir | Component::Normal(_) => None,
    }
}

/// How the volume a path lives on reports its case behaviour.
///
/// This is a **fact read from the volume**, not a rule this code asserts. It asks the volume for
/// its filesystem flags through `GetVolumeInformationW`, which is the same authority the object
/// manager consults when it resolves a name.
///
/// * `FILE_CASE_SENSITIVE_SEARCH` set means the volume *supports* case-sensitive directory lookups;
/// * clear means it does not;
/// * a path whose volume cannot be read answers [`VolumeCaseRule::Unknown`]. That is deliberately
///   *not* the same as "case-insensitive": the caller then treats the slot as having no determinate
///   identity, so an unreadable volume makes the owner refuse rather than merge two spellings on a
///   guess. Nothing here defaults to a rule it did not read.
///
/// # Why this alone is not the *effective* rule for one directory
///
/// `FILE_CASE_SENSITIVE_SEARCH` is a **capability flag on the volume**, and that was measured
/// directly on this machine: `C:\`'s native flags contain the bit, while `CaseProbe.txt` and
/// `caseprobe.txt` still resolve to the *same* file id there. Reporting `Sensitive` from that bit
/// would claim a rule the directory does not follow, which is exactly what the pre-fix code did —
/// and the pre-fix `ResourceFacts` rule was consequently `Unknown`, honestly, but derived from the
/// TEMP volume rather than from the target.
///
/// A caller that holds the **target directory's** verified handle must therefore use
/// [`directory_case_rule`], which reads the directory's own flag. This function remains for callers
/// that only have a path and are asking about the volume.
#[must_use]
pub fn volume_case_rule(path: &Path) -> VolumeCaseRule {
    use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW;

    // `FILE_CASE_SENSITIVE_SEARCH` from `Win32_System_SystemServices`. Spelled here rather than
    // imported because enabling that windows-sys feature would widen this crate's generated bindings
    // for one constant; the compiler still checks the call, and the bit is the API's own documented
    // value for `GetVolumeInformationW`'s filesystem-flags output.
    const FILE_CASE_SENSITIVE_SEARCH: u32 = 0x0000_0001;

    let Some(root) = volume_root_of(path) else {
        // A relative path names no volume, so no volume fact can be read about it.
        return VolumeCaseRule::Unknown;
    };
    let mut wide = root.as_os_str().encode_wide().collect::<Vec<u16>>();
    wide.push(0);
    let mut flags: u32 = 0;
    // SAFETY: `wide` is a NUL-terminated UTF-16 path, and every output buffer is either null with
    // a zero size (valid: the call then only reports what it needs) or a correctly sized u32.
    let ok = unsafe {
        GetVolumeInformationW(
            wide.as_ptr(),
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &raw mut flags,
            std::ptr::null_mut(),
            0,
        )
    };
    if ok == 0 {
        return VolumeCaseRule::Unknown;
    }
    if flags & FILE_CASE_SENSITIVE_SEARCH != 0 {
        VolumeCaseRule::Sensitive
    } else {
        VolumeCaseRule::InsensitivePreserving
    }
}

/// The **effective** case rule of the directory one verified handle names.
///
/// A directory carries its own case-sensitivity flag (the WSL-style per-directory flag, set through
/// `fsutil file setCaseSensitiveInfo`). It is part of the directory's own information and is read
/// here from the handle the caller already verified, so the answer describes *this* directory
/// rather than the volume's capability.
///
/// * the flag is present and set: the directory really distinguishes case;
/// * the flag is present and clear: the directory really folds case;
/// * the flag cannot be read — an older filesystem, a volume that does not carry it: `Unknown`,
///   which the owner then treats as a missing fact rather than as a free resource.
///
/// Deliberately **not** a fallback to the volume's capability bit: that bit is not this directory's
/// rule, and using it is the defect this function replaces.
#[must_use]
pub fn directory_case_rule(directory: &VerifiedFile) -> VolumeCaseRule {
    match directory_case_sensitivity_flag(directory) {
        Some(true) => VolumeCaseRule::Sensitive,
        Some(false) => VolumeCaseRule::InsensitivePreserving,
        None => VolumeCaseRule::Unknown,
    }
}

/// Reads the per-directory case-sensitivity flag from one held directory handle.
///
/// `Some(true)` when the directory's own information carries the flag set, `Some(false)` when it
/// carries it clear, and `None` when it could not be read — which is a missing fact, never a claim.
#[must_use]
pub fn directory_case_sensitivity_flag(directory: &VerifiedFile) -> Option<bool> {
    use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx;

    /// `FileCaseSensitiveInfo` from `Win32_Storage_FileSystem`, carrying `FILE_CASE_SENSITIVE_INFO`.
    const FILE_CASE_SENSITIVE_INFO_CLASS: i32 = 23;
    /// `FILE_CS_FLAG_CASE_SENSITIVE_DIR`.
    const FILE_CS_FLAG_CASE_SENSITIVE_DIR: u32 = 0x0000_0001;

    #[repr(C)]
    struct FileCaseSensitiveInfo {
        flags: u32,
    }

    let mut info = FileCaseSensitiveInfo { flags: 0 };
    // SAFETY: the handle is a live handle owned by `directory`, and the output buffer is a
    // correctly sized `FILE_CASE_SENSITIVE_INFO`.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            directory.file().as_raw_handle().cast(),
            FILE_CASE_SENSITIVE_INFO_CLASS,
            (&raw mut info).cast(),
            u32::try_from(size_of::<FileCaseSensitiveInfo>()).expect("the struct fits u32"),
        )
    };
    if ok == 0 {
        return None;
    }
    Some(info.flags & FILE_CS_FLAG_CASE_SENSITIVE_DIR != 0)
}

/// What a volume really does about case, read from the volume.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VolumeCaseRule {
    /// The volume folds case for ordinary directory lookups.
    InsensitivePreserving,
    /// The volume distinguishes case for ordinary directory lookups.
    Sensitive,
    /// The volume's flags could not be read, so no rule is claimed.
    Unknown,
}

/// The identity of the object one **already-held** file handle names.
///
/// Read from the handle rather than from a path, so the answer describes the object the caller is
/// really holding even if the name has since been taken by something else. This is the primitive a
/// caller uses to re-verify "the object I decided about" at a boundary where a path lookup could no
/// longer be trusted.
///
/// # Errors
/// Returns the real Win32 failure. A handle that cannot be queried is *not* reported as a different
/// object: the caller has to see that it could not establish the fact.
pub fn held_file_identity(file: &File) -> io::Result<NativeFileIdentity> {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ID_INFO, FileIdInfo, GetFileInformationByHandleEx,
    };

    let mut identity: FILE_ID_INFO = unsafe { zeroed() };
    // SAFETY: the handle is live and owned by the caller, and the output buffer is a correctly
    // sized `FILE_ID_INFO`.
    let ok = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle().cast(),
            FileIdInfo,
            (&raw mut identity).cast(),
            u32::try_from(size_of::<FILE_ID_INFO>()).expect("FILE_ID_INFO fits u32"),
        )
    };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(NativeFileIdentity {
        volume_serial_number: identity.VolumeSerialNumber,
        file_id: identity.FileId.Identifier,
    })
}

/// Opens one existing object for a caller that must **hold it across a path operation**.
///
/// Holds the selected object for an identity-bound move. The handle includes DELETE access,
/// admits ordinary readers, and denies competing writes and deletes. A caller must use
/// `handle_bound_move_existing` on this handle; reopening the source by path is not safe.
///
/// # Errors
/// Returns the actual open failure; a sharing refusal never means that the object is absent.
pub fn open_held_for_move(path: &Path) -> io::Result<VerifiedFile> {
    open_native_with_desired_access(path, GENERIC_READ | DELETE, false, FILE_SHARE_READ)
}

/// Read-only snapshot handle that denies competing writes and renames without
/// requiring delete access on the source being copied.
pub fn open_held_for_read(path: &Path) -> io::Result<VerifiedFile> {
    open_native_with_desired_access(path, GENERIC_READ, false, FILE_SHARE_READ)
}

pub fn open_verified_existing(path: &Path, access: NativeFileAccess) -> io::Result<VerifiedFile> {
    open_native(path, access, true)
}

/// No-follow search open. Prefer denying writers so same-length rewrites cannot
/// race the scan; if a writer already holds the file, fall back to allowing
/// write sharing and require a same-handle rehash before committing matches.
pub struct SearchSource {
    file: VerifiedFile,
    writers_denied: bool,
}

impl SearchSource {
    #[must_use]
    pub fn info(&self) -> &NativePathInfo {
        self.file.info()
    }

    #[must_use]
    pub fn file_mut(&mut self) -> &mut File {
        self.file.file_mut()
    }

    #[must_use]
    pub fn writers_denied(&self) -> bool {
        self.writers_denied
    }

    #[must_use]
    pub fn identity(&self) -> &NativeFileIdentity {
        &self.file.info().identity
    }
}

pub fn open_verified_search_source(path: &Path) -> io::Result<SearchSource> {
    match open_native_with_share(path, NativeFileAccess::Read, false, FILE_SHARE_READ) {
        Ok(file) => search_source(file, true),
        Err(error)
            if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32)
                || error.kind() == io::ErrorKind::PermissionDenied =>
        {
            search_source(
                open_native_with_share(
                    path,
                    NativeFileAccess::Read,
                    false,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                )?,
                false,
            )
        }
        Err(error) => Err(error),
    }
}

fn search_source(file: VerifiedFile, writers_denied: bool) -> io::Result<SearchSource> {
    if file.info.is_directory {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "search source is a directory"));
    }
    if file.info.is_reparse_point {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "search source is a reparse point",
        ));
    }
    Ok(SearchSource { file, writers_denied })
}

/// Metadata-only, no-follow execution binding. Share modes allow a separate
/// retained ACL handle to own the directory's no-delete lease. Preparation
/// compares identity again at dispatch; this handle alone is not a scope lock.
pub fn open_execution_metadata(path: &Path) -> io::Result<VerifiedFile> {
    open_native(path, NativeFileAccess::Metadata, false)
}

/// One installation-wide execution resource lease across Core instances. The
/// installer creates this protected file; an absent/aliased lock is not adopted.
pub fn lock_execution_resources(path: &Path) -> io::Result<VerifiedFile> {
    // Share exclusion applies to data access, not a metadata-only handle.
    let file = open_native_with_desired_access(path, GENERIC_READ, false, 0)?;
    if file.info.is_directory || file.info.is_reparse_point || file.info.number_of_links != 1 {
        return Err(io::Error::other("invalid execution resource lock"));
    }
    Ok(file)
}

/// Opens a package-import source without following a reparse point and keeps
/// the verified handle alive for the caller's read. Only concurrent readers
/// are shared; callers must still verify the final length and content hash in
/// case a writer already held a compatible handle before this call.
pub fn open_verified_import_source(
    path: &Path,
    access: NativeFileAccess,
) -> io::Result<VerifiedFile> {
    let verified = open_native_with_share(path, access, false, FILE_SHARE_READ)?;
    if verified.info.is_reparse_point {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "package import source is a reparse point",
        ));
    }
    Ok(verified)
}

/// Opens an Attempt-local tool output without following the final path
/// component and denies concurrent write, rename, and deletion while the
/// caller verifies and imports the bytes.
pub fn open_verified_tool_output_source(path: &Path) -> io::Result<VerifiedFile> {
    let verified = open_native_with_share(path, NativeFileAccess::Read, false, FILE_SHARE_READ)?;
    if verified.info.is_directory
        || verified.info.is_reparse_point
        || verified.info.number_of_links != 1
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tool output must be a regular, non-reparse, single-link file",
        ));
    }
    Ok(verified)
}

/// Opens and holds an Attempt output root without following a reparse point.
/// The restrictive share mode prevents replacement while child paths are
/// checked against this exact directory identity.
pub fn open_verified_tool_output_root(path: &Path) -> io::Result<VerifiedFile> {
    let verified =
        open_native_with_share(path, NativeFileAccess::Metadata, false, FILE_SHARE_READ)?;
    if !verified.info.is_directory || verified.info.is_reparse_point {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tool output root must be a non-reparse directory",
        ));
    }
    Ok(verified)
}

/// Opens an Attempt output relative to an exact private root. Every component
/// is opened no-follow, checked against the held root identity, and retained
/// until the returned value is dropped.
pub fn open_verified_tool_output_from_root(
    root_path: &Path,
    relative_path: &Path,
) -> io::Result<VerifiedToolOutputFile> {
    if relative_path.is_absolute()
        || relative_path.components().any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tool output path must contain only relative normal components",
        ));
    }
    let components = relative_path.components().collect::<Vec<_>>();
    if components.is_empty() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "tool output path is empty"));
    }

    let root = open_verified_tool_output_root(root_path)?;
    let mut current = root_path.to_path_buf();
    let mut parents = Vec::with_capacity(components.len().saturating_sub(1));
    for component in &components[..components.len() - 1] {
        current.push(component.as_os_str());
        let parent =
            open_native_with_share(&current, NativeFileAccess::Metadata, false, FILE_SHARE_READ)?;
        if !parent.info.is_directory
            || parent.info.is_reparse_point
            || !final_path_within(root.info(), parent.info())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "tool output parent is not a contained, non-reparse directory",
            ));
        }
        parents.push(parent);
    }
    current.push(components.last().expect("non-empty component list").as_os_str());
    let file = open_verified_tool_output_source(&current)?;
    if !final_path_within(root.info(), file.info()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "tool output resolved outside the private output root",
        ));
    }
    Ok(VerifiedToolOutputFile { root, parents, file })
}

/// Opens and holds a destination directory without FILE_SHARE_DELETE so its
/// final path cannot be renamed or replaced during a handle-bound publish.
pub fn open_verified_publish_parent(path: &Path) -> io::Result<VerifiedFile> {
    // FILE_RENAME_INFO.RootDirectory is documented to require a target
    // directory handle opened for traverse/read-attributes. Keep this exact
    // instead of using a broader generic write mapping.
    let parent = open_native_with_desired_access(
        path,
        FILE_TRAVERSE | FILE_READ_ATTRIBUTES,
        true,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
    )?;
    if !parent.info.is_directory {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "publish parent is not a directory",
        ));
    }
    Ok(parent)
}

/// Opens an executable for verification while denying concurrent writes and
/// replacement. Keep the returned handle alive until the suspended process
/// has been created and assigned to its Job.
pub fn open_verified_executable(path: &Path) -> io::Result<VerifiedFile> {
    open_native_with_share(path, NativeFileAccess::Read, true, FILE_SHARE_READ)
}

fn open_native(
    path: &Path,
    access: NativeFileAccess,
    follow_reparse: bool,
) -> io::Result<VerifiedFile> {
    open_native_with_share(
        path,
        access,
        follow_reparse,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    )
}

fn open_native_with_share(
    path: &Path,
    access: NativeFileAccess,
    follow_reparse: bool,
    share_mode: u32,
) -> io::Result<VerifiedFile> {
    let desired_access = match access {
        NativeFileAccess::Metadata => FILE_READ_ATTRIBUTES,
        NativeFileAccess::Read => GENERIC_READ,
        NativeFileAccess::ReadWrite => GENERIC_READ | GENERIC_WRITE,
    };
    open_native_with_desired_access(path, desired_access, follow_reparse, share_mode)
}

pub(crate) fn open_native_with_desired_access(
    path: &Path,
    desired_access: u32,
    follow_reparse: bool,
    share_mode: u32,
) -> io::Result<VerifiedFile> {
    let wide = wide_path(path)?;
    let mut flags = FILE_FLAG_BACKUP_SEMANTICS;
    if !follow_reparse {
        flags |= FILE_FLAG_OPEN_REPARSE_POINT;
    }
    // SAFETY: `wide` is NUL-terminated and remains alive for the call. The
    // returned owned handle is validated and transferred exactly once to File.
    let raw = unsafe {
        CreateFileW(
            wide.as_ptr(),
            desired_access,
            share_mode,
            std::ptr::null(),
            OPEN_EXISTING,
            flags,
            std::ptr::null_mut(),
        )
    };
    let handle = OwnedHandle::from_raw(raw)?;
    let info = query_info(handle.borrow())?;
    let raw = handle.into_raw();
    // SAFETY: ownership of the valid handle was removed from `OwnedHandle` and
    // is transferred to exactly one `File`.
    let file = unsafe { File::from_raw_handle(raw) };
    Ok(VerifiedFile { file, info })
}

fn query_info(handle: BorrowedHandle<'_>) -> io::Result<NativePathInfo> {
    // SAFETY: both structures are plain Win32 output buffers with exact sizes.
    let mut identity: FILE_ID_INFO = unsafe { zeroed() };
    // SAFETY: handle is valid and the output buffer is writable for its size.
    let identity_ok = unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw(),
            FileIdInfo,
            (&raw mut identity).cast(),
            u32::try_from(size_of::<FILE_ID_INFO>()).expect("FILE_ID_INFO size fits u32"),
        )
    };
    if identity_ok == 0 {
        return Err(io::Error::last_os_error());
    }

    // SAFETY: same buffer contract as above for FILE_ATTRIBUTE_TAG_INFO.
    let mut attributes: FILE_ATTRIBUTE_TAG_INFO = unsafe { zeroed() };
    // SAFETY: handle is valid and the output buffer is writable for its size.
    let attributes_ok = unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw(),
            FileAttributeTagInfo,
            (&raw mut attributes).cast(),
            u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>())
                .expect("FILE_ATTRIBUTE_TAG_INFO size fits u32"),
        )
    };
    if attributes_ok == 0 {
        return Err(io::Error::last_os_error());
    }

    // `FILE_STANDARD_INFO::NumberOfLinks` is the authoritative Win32 hard-link
    // count for the opened handle. Keep it in the same identity snapshot used
    // for the final-path and reparse checks.
    let mut standard: FILE_STANDARD_INFO = unsafe { zeroed() };
    // SAFETY: handle is valid and the output buffer is writable for its exact
    // Win32 structure size.
    let standard_ok = unsafe {
        GetFileInformationByHandleEx(
            handle.as_raw(),
            FileStandardInfo,
            (&raw mut standard).cast(),
            u32::try_from(size_of::<FILE_STANDARD_INFO>())
                .expect("FILE_STANDARD_INFO size fits u32"),
        )
    };
    if standard_ok == 0 {
        return Err(io::Error::last_os_error());
    }

    let final_path = final_path(handle)?;
    Ok(NativePathInfo {
        final_path,
        identity: NativeFileIdentity {
            volume_serial_number: identity.VolumeSerialNumber,
            file_id: identity.FileId.Identifier,
        },
        is_directory: attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0,
        is_reparse_point: attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0,
        reparse_tag: attributes.ReparseTag,
        number_of_links: standard.NumberOfLinks,
        delete_pending: standard.DeletePending,
    })
}

pub(crate) fn verified_from_file(file: File) -> io::Result<VerifiedFile> {
    let handle = BorrowedHandle::borrow(&file);
    // `handle` is a view of the handle `file` owns, so a failure here ends the
    // borrow and leaves `file` as the only close owner. Nothing is forgotten on
    // either path: `file` is moved into the result exactly once.
    let info = query_info(handle)?;
    Ok(VerifiedFile { file, info })
}

/// Adopt an owned native handle as a verified object. Used where the handle comes
/// from an identity-bound open rather than a path `CreateFileW`.
pub(crate) fn verified_from_owned_handle(handle: OwnedHandle) -> io::Result<VerifiedFile> {
    let info = query_info(handle.borrow())?;
    let raw = handle.into_raw();
    // SAFETY: ownership of the valid handle was removed from `OwnedHandle` and is
    // transferred to exactly one `File`.
    let file = unsafe { File::from_raw_handle(raw) };
    Ok(VerifiedFile { file, info })
}

fn final_path(handle: BorrowedHandle<'_>) -> io::Result<PathBuf> {
    let flags = FILE_NAME_NORMALIZED | VOLUME_NAME_DOS;
    // SAFETY: a null buffer with size zero requests the required length.
    let required =
        unsafe { GetFinalPathNameByHandleW(handle.as_raw(), std::ptr::null_mut(), 0, flags) };
    if required == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buffer = vec![0_u16; usize::try_from(required).unwrap_or(usize::MAX).saturating_add(1)];
    // SAFETY: `buffer` is writable for the reported number of UTF-16 code units.
    let written = unsafe {
        GetFinalPathNameByHandleW(
            handle.as_raw(),
            buffer.as_mut_ptr(),
            u32::try_from(buffer.len()).unwrap_or(u32::MAX),
            flags,
        )
    };
    if written == 0 || usize::try_from(written).is_ok_and(|length| length >= buffer.len()) {
        return Err(io::Error::last_os_error());
    }
    buffer.truncate(usize::try_from(written).expect("Win32 path length fits usize"));
    Ok(PathBuf::from(String::from_utf16_lossy(&buffer)))
}

fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    if !path.is_absolute() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "path must be absolute"));
    }
    // Win32 opens are limited to `MAX_PATH` unless the path is given in extended
    // form. The immutable Package Store nests `packages/<id>/<version>/<64 hex>/…`
    // under the data root, which a namespaced data root can push past that limit:
    // enumeration of the parent still succeeds while opening the manifest fails with
    // "path not found". The extended form removes the limit for verified opens
    // without changing the path this process reasons about.
    let extended = extended_length_path(path);
    let mut wide = OsStr::new(&extended).encode_wide().collect::<Vec<_>>();
    if wide.contains(&0) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "path contains NUL"));
    }
    wide.push(0);
    Ok(wide)
}

/// The `\\?\` form of an absolute Windows path.
///
/// Device paths (`\\.\…`), paths that already carry a prefix, and relative paths are
/// returned unchanged, so this is safe to apply unconditionally to a path that is
/// about to be handed to a Win32 file API.
///
/// A verbatim path is taken literally by the API — it is not normalized — so this
/// refuses to convert anything it would otherwise change the meaning of: a path with
/// `.`/`..` components is returned unchanged (resolving them textually could cross a
/// reparse point), and forward slashes are rewritten first because leaving them in a
/// verbatim path is `ERROR_INVALID_NAME`.
#[must_use]
pub fn extended_length_path(path: &Path) -> PathBuf {
    let text = path.as_os_str().to_string_lossy();
    if text.starts_with(r"\\?\") || text.starts_with(r"\\.\") {
        return path.to_path_buf();
    }
    // `Path::components` drops interior `.` segments, so the check is on the text:
    // any `.`/`..` segment means the verbatim form would change the meaning.
    let has_dot_segment = text.split(['\\', '/']).any(|segment| segment == "." || segment == "..");
    if has_dot_segment {
        return path.to_path_buf();
    }
    let normalized = text.replace('/', "\\");
    if let Some(rest) = normalized.strip_prefix(r"\\") {
        return PathBuf::from(format!(r"\\?\UNC\{rest}"));
    }
    let drive_absolute = path.is_absolute()
        && normalized.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
        && normalized.as_bytes().get(1) == Some(&b':');
    if drive_absolute { PathBuf::from(format!(r"\\?\{normalized}")) } else { path.to_path_buf() }
}

#[must_use]
pub fn final_path_within(root: &NativePathInfo, candidate: &NativePathInfo) -> bool {
    if root.identity.volume_serial_number != candidate.identity.volume_serial_number {
        return false;
    }
    let root_components = root.final_path.components().collect::<Vec<_>>();
    let candidate_components = candidate.final_path.components().collect::<Vec<_>>();
    root_components.len() <= candidate_components.len()
        && root_components
            .iter()
            .zip(candidate_components.iter())
            .all(|(left, right)| component_eq(left, right))
}

/// Compares Windows path text with the operating system's ordinal,
/// case-insensitive table. This is the same non-linguistic comparison needed
/// to reject package paths that would alias in a case-insensitive namespace.
pub fn windows_ordinal_compare_ignore_case(left: &OsStr, right: &OsStr) -> io::Result<Ordering> {
    let left = left.encode_wide().collect::<Vec<_>>();
    let right = right.encode_wide().collect::<Vec<_>>();
    let left_len = i32::try_from(left.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "left path is too long"))?;
    let right_len = i32::try_from(right.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "right path is too long"))?;
    // SAFETY: both UTF-16 buffers remain alive for the call, their explicit
    // lengths fit i32, and TRUE is exactly 1 as required by the API.
    let comparison =
        unsafe { CompareStringOrdinal(left.as_ptr(), left_len, right.as_ptr(), right_len, 1) };
    match comparison {
        CSTR_LESS_THAN => Ok(Ordering::Less),
        CSTR_EQUAL => Ok(Ordering::Equal),
        CSTR_GREATER_THAN => Ok(Ordering::Greater),
        _ => Err(io::Error::last_os_error()),
    }
}

fn component_eq(left: &Component<'_>, right: &Component<'_>) -> bool {
    left.as_os_str().to_string_lossy().eq_ignore_ascii_case(&right.as_os_str().to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::{
        NativeFileAccess, extended_length_path, final_path_within, inspect_existing_path,
        open_verified_executable, open_verified_existing, windows_ordinal_compare_ignore_case,
    };
    use std::path::{Path, PathBuf};

    #[test]
    fn extended_length_paths_cover_drive_unc_device_and_prefixed_forms() {
        assert_eq!(
            extended_length_path(Path::new(r"C:\Aura\store\packages\a\b\manifest.json")),
            PathBuf::from(r"\\?\C:\Aura\store\packages\a\b\manifest.json")
        );
        assert_eq!(
            extended_length_path(Path::new(r"\\server\share\payload")),
            PathBuf::from(r"\\?\UNC\server\share\payload")
        );
        // Device paths and already-extended paths are handed through untouched: the
        // null-device work depends on `\\.\NUL` staying exactly that.
        assert_eq!(extended_length_path(Path::new(r"\\.\NUL")), PathBuf::from(r"\\.\NUL"));
        assert_eq!(
            extended_length_path(Path::new(r"\\?\C:\already\extended")),
            PathBuf::from(r"\\?\C:\already\extended")
        );
        assert_eq!(
            extended_length_path(Path::new(r"relative\path")),
            PathBuf::from(r"relative\path")
        );
        // A verbatim path is taken literally, so forward slashes have to be rewritten
        // before the prefix goes on.
        assert_eq!(
            extended_length_path(Path::new(r"C:\Windows/System32/cmd.exe")),
            PathBuf::from(r"\\?\C:\Windows\System32\cmd.exe")
        );
        assert_eq!(
            extended_length_path(Path::new(r"\\server/share/payload")),
            PathBuf::from(r"\\?\UNC\server\share\payload")
        );
        // `.`/`..` are left alone: a verbatim path would keep them literally, and
        // resolving them textually could cross a reparse point.
        assert_eq!(
            extended_length_path(Path::new(r"C:\Aura\..\target\pwsh.exe")),
            PathBuf::from(r"C:\Aura\..\target\pwsh.exe")
        );
        assert_eq!(
            extended_length_path(Path::new(r"C:\Aura\.\pwsh.exe")),
            PathBuf::from(r"C:\Aura\.\pwsh.exe")
        );
    }

    #[test]
    fn a_verified_open_reaches_past_the_max_path_limit() {
        // The immutable Package Store nests `<root>/packages/<id>/<version>/<64 hex>`
        // and the manifest name, which a namespaced data root pushes past `MAX_PATH`.
        // Enumeration of the parent still works at that length, so only the open
        // failed before the extended-length form was used.
        let shallow = tempfile::tempdir().expect("temp root");
        let deep = shallow
            .path()
            .join("aura-electron-e2e-fixture")
            .join("Aura")
            .join("namespaces")
            .join("961d23db09870b4258ab1f48d367ca5d")
            .join("data")
            .join("tool-package-store")
            .join("packages")
            .join("tpk_01M31W36FPWKB0SV6Z5A4RJARP")
            .join("1.0.0")
            .join("748d3af12940a0f1642dafce2d4489a7714f771c72761c16e9ff77c30af85c8c");
        std::fs::create_dir_all(extended_length_path(&deep)).expect("deep payload root");
        let manifest = deep.join("aura-tool-package.json");
        assert!(manifest.as_os_str().len() > 260, "fixture path is too short: {manifest:?}");
        std::fs::write(extended_length_path(&manifest), b"{}").expect("write manifest");
        // Plain enumeration succeeds at this length; the verified open is what needs
        // the extended form.
        assert_eq!(std::fs::read_dir(&deep).expect("enumerate payload root").count(), 1);
        let opened = super::open_verified_import_source(&manifest, NativeFileAccess::Read)
            .expect("verified open past MAX_PATH");
        assert!(!opened.info().is_directory);
        assert_eq!(
            opened.file().metadata().expect("metadata").len(),
            2,
            "the opened handle must be the manifest written above"
        );
    }

    #[test]
    fn execution_resource_lock_excludes_another_core_until_release() {
        let directory = tempfile::tempdir().expect("temp root");
        let path = directory.path().join("execution.lock");
        std::fs::write(&path, b"execution-resources-v1").expect("installer fixture");
        let first = super::lock_execution_resources(&path).expect("first Core");
        assert!(
            matches!(super::lock_execution_resources(&path), Err(e) if e.raw_os_error() == Some(32))
        );
        assert!(std::fs::remove_file(&path).is_err());
        drop(first);
        let _second = super::lock_execution_resources(&path).expect("next Core");
    }

    #[test]
    fn final_handle_path_and_identity_are_stable() {
        let directory = tempfile::tempdir().expect("temp root");
        let file_path = directory.path().join("sample.txt");
        std::fs::write(&file_path, b"Aura").expect("seed file");
        let root = inspect_existing_path(directory.path(), true).expect("root info");
        let file = open_verified_existing(&file_path, NativeFileAccess::Read).expect("file info");
        assert!(final_path_within(&root, file.info()));
        assert_eq!(file.file().metadata().expect("metadata").len(), 4);
    }

    #[test]
    fn executable_verification_handle_denies_replacement_until_dropped() {
        let directory = tempfile::tempdir().expect("temp root");
        let executable = directory.path().join("tool.exe");
        std::fs::write(&executable, b"Aura executable fixture").expect("seed executable");

        let verified = open_verified_executable(&executable).expect("verified executable");
        let replacement = directory.path().join("replacement.exe");
        std::fs::write(&replacement, b"replacement").expect("seed replacement");
        assert!(std::fs::remove_file(&executable).is_err());

        drop(verified);
        std::fs::remove_file(&executable).expect("delete after verification closes");
        std::fs::rename(&replacement, &executable).expect("replace after verification closes");
    }

    #[test]
    fn ordinal_case_comparison_uses_windows_uppercase_tables() {
        assert_eq!(
            windows_ordinal_compare_ignore_case("Tools/Ä.exe".as_ref(), "tools/ä.EXE".as_ref())
                .expect("compare paths"),
            std::cmp::Ordering::Equal
        );
    }

    /// The `query_info` failure path keeps exactly one close owner.
    ///
    /// The defect: the old `verified_from_file` built an `OwnedHandle` with
    /// `borrow_raw`, ran `query_info(&handle)?`, and called `mem::forget(handle)`
    /// only on the success path. A failed query therefore skipped the forget and
    /// dropped that wrapper while the `File` it was borrowed from still owned the
    /// same value — two close owners for one handle. The borrow is now a distinct
    /// non-owning type (`BorrowedHandle`) with no `Drop` and no `into_raw`, so
    /// there is no forget to skip and the `File` is the only close owner on both
    /// paths.
    ///
    /// WHAT THIS TEST DOES AND DOES NOT PROVE. It drives the failure
    /// deterministically on a handle that stays valid: an anonymous pipe is a
    /// real handle with no file identity, so `GetFileInformationByHandleEx`
    /// fails on it with `ERROR_INVALID_PARAMETER` (87) while the handle is still
    /// open. The test then asserts the handle value is *not* left dangling by
    /// opening fresh handles and requiring them to work.
    ///
    /// That assertion passes on the pre-fix form as well, and the reuse
    /// observation says why: the two closes the old code performed happen
    /// back-to-back inside one frame — the wrapper drops, then the `File` drops —
    /// with no handle allocated in between, so the second `CloseHandle` targets
    /// the already-freed value and is a silent no-op, and the value is simply
    /// released once. A leak would have kept the value out of circulation; a
    /// close of a *different* live handle would have broken the witness open.
    /// Neither happens. So this test is a regression guard on close
    /// responsibility and on the failure path being reachable, **not** a
    /// demonstration that the old code corrupted a handle in this environment.
    /// The old code is still wrong: it created a second close owner for a handle
    /// it did not own, which is undefined behaviour that this environment happens
    /// not to punish.
    #[test]
    fn a_failed_handle_query_keeps_its_single_close_owner() {
        use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _};

        let directory = tempfile::tempdir().expect("temp root");
        let witness_path = directory.path().join("witness.bin");
        std::fs::write(&witness_path, b"witness").expect("seed witness");

        // A valid handle for an object `query_info` cannot describe.
        let (reader, writer) = std::io::pipe().expect("anonymous pipe");
        let raw = reader.as_raw_handle();
        std::mem::forget(reader);
        // SAFETY: ownership of `raw` was taken above and is transferred to
        // exactly one `File`, which is the value the production call receives.
        let queried = unsafe { std::fs::File::from_raw_handle(raw) };

        let result = super::verified_from_file(queried);
        drop(writer);
        let error = match result {
            Ok(_) => {
                panic!(
                    "query_info failure path was not exercised: the pipe handle answered \
                     a file query"
                );
            }
            Err(error) => error,
        };
        assert_eq!(
            error.raw_os_error(),
            Some(87),
            "a pipe has no file identity, so the first query must fail with \
             ERROR_INVALID_PARAMETER"
        );

        // The failing call must not have consumed a handle belonging to anyone
        // else, and must not have leaked the one it was given. Both are visible
        // here: the object the pipe was attached to is gone from this process, so
        // the value is free for reuse, and a fresh open must still work.
        let mut witness = std::fs::File::open(&witness_path).expect("the witness must open");
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut witness, &mut bytes)
            .expect("the witness handle must be live, not a reused closed value");
        assert_eq!(bytes, b"witness", "the witness must still refer to its own object");
    }

    /// The success path keeps exactly one close owner too.
    ///
    /// `verified_from_file` hands the same `File` back inside the result, so the
    /// handle stays owned across the query; dropping the result closes it once.
    /// A second close would have released a handle value the process reuses.
    #[test]
    fn a_successful_handle_query_keeps_one_close_owner() {
        use std::os::windows::io::AsRawHandle;

        let directory = tempfile::tempdir().expect("temp root");
        let path = directory.path().join("query-success.bin");
        std::fs::write(&path, b"fixture").expect("seed");
        let owned = std::fs::File::open(&path).expect("open fixture");
        let raw = owned.as_raw_handle();

        let verified = super::verified_from_file(owned).expect("query succeeds");
        assert_eq!(
            verified.file().as_raw_handle(),
            raw,
            "the returned file must own the very handle that was queried"
        );
        assert_eq!(verified.info().final_path.file_name(), path.file_name());
        assert_eq!(verified.file().metadata().expect("metadata").len(), 7);
        drop(verified);
        std::fs::remove_file(&path).expect("the file must be deletable, so no handle leaked");
        assert!(!path.exists(), "the handle must be closed exactly once, not leaked");
    }

    #[test]
    fn metadata_open_fails_at_createfile_when_read_attributes_denied() {
        let directory = tempfile::tempdir().expect("temp root");
        let path = directory.path().join("denied.bin");
        std::fs::write(&path, b"fixture").expect("seed");
        let path_text = path.to_str().expect("utf8 path");
        let mut deny = std::process::Command::new("icacls.exe");
        deny.args([path_text, "/inheritance:r", "/deny", "*S-1-1-0:(REA,RA)"]);
        let denied = crate::test_spawn::status(&mut deny).expect("icacls deny");
        assert!(denied.success(), "icacls deny read attributes");
        let inspect = match inspect_existing_path(&path, false) {
            Err(error) => error,
            Ok(_) => panic!("inspect must fail"),
        };
        assert_eq!(
            inspect.raw_os_error(),
            Some(5),
            "first CreateFileW FILE_READ_ATTRIBUTES returns ACCESS_DENIED before handle query"
        );
        let open = match super::open_execution_metadata(&path) {
            Err(error) => error,
            Ok(_) => panic!("open must fail"),
        };
        assert_eq!(open.raw_os_error(), Some(5));
        let mut grant = std::process::Command::new("icacls.exe");
        let _ = crate::test_spawn::status(grant.args([path_text, "/grant", "*S-1-1-0:F"]));
    }
}
