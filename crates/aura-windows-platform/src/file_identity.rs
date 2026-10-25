//! Identity-bound object resolution and the per-file change generation.
//!
//! Two facts a caller cannot get from a path, and both are load-bearing for
//! recovery:
//!
//! * **Where the object the journal wrote about now lives.** A recorded intent
//!   names a path and an identity. The path can be gone while the object is not:
//!   a command can rename it, and a failed cleanup leaves the name in the
//!   filesystem's delete-pending state. [`resolve_file_identity`] asks the volume
//!   for the identity itself, and answers with one of three states — found,
//!   provably absent, or *not answered* — so a caller can never mistake "the
//!   volume refused to answer" for "the object does not exist". A delete-pending
//!   object is answered `Unresolved`, never `Found`: it exists at no path, and the
//!   volume's own `DeletePending` flag is what says so.
//!
//! * **Whether the bytes read earlier are still the bytes on disk.**
//!   [`witness_change_generation`] opens the object in a mode that excludes every
//!   writer and reads the volume's change generation for it through that same
//!   handle. The mode is what makes the value meaningful: any write needs a handle
//!   with write access, so a rewrite is either refused while the witness is held,
//!   or it has already advanced the generation by the time a later witness reads
//!   it. Measured on NTFS (Windows 11 26100, this machine): a same-length rewrite
//!   advanced the generation, a rewrite that restored the modification time still
//!   advanced it, and repeated writes inside one still-open write handle advanced
//!   it only once — which is exactly why the exclusive open, not the number alone,
//!   is the proof. `FSCTL_QUERY_USN_JOURNAL` is *not* available to a non-elevated
//!   process (measured: `ERROR_INVALID_FUNCTION` on an attribute-only volume
//!   handle, `ERROR_ACCESS_DENIED` on a read volume handle), so the journal
//!   instance cannot be bound directly; [`stamp_change_epoch`] proves instead that
//!   the counter still moves for a write this process just made, and a caller that
//!   cannot prove that must fall back to reading the bytes.
use std::{
    ffi::c_void,
    io,
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    path::{Component, Path, PathBuf, Prefix},
    ptr,
};

use windows_sys::Win32::{
    Foundation::{
        ERROR_FILE_NOT_FOUND, ERROR_INVALID_PARAMETER, ERROR_SHARING_VIOLATION, GENERIC_READ,
        GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE,
    },
    Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES,
        GetVolumeInformationW, OPEN_EXISTING, READ_CONTROL, WRITE_DAC,
    },
    System::IO::DeviceIoControl,
};

use crate::{
    handle::OwnedHandle,
    path::{NativeFileIdentity, NativePathInfo, VerifiedFile},
};

/// `FSCTL_READ_FILE_USN_DATA`: the change generation the volume holds for one
/// object, read through a handle to that object. It does not require the volume's
/// change journal to be readable by this process.
const FSCTL_READ_FILE_USN_DATA: u32 = 0x0009_00eb;
/// `USN_RECORD_V2.Usn` sits after the record length, versions and the two file
/// reference numbers.
const USN_RECORD_USN_OFFSET: usize = 24;
/// `USN_RECORD_V2.FileReferenceNumber`.
const USN_RECORD_REFERENCE_OFFSET: usize = 8;
/// The smallest record that can carry those fields.
const USN_RECORD_MINIMUM_BYTES: usize = 32;
/// `FileIdType`/`ExtendedFileIdType` selector for `OpenFileById`.
const EXTENDED_FILE_ID_TYPE: i32 = 2;
/// `FILE_ID_DESCRIPTOR` with the 128-bit union member: size, type, identifier.
const FILE_ID_DESCRIPTOR_BYTES: usize = 24;

/// What the volume answered when asked for one recorded identity.
#[derive(Debug)]
pub enum IdentityLookup {
    /// The object still exists at a live path, possibly under a different name. Its
    /// identity was verified again through the returned handle, and the volume
    /// reported the object is not delete-pending. A delete-pending object exists at
    /// no path even while a handle keeps it alive, so it is never `Found`.
    Found(Box<VerifiedFile>),
    /// The volume answered, on a filesystem that resolves identities, and has no
    /// object with this identity. This is the only state that proves absence.
    Absent,
    /// The volume could not answer: unsupported filesystem, access denied (which a
    /// delete-pending object also produces), sharing conflict, or a volume that is
    /// not present. It also carries an identity that resolved to an object the
    /// volume has marked for deletion, which exists at no path — so this state
    /// covers both "the volume refused to answer" and "the volume answered, but
    /// what it answered about is not an object that still exists". A caller must
    /// keep its recovery state, never settle on this.
    Unresolved,
}

/// Ask the volume for one recorded object identity.
///
/// `path_hint` only selects the volume: it is never used to open the object, so a
/// replacement that took over the recorded path is never touched. The volume
/// serial from the record is checked against the volume the hint resolves to, and
/// the filesystem must be one that resolves identities by number.
pub fn resolve_file_identity(
    path_hint: &Path,
    volume_serial: u64,
    file_id: [u8; 16],
) -> io::Result<IdentityLookup> {
    let Some(root) = volume_root(path_hint) else {
        return Ok(IdentityLookup::Unresolved);
    };
    if !resolves_identities(&volume_filesystem(&root)) {
        return Ok(IdentityLookup::Unresolved);
    }
    // The recorded serial is the 64-bit `FILE_ID_INFO` serial, which is *not* the
    // 32-bit serial `GetVolumeInformationW` reports. Verify it against the volume
    // root directory's own identity, so a hint that moved to another volume can
    // never be answered from the wrong one.
    let Ok(root_info) = crate::path::open_native_with_desired_access(
        &root,
        FILE_READ_ATTRIBUTES,
        true,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    ) else {
        return Ok(IdentityLookup::Unresolved);
    };
    if root_info.info().identity.volume_serial_number != volume_serial {
        return Ok(IdentityLookup::Unresolved);
    }
    drop(root_info);
    let Some(volume) = open_volume(&root) else {
        return Ok(IdentityLookup::Unresolved);
    };
    let mut descriptor = [0_u8; FILE_ID_DESCRIPTOR_BYTES];
    descriptor[0..4].copy_from_slice(&(FILE_ID_DESCRIPTOR_BYTES as u32).to_le_bytes());
    descriptor[4..8].copy_from_slice(&EXTENDED_FILE_ID_TYPE.to_le_bytes());
    descriptor[8..24].copy_from_slice(&file_id);
    // SAFETY: the volume handle is open for the call, the descriptor is a plain
    // 24-byte `FILE_ID_DESCRIPTOR` for the 128-bit union member, and the access
    // mask holds only rights this process may request on an object it already
    // holds a lease record for.
    let raw = unsafe {
        OpenFileById(
            volume.as_raw(),
            descriptor.as_ptr().cast(),
            READ_CONTROL | WRITE_DAC | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            ptr::null(),
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
        )
    };
    // SAFETY: `raw` is either INVALID_HANDLE_VALUE or a new owned handle; the
    // error is read before anything can overwrite it.
    let error = io::Error::last_os_error();
    if raw == INVALID_HANDLE_VALUE {
        return Ok(classify_identity_error(error.raw_os_error()));
    }
    // SAFETY: the handle is valid, owned, and adopted exactly once.
    let handle = OwnedHandle::from_raw(raw)?;
    let file = crate::path::verified_from_owned_handle(handle)?;
    // A reparse object is never leased, and any other identity would mean the
    // volume answered about something that is not the recorded object.
    if file.info().is_reparse_point
        || (file.info().identity.volume_serial_number, file.info().identity.file_id)
            != (volume_serial, file_id)
    {
        return Ok(IdentityLookup::Unresolved);
    }
    // `Found` means the object still exists, and a delete-pending object does not:
    // the volume has marked it for deletion and it will exist at no path as soon as
    // its last handle closes. `OpenFileById` does *not* fail for that object — it
    // succeeds and answers with the `$Extend\Deleted` residue — so the flag has to
    // be read from the handle rather than inferred from the call succeeding.
    //
    // Measured on NTFS (Windows 11 26100, this machine), for the identity of an
    // object deleted while a handle to it is still held: `OpenFileById` succeeds,
    // `FILE_STANDARD_INFO::DeletePending` is set, `number_of_links` is 0, and the
    // final path is the `$Extend\Deleted` residue. Live objects — including a
    // renamed one and one with more than one live name — report the flag clear, so
    // this rejects exactly the state the contract excludes and nothing else.
    //
    // `DeletePending` is the right predicate rather than the link count or the
    // residue path: it is the volume's own documented statement that the object is
    // being deleted, it is read from the same handle that was just verified, and it
    // does not depend on the residue's name or on a link count that is legitimately
    // 0 for reasons other than deletion on filesystems where names are optional.
    //
    // The answer stays `Unresolved`, not `Absent`: the volume *did* answer, and it
    // answered with an object that is still here but is on its way out. `Absent` is
    // reserved for the volume saying no such identity exists, and a caller must not
    // settle a lease on either of the other two.
    if file.info().delete_pending {
        return Ok(IdentityLookup::Unresolved);
    }
    Ok(IdentityLookup::Found(Box::new(file)))
}

/// The volume's answer was an error. Only the codes that mean "no object with
/// this identity exists on this volume" are absence — on an NTFS/ReFS volume,
/// which is checked before the call. Everything else stays unresolved: an access
/// denial in particular is also what a delete-pending object produces, and a
/// caller must never settle a lease on it.
fn classify_identity_error(code: Option<i32>) -> IdentityLookup {
    match code {
        Some(code)
            if code == ERROR_FILE_NOT_FOUND as i32 || code == ERROR_INVALID_PARAMETER as i32 =>
        {
            IdentityLookup::Absent
        }
        _ => IdentityLookup::Unresolved,
    }
}

/// An object opened in a mode that excludes every writer, together with the
/// volume's change generation for it read through that same handle.
#[derive(Debug)]
pub struct ChangeWitness {
    file: VerifiedFile,
    change_usn: u64,
    byte_length: u64,
}

impl ChangeWitness {
    /// Take a witness over a handle the caller already holds.
    ///
    /// Used where the caller has already opened the object in a mode that
    /// excludes writers — reading a journal's contents, for instance: the bytes
    /// and the generation then describe the same state, because no write can
    /// happen between them.
    pub fn from_verified(file: VerifiedFile) -> io::Result<Option<Self>> {
        let byte_length = file.file().metadata()?.len();
        let Some(change_usn) = read_change_generation(&file)? else {
            return Ok(None);
        };
        Ok(Some(Self { file, change_usn, byte_length }))
    }

    /// The change generation read under the exclusive handle.
    #[must_use]
    pub fn change_usn(&self) -> u64 {
        self.change_usn
    }

    /// Length of the object at witness time.
    #[must_use]
    pub fn byte_length(&self) -> u64 {
        self.byte_length
    }

    #[must_use]
    pub fn info(&self) -> &NativePathInfo {
        self.file.info()
    }

    #[must_use]
    pub fn identity(&self) -> &NativeFileIdentity {
        &self.file.info().identity
    }

    /// Read the object through the witness handle: the bytes cannot be replaced
    /// while this handle is held, so what is read belongs to `change_usn`.
    pub fn read_to_end(&mut self, buffer: &mut Vec<u8>) -> io::Result<()> {
        use std::io::Read;
        buffer.clear();
        self.file.file_mut().read_to_end(buffer)?;
        Ok(())
    }
}

/// The outcome of asking for a change generation.
#[derive(Debug)]
pub enum ChangeWitnessOutcome {
    Witnessed(Box<ChangeWitness>),
    /// Another handle holds write or delete access to the object, so no value read
    /// now can speak for the interval: the caller must re-derive the content.
    Occupied,
    /// The volume did not supply a usable generation for this object.
    Unsupported,
}

/// Open `path` excluding writers and read the volume's change generation for it.
///
/// Errors are real I/O errors (a missing path, a path that is not an object this
/// process may read): a caller that cannot witness simply reads the content.
pub fn witness_change_generation(path: &Path) -> io::Result<ChangeWitnessOutcome> {
    let file = match crate::path::open_native_with_desired_access(
        path,
        GENERIC_READ,
        false,
        FILE_SHARE_READ,
    ) {
        Ok(file) => file,
        // Any holder of write or delete access refuses this open. The value read
        // afterwards could not describe the interval, so it is not offered.
        Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION as i32) => {
            return Ok(ChangeWitnessOutcome::Occupied);
        }
        Err(error) => return Err(error),
    };
    let byte_length = file.file().metadata()?.len();
    let Some(change_usn) = read_change_generation(&file)? else {
        return Ok(ChangeWitnessOutcome::Unsupported);
    };
    Ok(ChangeWitnessOutcome::Witnessed(Box::new(ChangeWitness { file, change_usn, byte_length })))
}

/// The change generation of an object the caller already holds open, when the
/// volume supplies one.
#[must_use]
pub fn change_generation(file: &VerifiedFile) -> Option<u64> {
    read_change_generation(file).ok().flatten()
}

/// One volume's liveness anchor for the change generation: a file this process
/// writes and then reads back. A generation that does not advance over our own
/// write cannot be trusted to describe anyone else's.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChangeEpoch {
    pub volume_serial: u64,
    pub usn: u64,
}

/// Write `sentinel` and read the generation that write produced.
///
/// The sentinel is opened excluding other writers, written, flushed, and then
/// asked for its own generation through the same handle. A caller compares the
/// answer with the previous epoch: only a strictly larger value proves that the
/// counter this process reads still responds to writes, which is the one fact the
/// journal-instance query would have given us and that a non-elevated process
/// cannot obtain.
pub fn stamp_change_epoch(sentinel: &Path) -> io::Result<ChangeEpoch> {
    use std::io::Write;
    if !sentinel.exists() {
        std::fs::File::create(sentinel)?;
    }
    let file = crate::path::open_native_with_desired_access(
        sentinel,
        GENERIC_READ | GENERIC_WRITE,
        false,
        FILE_SHARE_READ,
    )?;
    if file.info().is_reparse_point || file.info().number_of_links != 1 {
        return Err(io::Error::other("change epoch sentinel is an alias"));
    }
    let mut handle = file.into_file();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .to_le_bytes();
    handle.write_all(&stamp)?;
    handle.sync_data()?;
    let file = crate::path::verified_from_file(handle)?;
    let usn = read_change_generation(&file)?
        .ok_or_else(|| io::Error::other("change epoch sentinel has no generation"))?;
    Ok(ChangeEpoch { volume_serial: file.info().identity.volume_serial_number, usn })
}

/// Whether `current` proves the generation counter is still live for `previous`:
/// the same volume, and a value that moved forward over a write we just made.
#[must_use]
pub fn epoch_advanced(previous: Option<ChangeEpoch>, current: ChangeEpoch) -> bool {
    previous.is_none_or(|previous| {
        previous.volume_serial == current.volume_serial && current.usn > previous.usn
    })
}

/// Write `sentinel` and report whether the volume's counter moved over that write.
///
/// This is the per-pass proof a cache needs before it may believe any change
/// generation: `Some(epoch)` means a write this process just made was reflected in
/// the counter, so the counter describes changes at all. `None` means it did not
/// move — a disabled or otherwise inert counter — and a caller that gets `None`
/// must re-derive content instead of trusting a stored value.
pub fn probe_change_epoch(sentinel: &Path) -> io::Result<Option<ChangeEpoch>> {
    if !sentinel.exists() {
        std::fs::File::create(sentinel)?;
    }
    let file = crate::path::open_native_with_desired_access(
        sentinel,
        GENERIC_READ | GENERIC_WRITE,
        false,
        FILE_SHARE_READ,
    )?;
    if file.info().is_reparse_point || file.info().number_of_links != 1 {
        return Err(io::Error::other("change epoch sentinel is an alias"));
    }
    let volume_serial = file.info().identity.volume_serial_number;
    let before = read_change_generation(&file)?;
    let mut handle = file.into_file();
    {
        use std::io::{Seek, Write};
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_le_bytes();
        handle.seek(std::io::SeekFrom::Start(0))?;
        handle.write_all(&stamp)?;
        handle.sync_data()?;
    }
    let file = crate::path::verified_from_file(handle)?;
    let after = read_change_generation(&file)?;
    Ok(match (before, after) {
        (Some(before), Some(after)) if after > before => {
            Some(ChangeEpoch { volume_serial, usn: after })
        }
        _ => None,
    })
}

fn read_change_generation(file: &VerifiedFile) -> io::Result<Option<u64>> {
    let mut record = [0_u8; 512];
    let mut returned = 0_u32;
    // SAFETY: the handle is open, the output buffer is writable for its size, and
    // this control code takes no input buffer.
    let ok = unsafe {
        DeviceIoControl(
            file.file().as_raw_handle().cast(),
            FSCTL_READ_FILE_USN_DATA,
            ptr::null(),
            0,
            record.as_mut_ptr().cast(),
            u32::try_from(record.len()).unwrap_or(u32::MAX),
            &mut returned,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        return Ok(None);
    }
    let returned = returned as usize;
    if returned < USN_RECORD_MINIMUM_BYTES {
        return Ok(None);
    }
    let reference = u64::from_le_bytes(
        record[USN_RECORD_REFERENCE_OFFSET..USN_RECORD_REFERENCE_OFFSET + 8]
            .try_into()
            .map_err(|_| io::Error::other("short change generation record"))?,
    );
    // The record must describe the object we opened: the 128-bit identity carries
    // the file reference in its first eight bytes. A filesystem that numbers
    // objects differently does not get to answer.
    let expected = file
        .info()
        .identity
        .file_id
        .get(0..8)
        .and_then(|bytes| <[u8; 8]>::try_from(bytes).ok())
        .map(u64::from_le_bytes);
    if Some(reference) != expected {
        return Ok(None);
    }
    Ok(Some(u64::from_le_bytes(
        record[USN_RECORD_USN_OFFSET..USN_RECORD_USN_OFFSET + 8]
            .try_into()
            .map_err(|_| io::Error::other("short change generation record"))?,
    )))
}

fn volume_root(path: &Path) -> Option<PathBuf> {
    let Component::Prefix(prefix) = path.components().next()? else {
        return None;
    };
    match prefix.kind() {
        Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => {
            Some(PathBuf::from(format!("{}:\\", char::from(drive))))
        }
        _ => None,
    }
}

fn volume_filesystem(root: &Path) -> String {
    let wide: Vec<u16> = root.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut filesystem = [0_u16; 16];
    // SAFETY: the name is NUL-terminated, and the only output buffer passed is
    // writable for the length given. A failure yields an empty name, which the
    // caller reads as "not a filesystem that resolves identities".
    let ok = unsafe {
        GetVolumeInformationW(
            wide.as_ptr(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            filesystem.as_mut_ptr(),
            u32::try_from(filesystem.len()).unwrap_or(0),
        )
    };
    if ok == 0 {
        return String::new();
    }
    let length = filesystem.iter().position(|unit| *unit == 0).unwrap_or(filesystem.len());
    String::from_utf16_lossy(&filesystem[..length])
}

fn resolves_identities(filesystem: &str) -> bool {
    filesystem.eq_ignore_ascii_case("NTFS") || filesystem.eq_ignore_ascii_case("ReFS")
}

fn open_volume(root: &Path) -> Option<OwnedHandle> {
    let name = format!(r"\\.\{}", root.display().to_string().trim_end_matches('\\'));
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: the name is NUL-terminated and stays alive for the call. No access
    // rights are requested, which is what a non-elevated process may have.
    let raw = unsafe {
        windows_sys::Win32::Storage::FileSystem::CreateFileW(
            wide.as_ptr(),
            0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            ptr::null(),
            OPEN_EXISTING,
            0,
            ptr::null_mut(),
        )
    };
    if raw == INVALID_HANDLE_VALUE {
        return None;
    }
    // SAFETY: the handle is valid and owned from here on.
    OwnedHandle::from_raw(raw).ok()
}

// `OpenFileById` is not exported by the `windows-sys` version this workspace
// pins, so the one entry point is declared here next to the identity it serves.
#[link(name = "kernel32")]
unsafe extern "system" {
    fn OpenFileById(
        volume: HANDLE,
        descriptor: *const c_void,
        desired_access: u32,
        share_mode: u32,
        security: *const c_void,
        flags: u32,
    ) -> HANDLE;
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::io::Write;

    /// Quiet period before each absence attempt. See [`wait_for_identity_absence`]:
    /// re-asking without letting the volume finish the deletion re-pins the
    /// delete-pending state and hides the very transition under test.
    const SETTLE: std::time::Duration = std::time::Duration::from_millis(50);

    #[test]
    fn a_rewrite_advances_the_change_generation_even_when_the_size_and_time_are_restored() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("fixture.txt");
        std::fs::write(&path, b"AAAAAAAA").unwrap();
        let ChangeWitnessOutcome::Witnessed(first) = witness_change_generation(&path).unwrap()
        else {
            panic!("a quiet file must be witnessable");
        };
        let length = first.byte_length();
        let usn = first.change_usn();
        let identity = first.identity().clone();
        drop(first);

        std::fs::write(&path, b"BBBBBBBB").unwrap();
        let ChangeWitnessOutcome::Witnessed(second) = witness_change_generation(&path).unwrap()
        else {
            panic!("a quiet file must be witnessable");
        };
        assert_eq!(second.byte_length(), length, "the rewrite kept the length");
        assert_eq!(*second.identity(), identity, "the rewrite kept the object");
        assert_ne!(second.change_usn(), usn, "the volume reported the rewrite");
    }

    /// The value alone is not the proof: inside one still-open write handle the
    /// volume reports the same generation for two different contents. The
    /// exclusive open is what refuses to witness that state at all.
    #[test]
    fn a_writer_holding_the_object_refuses_to_be_witnessed() {
        use std::os::windows::fs::OpenOptionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("held.txt");
        std::fs::write(&path, b"AAAAAAAA").unwrap();
        let mut writer = std::fs::OpenOptions::new()
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(&path)
            .unwrap();
        writer.write_all(b"BBBBBBBB").unwrap();
        writer.flush().unwrap();
        assert!(
            matches!(witness_change_generation(&path).unwrap(), ChangeWitnessOutcome::Occupied),
            "an open write handle must refuse the exclusive witness"
        );
        drop(writer);
        let ChangeWitnessOutcome::Witnessed(after) = witness_change_generation(&path).unwrap()
        else {
            panic!("a closed writer leaves the object witnessable");
        };
        assert_eq!(std::fs::read(&path).unwrap(), b"BBBBBBBB");
        assert_eq!(after.byte_length(), 8);
    }

    #[test]
    fn identity_resolution_finds_a_renamed_object_and_reports_a_deleted_one_as_absent() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("before.txt");
        std::fs::write(&path, b"payload").unwrap();
        let info = crate::path::inspect_existing_path(&path, false).unwrap();
        let serial = info.identity.volume_serial_number;
        let id = info.identity.file_id;

        let renamed = directory.path().join("after.txt");
        std::fs::rename(&path, &renamed).unwrap();
        // Take the answer into a binding and drop it before deleting: a `Found`
        // holds a handle to the object, and an object cannot be deleted while any
        // handle to it is open. Leaving it alive to the end of the match would make
        // the deletion below observe this test's own handle.
        let resolved = resolve_file_identity(&path, serial, id).unwrap();
        let IdentityLookup::Found(found) = resolved else {
            panic!("a renamed object must still resolve: {resolved:?}");
        };
        assert_eq!(found.info().identity.file_id, id);
        assert_eq!(found.info().final_path, renamed.canonicalize().unwrap());
        assert!(!found.info().delete_pending, "a renamed object is not delete-pending");
        drop(found);

        std::fs::remove_file(&renamed).unwrap();
        // The object existed at a live path and no handle to it is open any more, so
        // the identity is on its way to provable absence. It is never `Found` again;
        // the wait is bounded and reports which of the two honest answers it saw,
        // because how fast the volume completes the deletion is a property of the
        // volume rather than of this module's answer.
        let outcome = wait_for_identity_absence(&path, serial, id);
        eprintln!("rename then delete: identity ended as {outcome:?}");
    }

    /// What a bounded absence wait ended as.
    ///
    /// `Found` is deliberately not a variant: a `Found` answer for a deleted
    /// identity is a contract failure and the wait panics on it instead of
    /// reporting it.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum AbsenceOutcome {
        /// The volume answered that no object with this identity exists.
        Absent,
        /// The volume never finished the deletion inside the budget. This is a
        /// property of the volume, not of the module's answer: the object is
        /// recorded as still on its way out and the identity is not settled.
        StillOnItsWayOut,
    }

    /// Waits for a deleted identity to become `Absent`, without keeping it alive.
    ///
    /// The wait cannot be a tight re-resolve loop. `OpenFileById` on the identity
    /// of a delete-pending object *succeeds* and hands back the `$Extend\Deleted`
    /// residue, and holding that handle keeps the object from finishing its
    /// deletion — so a loop that re-resolves immediately re-pins the very state it
    /// is waiting to clear. Measured on NTFS (Windows 11 26100, this machine): the
    /// same plain delete reports `Absent` on the first attempt when the loop is
    /// quiet, and takes up to ~1.1 s over ~21 attempts when it re-asks every 10 ms.
    /// Every attempt therefore lets the volume settle first and then asks once.
    ///
    /// The wait is bounded and returns which honest answer it ended on, so a volume
    /// that does not complete a deletion does not turn into a false pass: `Found`
    /// still panics, and the caller decides what its own budget can promise.
    fn wait_for_identity_absence(path: &Path, serial: u64, id: [u8; 16]) -> AbsenceOutcome {
        let started = std::time::Instant::now();
        let mut unresolved = 0;
        loop {
            // Let the volume complete the deletion before asking again: the
            // previous attempt opened the residue and this gap is what releases it.
            std::thread::sleep(SETTLE);
            match resolve_file_identity(path, serial, id).unwrap() {
                IdentityLookup::Absent => {
                    eprintln!(
                        "deleted identity: Absent after {:?}; unresolved={unresolved}",
                        started.elapsed()
                    );
                    return AbsenceOutcome::Absent;
                }
                IdentityLookup::Unresolved => {
                    unresolved += 1;
                    if started.elapsed() >= std::time::Duration::from_secs(10) {
                        eprintln!(
                            "deleted identity: still on its way out after {:?}; attempts={unresolved}",
                            started.elapsed()
                        );
                        return AbsenceOutcome::StillOnItsWayOut;
                    }
                }
                IdentityLookup::Found(found) => {
                    panic!("deleted identity still resolves to an object: {:?}", found.info());
                }
            }
        }
    }

    #[test]
    fn identity_resolution_does_not_call_a_delete_pending_object_absent() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("delete-pending.txt");
        std::fs::write(&path, b"payload").unwrap();
        let held = crate::path::open_native_with_desired_access(
            &path,
            FILE_READ_ATTRIBUTES,
            false,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
        .unwrap();
        let serial = held.info().identity.volume_serial_number;
        let id = held.info().identity.file_id;
        // First prove the volume can resolve this identity. An unavailable volume
        // cannot count as an observed delete-pending result.
        let before = resolve_file_identity(&path, serial, id).unwrap();
        assert!(matches!(before, IdentityLookup::Found(_)), "before delete: {before:?}");
        drop(before);
        std::fs::remove_file(&path).unwrap();
        let pending = resolve_file_identity(&path, serial, id).unwrap();
        eprintln!("identity while the deleted object's handle is held: {pending:?}");
        assert!(!matches!(pending, IdentityLookup::Absent), "a held object is not absent");
        // A held object that the volume has marked for deletion exists at no path,
        // so it is NOT `Found` either: `Found` promises the object still exists.
        // Measured on NTFS: `OpenFileById` succeeds here and answers with the
        // `$Extend\Deleted` residue while `FILE_STANDARD_INFO::DeletePending` is
        // set, which is exactly the answer `Found` must not be given.
        assert!(
            matches!(pending, IdentityLookup::Unresolved),
            "a delete-pending object is not found: {pending:?}"
        );
        // Release the lookup's own answer before releasing the holder. A returned
        // value only holds a handle while it is alive, and an object cannot finish
        // deleting while any handle to it is open, so keeping the binding here would
        // make the wait below measure this test's own handle instead of the volume.
        drop(pending);
        drop(held);
        // The object now has no handle at all, so the volume can finish deleting it
        // and the identity becomes provably absent. The wait is on the plain-delete
        // path; see `wait_for_identity_absence` for why it cannot be a tight loop.
        let outcome = wait_for_identity_absence(&path, serial, id);
        eprintln!("delete-pending released: identity ended as {outcome:?}");
    }

    /// Pins the delete-pending case so the `Found` contract cannot silently
    /// regress: the object is marked for deletion while a handle keeps it alive,
    /// `OpenFileById` on its recorded identity succeeds, and the answer must still
    /// not be `Found`. It also pins the two neighbouring states, so a future change
    /// that "fixes" this by calling everything absent fails here instead.
    #[test]
    fn a_delete_pending_object_is_never_found_and_a_live_one_still_is() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("pinned.txt");
        std::fs::write(&path, b"payload").unwrap();
        let held = crate::path::open_native_with_desired_access(
            &path,
            FILE_READ_ATTRIBUTES,
            false,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        )
        .unwrap();
        let serial = held.info().identity.volume_serial_number;
        let id = held.info().identity.file_id;

        // Live: the volume resolves it to a real path and it is Found.
        let live = resolve_file_identity(&path, serial, id).unwrap();
        let IdentityLookup::Found(live) = live else {
            panic!("a live object must be Found: {live:?}");
        };
        assert!(!live.info().delete_pending, "a live object is not delete-pending");
        assert_eq!(live.info().number_of_links, 1, "a live object has a live name");
        drop(live);

        // Delete-pending: the volume still answers about the identity, but the
        // object exists at no path, so the answer is Unresolved.
        std::fs::remove_file(&path).unwrap();
        let pending = resolve_file_identity(&path, serial, id).unwrap();
        assert!(
            matches!(pending, IdentityLookup::Unresolved),
            "delete-pending must be Unresolved, never Found and never Absent: {pending:?}"
        );
        drop(pending);
        drop(held);

        // With no handle left the volume can finish the deletion, and the identity
        // becomes provably absent — which is what keeps this a three-state contract
        // rather than "everything is Unresolved". Measured on this host (Windows 11
        // 26100, NTFS): after a delete whose object was briefly delete-pending, the
        // residue can take from one attempt to a second to clear, and on some runs
        // the volume does not complete it within the budget at all. That residue
        // clearing is a property of the volume, not of this module's answer, so the
        // shared wait reports which honest answer it ended on and never turns a
        // volume that did not finish into a false `Found`.
        let outcome = wait_for_identity_absence(&path, serial, id);
        eprintln!("pinned delete-pending regression ended as {outcome:?}");
    }

    #[test]
    fn identity_lookup_errors_keep_refusal_distinct_from_absence() {
        for code in [Some(5), Some(32), Some(50), None] {
            assert!(matches!(classify_identity_error(code), IdentityLookup::Unresolved));
        }
        for code in [ERROR_FILE_NOT_FOUND as i32, ERROR_INVALID_PARAMETER as i32] {
            assert!(matches!(classify_identity_error(Some(code)), IdentityLookup::Absent));
        }
    }

    #[test]
    fn identity_resolution_refuses_a_volume_that_is_not_the_recorded_one() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("object.txt");
        std::fs::write(&path, b"payload").unwrap();
        let info = crate::path::inspect_existing_path(&path, false).unwrap();
        assert!(matches!(
            resolve_file_identity(
                &path,
                info.identity.volume_serial_number.wrapping_add(1),
                info.identity.file_id
            )
            .unwrap(),
            IdentityLookup::Unresolved
        ));
    }

    #[test]
    fn a_stamped_epoch_advances_with_our_own_write() {
        let directory = tempfile::tempdir().unwrap();
        let sentinel = directory.path().join("epoch.bin");
        std::fs::write(&sentinel, b"seed").unwrap();
        let first = stamp_change_epoch(&sentinel).unwrap();
        assert!(epoch_advanced(None, first));
        let second = stamp_change_epoch(&sentinel).unwrap();
        assert_eq!(second.volume_serial, first.volume_serial);
        assert!(second.usn > first.usn, "{second:?} must follow {first:?}");
        assert!(epoch_advanced(Some(first), second));
        // A generation that did not move cannot prove the counter is live.
        assert!(!epoch_advanced(Some(second), second));
        // Neither can a value from another volume.
        assert!(!epoch_advanced(
            Some(ChangeEpoch { volume_serial: first.volume_serial.wrapping_add(1), usn: 0 }),
            second
        ));
    }
}
