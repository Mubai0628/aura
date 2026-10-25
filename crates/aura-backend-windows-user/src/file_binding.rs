use anyhow::{Context, Result, bail, ensure};
use aura_domain_common::ContentHash;
use aura_windows_platform::{NativeFileAccess, NativePathInfo, VerifiedFile};
use aura_windows_user_contract::ExecutionFileBinding;
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Seek},
    path::{Path, PathBuf},
};

pub fn capture(path: &Path, contents: bool) -> Result<(ExecutionFileBinding, VerifiedFile)> {
    capture_inner(path, contents, false)
}

/// A program file that may have other hard links. Git for Windows normally hardlinks
/// cmd/git.exe and bin/git.exe, and cargo puts a second link to each `target\debug\*.exe` in
/// `deps`. These aliases do not weaken this
/// binding: the retained handle refuses writes through every link, and `verify_linked_image`
/// checks identity + content hash again. A launch of the user's own program goes through
/// `checked_program` instead: read in full once, then again only after a
/// change or a failure.
pub fn capture_linked_image(path: &Path) -> Result<(ExecutionFileBinding, VerifiedFile)> {
    capture_inner(path, true, true)
}

pub fn verify_linked_image(binding: &ExecutionFileBinding) -> Result<VerifiedFile> {
    let (actual, retained) = capture_linked_image(Path::new(&binding.path))?;
    ensure!(&actual == binding, "runner.file_binding_changed");
    Ok(retained)
}

/// Git's program, by name; it is the user's own, so it is checked as `checked_program` says.
pub fn capture_git_image(path: &Path) -> Result<(ExecutionFileBinding, VerifiedFile)> {
    ensure!(
        path.file_name().is_some_and(|name| name.eq_ignore_ascii_case("git.exe")),
        "git.image_name"
    );
    crate::checked_program::capture(path, crate::checked_program::Links::Allowed)
}

pub fn verify_git_image(binding: &ExecutionFileBinding) -> Result<VerifiedFile> {
    crate::checked_program::hold(binding, crate::checked_program::Links::Allowed)
}

fn capture_inner(
    path: &Path,
    contents: bool,
    other_links: bool,
) -> Result<(ExecutionFileBinding, VerifiedFile)> {
    let mut file = open_inner(path, contents, other_links)?;
    let hash = if contents { Some(hash_held(&mut file)?) } else { None };
    Ok((binding_of(&file, hash), file))
}

/// Opens `path` as a capture does — for reading and held against writes when `contents` — and
/// checks that it is the file the path named and has no other link unless `other_links`.
pub(crate) fn open_inner(path: &Path, contents: bool, other_links: bool) -> Result<VerifiedFile> {
    ensure!(path.is_absolute(), "runner.relative_path");
    let initial = aura_windows_platform::inspect_existing_path(path, false)
        .map_err(anyhow::Error::from)
        .context("runner.capture.inspect")?;
    ensure!(!initial.is_reparse_point, "runner.reparse_path");
    let file = if initial.is_directory || !contents {
        aura_windows_platform::open_execution_metadata(path)
            .map_err(anyhow::Error::from)
            .context("runner.capture.open")?
    } else {
        aura_windows_platform::open_verified_import_source(path, NativeFileAccess::Read)
            .map_err(anyhow::Error::from)
            .context("runner.capture.open")?
    };
    ensure!(file.info().identity == initial.identity, "runner.path_changed");
    ensure!(
        file.info().is_directory
            || file.info().number_of_links == 1
            || other_links
            || is_windows_cmd_image(file.info()),
        "runner.aliased_file"
    );
    Ok(file)
}

/// The content hash of a file held for reading, read from its start.
pub(crate) fn hash_held(file: &mut VerifiedFile) -> Result<ContentHash> {
    ensure!(!file.info().is_directory, "runner.expected_file");
    file.file_mut().rewind()?;
    let mut digest = Sha256::new();
    let mut bytes = [0_u8; 65536];
    loop {
        let count = file.file_mut().read(&mut bytes)?;
        if count == 0 {
            break;
        }
        digest.update(&bytes[..count]);
    }
    Ok(ContentHash {
        algorithm: aura_domain_common::ContentHashAlgorithm::Sha256,
        hex: format!("{:x}", digest.finalize()),
    })
}

pub(crate) fn binding_of(file: &VerifiedFile, sha256: Option<ContentHash>) -> ExecutionFileBinding {
    let identity = &file.info().identity;
    ExecutionFileBinding {
        path: file.info().final_path.to_string_lossy().into_owned(),
        volume_serial: identity.volume_serial_number.to_string(),
        file_id: identity.file_id,
        sha256,
    }
}

pub fn verify(binding: &ExecutionFileBinding) -> Result<VerifiedFile> {
    let (actual, retained) = capture(Path::new(&binding.path), binding.sha256.is_some())?;
    ensure!(&actual == binding, "runner.file_binding_changed");
    Ok(retained)
}

/// Microsoft Windows system-component catalog System32 CMD only. Exact identity
/// is still required after a journaled re-prepare; this never ignores
/// hash/file_id for Core, Runner, or pwsh.
pub fn verify_system_cmd(binding: &ExecutionFileBinding) -> Result<VerifiedFile> {
    evaluate_system_cmd(binding, microsoft_catalog)
}

pub fn capture_trusted_system_cmd() -> Result<ExecutionFileBinding> {
    let path = system_cmd_path()?;
    let (binding, mut retained) = capture(&path, true)?;
    ensure_live_system_cmd(retained.info())?;
    microsoft_catalog(&mut retained, Path::new(&binding.path))?;
    Ok(binding)
}

pub(crate) fn evaluate_system_cmd(
    binding: &ExecutionFileBinding,
    catalog: impl FnOnce(&mut VerifiedFile, &Path) -> Result<()>,
) -> Result<VerifiedFile> {
    let (actual, mut retained) = capture(Path::new(&binding.path), binding.sha256.is_some())?;
    ensure_live_system_cmd(retained.info())?;
    catalog(&mut retained, Path::new(&actual.path))?;
    if &actual == binding {
        return Ok(retained);
    }
    ensure!(
        actual.path == binding.path && actual.volume_serial == binding.volume_serial,
        "runner.file_binding_changed"
    );
    bail!("runner.system_cmd_serviced")
}

fn microsoft_catalog(file: &mut VerifiedFile, path: &Path) -> Result<()> {
    file.file_mut().rewind()?;
    aura_windows_platform::verify_microsoft_catalog_executable(file.file(), path)
        .map_err(anyhow::Error::from)
}

fn system_cmd_path() -> Result<PathBuf> {
    Ok(aura_windows_platform::windows_directory()?.join("System32/cmd.exe"))
}

fn ensure_live_system_cmd(info: &NativePathInfo) -> Result<()> {
    ensure!(is_windows_cmd_image(info), "runner.invalid_cmd_image");
    ensure_no_reparse_ancestors(&info.final_path)?;
    ensure_no_reparse_ancestors(&system_cmd_path()?)?;
    Ok(())
}

fn ensure_no_reparse_ancestors(path: &Path) -> Result<()> {
    for parent in path.ancestors() {
        if parent.is_absolute() {
            ensure!(
                !aura_windows_platform::inspect_existing_path(parent, false)?.is_reparse_point,
                "runner.reparse_path"
            );
        }
    }
    Ok(())
}

/// WinSxS hard-links `System32\cmd.exe`. That is the OS image identity, not an
/// Aura-owned alias, so Setup can bind the managed CMD tool on ordinary Windows.
fn is_windows_cmd_image(info: &NativePathInfo) -> bool {
    let Ok(cmd) = system_cmd_path().and_then(|path| {
        aura_windows_platform::inspect_existing_path(&path, false).map_err(Into::into)
    }) else {
        return false;
    };
    !info.is_directory && !info.is_reparse_point && info.identity == cmd.identity
}
