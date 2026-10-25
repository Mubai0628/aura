//! Bind only non-publishing sharing failures to the exact unchanged files.
use crate::path::{NativeFileAccess, NativePathInfo, open_verified_existing};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read},
    path::Path,
    time::SystemTime,
};

#[derive(PartialEq, Eq)]
pub(super) struct FileBinding {
    identity: NativePathInfo,
    bytes: u64,
    modified: SystemTime,
    sha256: [u8; 32],
}
fn fingerprint(path: &Path) -> io::Result<FileBinding> {
    let mut file = open_verified_existing(path, NativeFileAccess::Read)?;
    let meta = file.file().metadata()?;
    let modified = meta.modified()?;
    let mut hash = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let n = file.file_mut().read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    let after = file.file().metadata()?;
    if after.len() != meta.len() || after.modified()? != modified {
        return Err(io::Error::other("config file changed while binding"));
    }
    Ok(FileBinding {
        identity: file.info().clone(),
        bytes: meta.len(),
        modified,
        sha256: hash.finalize().into(),
    })
}
pub(super) fn binding(
    replacement: &Path,
    destination: &Path,
) -> io::Result<(FileBinding, FileBinding)> {
    Ok((fingerprint(replacement)?, fingerprint(destination)?))
}

#[cfg(test)]
mod tests {
    use crate::atomic_file::{AtomicReplaceError, atomic_replace_file};
    use std::{
        fs::{self, OpenOptions},
        os::windows::fs::OpenOptionsExt,
        thread,
        time::Duration,
    };
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

    #[test]
    fn transient_delete_sharing_releases_without_losing_either_content() {
        let dir = tempfile::tempdir().expect("temp");
        let target = dir.path().join("config");
        let replacement = dir.path().join("pending");
        fs::write(&target, b"old").expect("old");
        fs::write(&replacement, b"new").expect("new");
        let held = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&target)
            .expect("held reader");
        let release = thread::spawn(move || {
            thread::sleep(Duration::from_millis(90));
            drop(held);
        });
        atomic_replace_file(&replacement, &target)
            .expect("same operation succeeds after reader releases");
        release.join().expect("release");
        assert_eq!(fs::read(&target).expect("result"), b"new");
        assert!(!replacement.exists());
    }
    #[test]
    fn permanent_sharing_failure_retains_both_files() {
        let dir = tempfile::tempdir().expect("temp");
        let target = dir.path().join("config");
        let replacement = dir.path().join("pending");
        fs::write(&target, b"old").expect("old");
        fs::write(&replacement, b"new").expect("new");
        let _held = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&target)
            .expect("held reader");
        assert!(atomic_replace_file(&replacement, &target).is_err());
        assert_eq!(fs::read(&target).expect("old retained"), b"old");
        assert_eq!(fs::read(&replacement).expect("new retained"), b"new");
    }
    #[test]
    fn changed_target_cannot_be_replaced_during_sharing_retry() {
        let dir = tempfile::tempdir().expect("temp");
        let target = dir.path().join("config");
        let replacement = dir.path().join("pending");
        fs::write(&target, b"old").expect("old");
        fs::write(&replacement, b"new").expect("new");
        let held = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(&target)
            .expect("held reader");
        let changed = target.clone();
        let release = thread::spawn(move || {
            thread::sleep(Duration::from_millis(90));
            fs::write(changed, b"external edit").expect("external edit");
            thread::sleep(Duration::from_millis(90));
            drop(held);
        });
        let result = atomic_replace_file(&replacement, &target);
        release.join().expect("release");
        assert!(matches!(result, Err(AtomicReplaceError::RetryBindingChanged)));
        assert_eq!(fs::read(target).expect("external retained"), b"external edit");
    }
}
