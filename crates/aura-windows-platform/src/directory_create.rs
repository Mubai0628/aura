//! Directory creation returns the object created by the kernel, before any path re-open.
//! The returned handle has no DELETE access: an open handle with DELETE refuses every later open
//! that denies delete sharing, such as a publication parent another thread takes on the new
//! directory while the creator still holds its handle.
use std::{
    ffi::OsStr,
    fs::File,
    io,
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::{Component, Path},
    ptr,
};

use crate::{HandlePublishError, VerifiedFile, handle_bound_delete, path::verified_from_file};
use windows_sys::{
    Wdk::{
        Foundation::OBJECT_ATTRIBUTES,
        Storage::FileSystem::{
            FILE_CREATE, FILE_DIRECTORY_FILE, FILE_OPEN, FILE_OPEN_REPARSE_POINT,
            FILE_SYNCHRONOUS_IO_NONALERT, NtCreateFile,
        },
    },
    Win32::{
        Foundation::{GENERIC_READ, RtlNtStatusToDosError, STATUS_PENDING, UNICODE_STRING},
        Storage::FileSystem::{
            DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
            SYNCHRONIZE,
        },
        System::IO::IO_STATUS_BLOCK,
    },
};

pub fn handle_bound_create_directory(
    parent: &VerifiedFile,
    leaf: &OsStr,
) -> Result<VerifiedFile, HandlePublishError> {
    crate::atomic_file::ensure_parent_unchanged(parent)?;
    let mut components = Path::new(leaf).components();
    if !parent.info().is_directory
        || !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(HandlePublishError::InvalidFileName);
    }
    let mut name = leaf.encode_wide().collect::<Vec<_>>();
    if name.contains(&0) || name.contains(&u16::from(b':')) {
        return Err(HandlePublishError::InvalidFileName);
    }
    // FILE_CREATE refuses any occupant, so the returned handle owns exactly the newly created
    // object.
    let file = native_directory(parent.file(), &mut name, GENERIC_READ | SYNCHRONIZE, FILE_CREATE)?;
    let verified = file.try_clone().and_then(verified_from_file).and_then(|created| {
        if !crate::final_path_within(parent.info(), created.info()) {
            return Err(io::Error::other("created directory escaped its verified parent"));
        }
        Ok(created)
    });
    match verified {
        Ok(verified) => Ok(verified),
        Err(cause) => match delete_created(&file) {
            Ok(()) => Err(cause.into()),
            Err(cleanup) => Err(HandlePublishError::RollbackFailed {
                operation: cause.to_string(),
                rollback: cleanup.to_string(),
            }),
        },
    }
}

/// Removes the directory `receipt` names, wherever it is now. The receipt has no DELETE access, so
/// the object is opened again through the receipt itself, by an empty name relative to it, never
/// by path. (`ReOpenFile` refuses directory handles.)
fn delete_created(receipt: &File) -> Result<(), HandlePublishError> {
    let mut empty = [0u16; 1];
    let deletable = native_directory(receipt, &mut empty[..0], DELETE | SYNCHRONIZE, FILE_OPEN)?;
    handle_bound_delete(&deletable)
}

/// One synchronous native open of the directory `name` relative to `root`; an empty name opens
/// `root`'s own object again. Every sharing is admitted, and a reparse point is opened as itself,
/// never followed.
fn native_directory(
    root: &File,
    name: &mut [u16],
    access: u32,
    disposition: u32,
) -> Result<File, HandlePublishError> {
    let length = u16::try_from(name.len().saturating_mul(2))
        .map_err(|_| HandlePublishError::InvalidFileName)?;
    let mut unicode =
        UNICODE_STRING { Length: length, MaximumLength: length, Buffer: name.as_mut_ptr() };
    let attributes = OBJECT_ATTRIBUTES {
        Length: u32::try_from(size_of::<OBJECT_ATTRIBUTES>())
            .expect("native structure size fits u32"),
        RootDirectory: root.as_raw_handle(),
        ObjectName: &mut unicode,
        Attributes: 0,
        SecurityDescriptor: ptr::null_mut(),
        SecurityQualityOfService: ptr::null_mut(),
    };
    let mut raw = ptr::null_mut();
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: `root` stays owned throughout the synchronous native call. The counted UTF-16 name
    // and structures are live for the call.
    let result = unsafe {
        NtCreateFile(
            &mut raw,
            access,
            &attributes,
            &mut status,
            ptr::null(),
            FILE_ATTRIBUTE_DIRECTORY,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            disposition,
            FILE_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT | FILE_OPEN_REPARSE_POINT,
            ptr::null(),
            0,
        )
    };
    if result == STATUS_PENDING || result < 0 {
        if !raw.is_null() {
            // SAFETY: a returned owned handle must be closed even on an unexpected status.
            drop(unsafe { File::from_raw_handle(raw) });
        }
        // SAFETY: converting the returned NTSTATUS has no pointer arguments.
        let code = unsafe { RtlNtStatusToDosError(result) };
        return Err(io::Error::from_raw_os_error(i32::try_from(code).unwrap_or(i32::MAX)).into());
    }
    if raw.is_null() {
        return Err(io::Error::other("native directory open returned no handle").into());
    }
    // SAFETY: ownership of the unique handle the call returned transfers to File.
    Ok(unsafe { File::from_raw_handle(raw) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NativeFileAccess, open_verified_existing};

    #[test]
    fn creation_receipt_stays_bound_to_the_created_directory_after_rename() {
        let root = tempfile::tempdir().expect("root");
        let parent =
            open_verified_existing(root.path(), NativeFileAccess::Metadata).expect("parent");
        let created =
            handle_bound_create_directory(&parent, OsStr::new("created")).expect("create");
        std::fs::rename(root.path().join("created"), root.path().join("moved")).expect("rename");
        std::fs::create_dir(root.path().join("created")).expect("replacement");
        let moved = open_verified_existing(&root.path().join("moved"), NativeFileAccess::Metadata)
            .expect("moved");
        let replacement =
            open_verified_existing(&root.path().join("created"), NativeFileAccess::Metadata)
                .expect("replacement");
        assert_eq!(created.info().identity, moved.info().identity);
        assert_ne!(created.info().identity, replacement.info().identity);
        drop(moved);
        delete_created(created.file()).expect("delete actual created object");
        drop(created);
        assert!(!root.path().join("moved").exists());
        assert!(root.path().join("created").is_dir());
    }

    #[test]
    fn a_created_directory_can_be_held_as_a_publication_parent_while_its_receipt_is_open() {
        // Another thread may hold the new directory as a publication parent, which denies delete
        // sharing, before the creator has dropped its receipt.
        let root = tempfile::tempdir().expect("root");
        let parent =
            open_verified_existing(root.path(), NativeFileAccess::Metadata).expect("parent");
        let created =
            handle_bound_create_directory(&parent, OsStr::new("created")).expect("create");
        let held = crate::open_verified_publish_parent(&root.path().join("created"))
            .expect("the new directory held as a publication parent");
        assert_eq!(held.info().identity, created.info().identity);
    }

    #[test]
    fn creation_never_claims_an_existing_directory_or_accepts_a_stream_name() {
        let root = tempfile::tempdir().expect("root");
        std::fs::create_dir(root.path().join("occupied")).expect("occupant");
        std::fs::write(root.path().join("occupied/member.txt"), b"foreign").expect("member");
        let parent =
            open_verified_existing(root.path(), NativeFileAccess::Metadata).expect("parent");
        assert!(handle_bound_create_directory(&parent, OsStr::new("occupied")).is_err());
        assert!(handle_bound_create_directory(&parent, OsStr::new("name:stream")).is_err());
        assert_eq!(
            std::fs::read(root.path().join("occupied/member.txt")).expect("member"),
            b"foreign"
        );
    }
}
