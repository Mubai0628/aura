//! Staging owns a verified file handle throughout bounded writes and final publication.
use super::*;

pub struct StagedFilePublication {
    guard: Option<HandleBoundPublishedFile>,
    expected: Option<(NativeFileIdentity, String)>,
    bytes_written: u64,
    max_chunk: usize,
}

impl StagedFilePublication {
    pub fn new(
        parent: &VerifiedFile,
        file_name: &OsStr,
        expected: Option<(NativeFileIdentity, String)>,
    ) -> Result<Self, HandlePublishError> {
        ensure_parent_unchanged(parent)?;
        if !parent.info().is_directory || !is_leaf(file_name) {
            return Err(HandlePublishError::InvalidFileName);
        }
        let temporary_name = random_leaf(".aura-download-")?;
        let held_parent = verified_from_file(parent.file().try_clone()?)?;
        let path = parent.info().final_path.join(&temporary_name);
        let wide = wide(&path);
        // SAFETY: the path is NUL terminated and CreateFileW returns one owned handle.
        let raw = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE | DELETE,
                0,
                ptr::null(),
                CREATE_NEW,
                FILE_ATTRIBUTE_TEMPORARY,
                ptr::null_mut(),
            )
        };
        if raw.is_null() || raw == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error().into());
        }
        // SAFETY: ownership of the unique valid handle is transferred to File.
        let file = unsafe { File::from_raw_handle(raw) };
        let staging = match file.try_clone().and_then(verified_from_file) {
            Ok(staging) => staging,
            Err(error) => {
                delete_by_handle(&file)?;
                return Err(error.into());
            }
        };
        drop(file);
        let guard = HandleBoundPublishedFile {
            published: Some(staging),
            backup: None,
            parent: held_parent,
            file_name: file_name.to_owned(),
            finalized: false,
        };
        if !final_path_within(parent.info(), guard.info()) {
            return Err(HandlePublishError::ResultEscapedParent);
        }
        verify_actual_parent_identity(
            parent,
            guard.info(),
            guard.published.as_ref().expect("staged handle").file(),
            &temporary_name,
        )?;
        Ok(Self { guard: Some(guard), expected, bytes_written: 0, max_chunk: 0 })
    }

    /// A caller cannot grow a response-sized staging buffer through this port.
    pub fn append(&mut self, bytes: &[u8]) -> Result<(), HandlePublishError> {
        if bytes.len() > 64 * 1024 {
            return Err(io::Error::other("download chunk exceeds 64 KiB").into());
        }
        self.guard.as_mut().expect("unfinished staging").file_mut().write_all(bytes)?;
        self.bytes_written += bytes.len() as u64;
        self.max_chunk = self.max_chunk.max(bytes.len());
        Ok(())
    }
    pub fn bytes_written(&self) -> u64 {
        self.bytes_written
    }
    pub fn max_chunk_bytes(&self) -> usize {
        self.max_chunk
    }

    pub fn publish(mut self) -> Result<HandleBoundPublishedFile, HandlePublishError> {
        let mut guard = self.guard.take().expect("unfinished staging");
        guard.file_mut().sync_all()?;
        ensure_parent_unchanged(&guard.parent)?;
        let destination = guard.parent.info().final_path.join(&guard.file_name);
        if let Some((identity, hash)) = &self.expected {
            let mut target = open_replace_target(&destination)?;
            if target.info().identity != *identity
                || !final_path_within(guard.parent.info(), target.info())
            {
                return Err(HandlePublishError::TargetIdentityChanged);
            }
            let actual = blake3_of_handle(target.file_mut())?;
            if &actual != hash {
                return Err(HandlePublishError::TargetContentChanged {
                    expected: hash.clone(),
                    actual,
                });
            }
            let backup = random_leaf(".aura-backup-")?;
            rename_handle_into_parent(target.file(), &guard.parent, &backup, false)?;
            // The old object has moved. The guard now owns both restoration and temp cleanup.
            guard.backup = Some(target);
        }
        let publication = (|| -> Result<(), HandlePublishError> {
            ensure_parent_unchanged(&guard.parent)?;
            rename_handle_into_parent(
                guard.published.as_ref().expect("staged file").file(),
                &guard.parent,
                &guard.file_name,
                false,
            )?;
            guard.published = Some(verified_from_file(
                guard.published.as_ref().expect("published file").file().try_clone()?,
            )?);
            let held = guard.published.as_ref().expect("published file");
            verify_actual_parent_identity(
                &guard.parent,
                held.info(),
                held.file(),
                &guard.file_name,
            )?;
            if !final_path_within(guard.parent.info(), held.info())
                || !path_eq(&destination, &held.info().final_path)
            {
                return Err(HandlePublishError::ResultEscapedParent);
            }
            Ok(())
        })();
        if let Err(error) = publication {
            return Err(guard.finalize_failure(error));
        }
        Ok(guard)
    }
}

pub(super) fn blake3_of_handle(file: &mut File) -> Result<String, HandlePublishError> {
    use std::io::{Read as _, Seek as _, SeekFrom};
    file.seek(SeekFrom::Start(0))?;
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}
