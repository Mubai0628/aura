//! Whole-package verification before the dedicated Runner consumes script tools.
use anyhow::{Result, ensure};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

#[derive(Deserialize)]
struct Pin {
    id: String,
    version: String,
    archive_sha256: String,
    files_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    path: String,
    bytes: u64,
    sha256: String,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Same pinned payload verification for the adoption candidate's local managed package.
pub(crate) fn verify_managed_payload(
    payload: &Path,
    manifest: &Value,
) -> Result<Vec<aura_windows_platform::VerifiedFile>> {
    let pin: Pin = serde_json::from_str(include_str!("../../../runtimes/powershell.json"))?;
    ensure!(
        manifest["schema"] == "r06-runtime-files-v1"
            && manifest["id"] == pin.id
            && manifest["version"] == pin.version
            && manifest["archive_sha256"] == pin.archive_sha256,
        "runtime pin changed"
    );
    let files: Vec<File> = serde_json::from_value(manifest["files"].clone())?;
    let mut lines = files
        .iter()
        .map(|f| format!("{}|{}|{}", f.path, f.bytes, f.sha256.to_ascii_lowercase()))
        .collect::<Vec<_>>();
    lines.sort();
    ensure!(digest(lines.join("\n").as_bytes()) == pin.files_sha256, "runtime manifest changed");
    verify_files(payload, &files)
}

pub fn verify(root: &Path, record: &Value) -> Result<()> {
    verify_and_retain(root, record).map(drop)
}

/// Retain all package file identities through execution. A validated DLL or
/// script cannot be replaced between hashing and the child loading it.
pub fn verify_and_retain(
    root: &Path,
    record: &Value,
) -> Result<Vec<aura_windows_platform::VerifiedFile>> {
    let pin: Pin = serde_json::from_str(include_str!("../../../runtimes/powershell.json"))?;
    let runtime = &record["script_runtime"];
    let path = root.join("runtime");
    ensure!(
        runtime["path"].as_str().map(Path::new) == Some(path.as_path()),
        "runtime path changed"
    );
    let manifest = &runtime["manifest"];
    ensure!(
        manifest["schema"] == "r06-runtime-files-v1"
            && manifest["id"] == pin.id
            && manifest["version"] == pin.version
            && manifest["archive_sha256"] == pin.archive_sha256,
        "runtime pin changed"
    );
    let files: Vec<File> = serde_json::from_value(manifest["files"].clone())?;
    let mut lines = files
        .iter()
        .map(|f| format!("{}|{}|{}", f.path, f.bytes, f.sha256.to_ascii_lowercase()))
        .collect::<Vec<_>>();
    lines.sort();
    ensure!(digest(lines.join("\n").as_bytes()) == pin.files_sha256, "runtime manifest changed");
    let retained = verify_files(&path, &files)?;
    let tools = record["tools"].as_array().ok_or_else(|| anyhow::anyhow!("tools missing"))?;
    let defaults = tools.iter().filter(|t| t["name"] == "powershell").collect::<Vec<_>>();
    ensure!(defaults.len() == 1, "one managed script runtime required");
    let exe = path.join("pwsh.exe");
    let expected = files
        .iter()
        .find(|f| f.path == "pwsh.exe")
        .ok_or_else(|| anyhow::anyhow!("pwsh absent"))?;
    ensure!(
        defaults[0]["path"].as_str().map(Path::new) == Some(exe.as_path())
            && defaults[0]["sha256"]
                .as_str()
                .is_some_and(|v| v.eq_ignore_ascii_case(&expected.sha256)),
        "default runtime binding changed"
    );
    Ok(retained)
}

fn verify_files(root: &Path, files: &[File]) -> Result<Vec<aura_windows_platform::VerifiedFile>> {
    let mut retained = Vec::new();
    let mut expected = files.iter().map(|f| (f.path.clone(), f)).collect::<BTreeMap<_, _>>();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path)?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            ensure!(metadata.file_attributes() & 0x400 == 0, "runtime reparse point rejected");
        }
        ensure!(!metadata.file_type().is_symlink(), "runtime symlink rejected");
        if metadata.is_dir() {
            if path != root {
                let prefix =
                    format!("{}/", path.strip_prefix(root)?.to_string_lossy().replace('\\', "/"));
                ensure!(
                    files.iter().any(|f| f.path.starts_with(&prefix)),
                    "unknown runtime directory"
                );
            }
            for entry in fs::read_dir(&path)? {
                pending.push(entry?.path());
            }
        } else {
            let relative = path.strip_prefix(root)?.to_string_lossy().replace('\\', "/");
            let file = expected
                .remove(&relative)
                .ok_or_else(|| anyhow::anyhow!("unknown runtime file"))?;
            let (binding, handle) = super::file_binding::capture(&path, true)?;
            ensure!(
                handle.file().metadata()?.len() == file.bytes
                    && binding
                        .sha256
                        .as_ref()
                        .is_some_and(|hash| hash.hex.eq_ignore_ascii_case(&file.sha256)),
                "runtime file changed"
            );
            retained.push(handle);
        }
    }
    ensure!(expected.is_empty(), "runtime files missing");
    Ok(retained)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn package_verification_rejects_missing_changed_and_extra_files() -> Result<()> {
        let root = tempfile::tempdir()?;
        let files = vec![File { path: "pwsh.exe".into(), bytes: 2, sha256: digest(b"ok") }];
        assert!(verify_files(root.path(), &files).is_err());
        fs::write(root.path().join("pwsh.exe"), b"ok")?;
        verify_files(root.path(), &files)?;
        fs::write(root.path().join("pwsh.exe"), b"no")?;
        assert!(verify_files(root.path(), &files).is_err());
        fs::write(root.path().join("pwsh.exe"), b"ok")?;
        fs::write(root.path().join("extra.dll"), b"no")?;
        assert!(verify_files(root.path(), &files).is_err());
        Ok(())
    }
}
