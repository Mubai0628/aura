//! Packing a release folder's payload into the update package and the setup program.
use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::Serialize;
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, DateTime, ZipWriter, write::SimpleFileOptions};

use crate::payload;

#[derive(Debug, Serialize)]
pub struct Packed {
    pub version: String,
    pub files: usize,
    pub extracted_bytes: u64,
    pub setup: PathBuf,
    pub setup_sha256: String,
    pub update: PathBuf,
    pub update_sha256: String,
}

fn walk(root: &Path, folder: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "a release holds no links"));
        }
        if kind.is_dir() {
            walk(root, &entry.path(), out)?;
        } else {
            out.push(entry.path().strip_prefix(root).map_err(io::Error::other)?.to_path_buf());
        }
    }
    Ok(())
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    io::copy(&mut File::open(path)?, &mut hasher)?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Zips `payload` (the folder holding `Aura.ico` and `versions/<version>`, whose manifest names the
/// launcher and the setup program in it) into `update`, then writes `setup` as `stub` with that zip
/// appended. Entries are in name order with a
/// fixed time, so the same payload packs to the same bytes.
pub fn pack(payload_folder: &Path, stub: &Path, setup: &Path, update: &Path) -> Result<Packed> {
    let mut files = Vec::new();
    walk(payload_folder, payload_folder, &mut files)?;
    files.sort();
    ensure!(
        files.iter().any(|path| {
            path.starts_with("versions") && path.file_name() == Some(OsStr::new(payload::LAUNCHER))
        }),
        "the payload has no launcher"
    );
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true)
        .last_modified_time(DateTime::default());
    let mut writer = ZipWriter::new(File::create(update)?);
    for relative in &files {
        let name = relative.to_string_lossy().replace('\\', "/");
        writer.start_file(name, options)?;
        io::copy(&mut File::open(payload_folder.join(relative))?, &mut writer)?;
    }
    writer.finish()?.flush()?;

    let stub_bytes = payload::program_without_release(stub)?;
    payload::append(&stub_bytes, update, setup)?;
    let mut archive = payload::open_in_program(setup).context("the packed setup does not open")?;
    let version = payload::version_of(&mut archive)?;
    ensure!(archive.len() == files.len(), "the packed setup lost files");
    Ok(Packed {
        version,
        files: files.len(),
        extracted_bytes: payload::extracted_size(&mut archive)?,
        setup_sha256: sha256_file(setup)?,
        setup: setup.to_path_buf(),
        update_sha256: sha256_file(update)?,
        update: update.to_path_buf(),
    })
}
