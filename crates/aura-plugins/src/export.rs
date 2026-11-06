//! A plugin exported as a ZIP file: the files `scan_plugin` read, at their paths, checked again as
//! they are copied (still ordinary files with the size and content that were read), so what is
//! exported is what the plugin page showed. It is written beside the chosen file and moved over
//! it at the end; an export that fails leaves the chosen file as it was.
use crate::archive::MAX_PLUGIN_ARCHIVE_BYTES;
use crate::files::reparse_point;
use crate::package::PluginPackage;
use std::io::{Read, Write};
use std::path::Path;
use zip::{CompressionMethod, DateTime, ZipWriter, write::SimpleFileOptions};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PluginExportSummary {
    pub files: usize,
    /// The files' own size.
    pub bytes: u64,
    /// The ZIP file's size.
    pub archive_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PluginExportError {
    #[error("只能导出成 .zip 文件")]
    NotZip,
    #[error("不能导出到插件自己的文件夹里")]
    InsidePlugin,
    #[error("要保存到的文件夹不存在")]
    NoFolder,
    #[error("要保存的位置是一个文件夹")]
    NotAFile,
    #[error("插件的文件 {0} 在读取后变了，请刷新后再导出")]
    Changed(String),
    #[error("导出的 ZIP 超过 {} MB，导入时读不了", MAX_PLUGIN_ARCHIVE_BYTES / 1024 / 1024)]
    TooLarge,
    #[error("写入失败：{0}")]
    Io(String),
}

fn failed(error: impl std::fmt::Display) -> PluginExportError {
    PluginExportError::Io(error.to_string())
}

/// Writes the plugin that was read as a ZIP file at `destination`.
pub fn export_plugin(
    package: &PluginPackage,
    destination: &Path,
) -> Result<PluginExportSummary, PluginExportError> {
    export_within(package, destination, MAX_PLUGIN_ARCHIVE_BYTES)
}

fn export_within(
    package: &PluginPackage,
    destination: &Path,
    max_archive_bytes: u64,
) -> Result<PluginExportSummary, PluginExportError> {
    if !destination.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("zip")) {
        return Err(PluginExportError::NotZip);
    }
    let folder = destination
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .and_then(|folder| std::fs::canonicalize(folder).ok())
        .filter(|folder| folder.is_dir())
        .ok_or(PluginExportError::NoFolder)?;
    // Writing into the plugin would change the plugin, and a plugin in development would take
    // the ZIP file in the next time it is read.
    if folder.starts_with(std::fs::canonicalize(&package.root).map_err(failed)?) {
        return Err(PluginExportError::InsidePlugin);
    }
    if std::fs::symlink_metadata(destination).is_ok_and(|metadata| metadata.is_dir()) {
        return Err(PluginExportError::NotAFile);
    }
    let mut temporary = tempfile::Builder::new()
        .prefix(".aura-plugin-export-")
        .suffix(".part")
        .tempfile_in(&folder)
        .map_err(failed)?;
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(DateTime::default());
    let mut writer = ZipWriter::new(temporary.as_file_mut());
    let mut buffer = vec![0; 64 * 1024];
    for (file, hash) in package.files.iter().zip(&package.file_hashes) {
        let changed = || PluginExportError::Changed(file.path.clone());
        let path = file.path.split('/').fold(package.root.clone(), |path, part| path.join(part));
        let metadata = std::fs::symlink_metadata(&path).map_err(|_| changed())?;
        if reparse_point(&metadata) || !metadata.is_file() || metadata.len() != file.bytes {
            return Err(changed());
        }
        let mut source = std::fs::File::open(&path).map_err(|_| changed())?;
        writer.start_file(file.path.as_str(), options).map_err(failed)?;
        let mut hasher = blake3::Hasher::new();
        let mut copied = 0_u64;
        loop {
            let read = source.read(&mut buffer).map_err(failed)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
            writer.write_all(&buffer[..read]).map_err(failed)?;
            copied += read as u64;
        }
        if copied != file.bytes || hasher.finalize().to_hex().as_str() != hash {
            return Err(changed());
        }
    }
    writer.finish().map_err(failed)?;
    let written = temporary.as_file();
    written.sync_all().map_err(failed)?;
    let archive_bytes = written.metadata().map_err(failed)?.len();
    if archive_bytes > max_archive_bytes {
        return Err(PluginExportError::TooLarge);
    }
    temporary.persist(destination).map_err(|error| failed(error.error))?;
    Ok(PluginExportSummary { files: package.files.len(), bytes: package.bytes, archive_bytes })
}

#[cfg(test)]
mod tests {
    use super::{PluginExportError, export_within};

    #[test]
    fn a_zip_file_larger_than_an_import_reads_is_not_written() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("plain");
        std::fs::create_dir_all(root.join("commands/hello")).expect("folder");
        std::fs::write(
            root.join("aura-plugin.toml"),
            "schema_version = 1\nname = \"plain\"\nversion = \"1\"\n",
        )
        .expect("manifest");
        std::fs::write(
            root.join("commands/hello/command.toml"),
            "name = \"hello\"\ndescription = \"打招呼\"\nversion = 1\n",
        )
        .expect("command");
        std::fs::write(root.join("commands/hello/template.md"), "你好\n").expect("template");
        let package =
            crate::scan_plugin(&root, &aura_skills::FileHashCache::default()).expect("a plugin");
        let destination = temp.path().join("plain.zip");
        assert_eq!(
            export_within(&package, &destination, 64).expect_err("too large"),
            PluginExportError::TooLarge
        );
        assert!(!destination.exists());
        let left: Vec<_> = std::fs::read_dir(temp.path()).expect("folder").collect();
        assert_eq!(left.len(), 1, "only the plugin's folder; the temporary file is gone");
    }
}
