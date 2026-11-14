//! Free space before a test build. A build that runs out of space fails in the middle with
//! errors that look like test failures, and tests that cannot write their fixtures fail the same.
use std::{path::Path, process::Command};

use anyhow::{Context, Result, ensure};

/// The build drive holds the test build, its incremental state and the debug files.
pub(crate) const BUILD_DRIVE_MINIMUM_GB: f64 = 60.0;
/// The TEMP drive holds the run's fixtures until the run ends.
pub(crate) const TEMP_DRIVE_MINIMUM_GB: f64 = 20.0;

pub(crate) fn ensure(root: &Path) -> Result<()> {
    let build = free_gb(root)?;
    ensure!(
        build >= BUILD_DRIVE_MINIMUM_GB,
        "the build drive has {build:.1} GB free and a test build needs {BUILD_DRIVE_MINIMUM_GB} GB; \
         `cargo xtask sweep-target` lists build outputs cargo will not use again, `--apply` \
         deletes them"
    );
    let temp_folder = std::env::temp_dir();
    let temp = free_gb(&temp_folder)?;
    ensure!(
        temp >= TEMP_DRIVE_MINIMUM_GB,
        "the TEMP drive ({}) has {temp:.1} GB free and a test run needs {TEMP_DRIVE_MINIMUM_GB} GB",
        temp_folder.display()
    );
    Ok(())
}

/// Free space of the drive that holds `path`, in GB of 2^30 bytes as Windows shows them. The
/// standard library has no such query and this crate has no unsafe code, so Windows PowerShell
/// answers it.
pub(crate) fn free_gb(path: &Path) -> Result<f64> {
    let drive = drive_of(path)?;
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!("[System.IO.DriveInfo]::new('{drive}').AvailableFreeSpace"),
        ])
        .output()
        .context("failed to start Windows PowerShell for the free space")?;
    ensure!(output.status.success(), "the free space query failed with {}", output.status);
    let bytes: u64 = String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse()
        .context("the free space query returned no number")?;
    Ok(bytes as f64 / (1u64 << 30) as f64)
}

/// `D` for `D:\Work\app` and for `\\?\D:\…`; a UNC path has no drive letter.
pub(crate) fn drive_of(path: &Path) -> Result<char> {
    let text = path.to_string_lossy();
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    let mut characters = text.chars();
    match (characters.next(), characters.next()) {
        (Some(letter), Some(':')) if letter.is_ascii_alphabetic() => {
            Ok(letter.to_ascii_uppercase())
        }
        _ => anyhow::bail!("{} is not on a drive with a letter", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    #[test]
    fn the_drive_comes_from_a_plain_or_a_verbatim_path() {
        assert_eq!(super::drive_of(Path::new(r"D:\Work\app")).expect("plain path"), 'D');
        assert_eq!(super::drive_of(Path::new(r"\\?\c:\Users\x\Temp")).expect("verbatim path"), 'C');
        assert!(super::drive_of(Path::new(r"\\server\share")).is_err());
    }
}
