//! What kind of drive a folder is on.
use std::{io, os::windows::ffi::OsStrExt, path::Path};
use windows_sys::Win32::{
    Storage::FileSystem::{GetDriveTypeW, GetVolumePathNameW},
    System::WindowsProgramming::{
        DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOTE, DRIVE_REMOVABLE,
    },
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DriveKind {
    Fixed,
    Removable,
    Remote,
    Optical,
    RamDisk,
    Unknown,
}

/// The kind of drive `path` is on, taken from the volume it is mounted on: a folder that is itself
/// the mount point of another volume counts as that volume.
pub fn drive_kind(path: &Path) -> io::Result<DriveKind> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // The volume path is never longer than the path it was asked for, plus a trailing separator.
    let mut volume = vec![0_u16; wide.len() + 2];
    let capacity = u32::try_from(volume.len()).map_err(|_| io::Error::other("path too long"))?;
    // SAFETY: `wide` is NUL-terminated and `volume` holds `capacity` units.
    if unsafe { GetVolumePathNameW(wide.as_ptr(), volume.as_mut_ptr(), capacity) } == 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: GetVolumePathNameW wrote a NUL-terminated volume root into `volume`.
    let kind = unsafe { GetDriveTypeW(volume.as_ptr()) };
    Ok(match kind {
        DRIVE_FIXED => DriveKind::Fixed,
        DRIVE_REMOVABLE => DriveKind::Removable,
        DRIVE_REMOTE => DriveKind::Remote,
        DRIVE_CDROM => DriveKind::Optical,
        DRIVE_RAMDISK => DriveKind::RamDisk,
        _ => DriveKind::Unknown,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_in_temp_is_on_a_fixed_drive() {
        let folder = tempfile::tempdir().expect("TEMP folder");
        assert_eq!(drive_kind(folder.path()).expect("drive kind"), DriveKind::Fixed);
    }

    #[test]
    fn a_missing_folder_reports_its_volume_or_an_error_never_a_guess() {
        let folder = tempfile::tempdir().expect("TEMP folder");
        let missing = folder.path().join("not-there");
        // A path that does not exist still names a volume; the answer is that volume's kind.
        assert_eq!(drive_kind(&missing).expect("drive kind"), DriveKind::Fixed);
    }
}
