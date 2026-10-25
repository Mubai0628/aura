//! Copying a plugin into Aura's plugin folder: from a folder (only its checked files are copied)
//! or from a ZIP file (unpacked only after every entry is checked). Either way the copy is
//! checked again in a hidden staging folder before it gets its place, and the source is not
//! changed.
use super::runtime::PluginRuntime;
use aura_plugins::{PluginPackage, scan_plugin, unpack_plugin_archive};
use aura_skills::FileHashCache;
use aura_windows_platform::{NativeFileAccess, open_verified_import_source};
use std::path::{Path, PathBuf};

/// A checked plugin in a staging folder. `folder` is moved to its place; `staging` is removed
/// afterwards (it is `folder` itself or the folder that holds it).
pub(super) struct Staged {
    pub staging: PathBuf,
    pub folder: PathBuf,
    pub package: PluginPackage,
}

pub(super) fn stage(runtime: &PluginRuntime, source: &Path) -> anyhow::Result<Staged> {
    anyhow::ensure!(source.is_absolute(), "plugins.import_path: 请选择一个插件文件夹或 ZIP 文件");
    let root = runtime.root();
    std::fs::create_dir_all(runtime.library())?;
    for folder in [root.clone(), runtime.library()] {
        let metadata = std::fs::symlink_metadata(&folder)?;
        anyhow::ensure!(
            metadata.is_dir() && !crate::skills::reparse_point(&metadata),
            "plugins.store_linked: Aura 的插件文件夹是链接或挂载点，不能导入"
        );
    }
    let metadata = std::fs::symlink_metadata(source)
        .map_err(|_| anyhow::anyhow!("plugins.import_path: 找不到这个文件夹或文件"))?;
    let staging = root.join(format!(".staging-{}", aura_domain_common::CorrelationId::new()));
    let staged = if metadata.is_dir() {
        from_folder(runtime, source, &staging)
    } else {
        from_archive(source, &staging)
    };
    if staged.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    staged
}

/// Copies the checked files, and only those, then checks the copy is the same content.
fn from_folder(runtime: &PluginRuntime, source: &Path, staging: &Path) -> anyhow::Result<Staged> {
    let package = scan_plugin(source, &runtime.hashes)
        .map_err(|error| anyhow::anyhow!("plugins.invalid: {error}"))?;
    let folder = staging.join(&package.manifest.name);
    std::fs::create_dir_all(&folder)?;
    for file in &package.files {
        let relative = file.path.replace('/', "\\");
        let destination = folder.join(&relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(source.join(&relative), destination)?;
    }
    let copied = scan_plugin(&folder, &FileHashCache::default())
        .map_err(|error| anyhow::anyhow!("plugins.invalid: {error}"))?;
    anyhow::ensure!(
        copied.fingerprint == package.fingerprint,
        "plugins.import_changed: 复制时文件夹内容在变，请稍后再试"
    );
    Ok(Staged { staging: staging.to_path_buf(), folder, package: copied })
}

/// Unpacks a ZIP file opened without following links and held so it cannot change meanwhile,
/// then moves the plugin's root to a folder named by the plugin.
fn from_archive(source: &Path, staging: &Path) -> anyhow::Result<Staged> {
    let file = open_verified_import_source(source, NativeFileAccess::Read)
        .map_err(|_| anyhow::anyhow!("plugins.import_path: 这个文件打不开，或是链接，不能导入"))?;
    anyhow::ensure!(
        !file.info().is_directory,
        "plugins.import_path: 请选择一个插件文件夹或 ZIP 文件"
    );
    let unpacked = staging.join(".unpacked");
    std::fs::create_dir_all(&unpacked)?;
    let root = unpack_plugin_archive(file.into_file(), &unpacked)
        .map_err(|error| anyhow::anyhow!("plugins.invalid_archive: {error}"))?;
    let package = scan_plugin(&root, &FileHashCache::default())
        .map_err(|error| anyhow::anyhow!("plugins.invalid: {error}"))?;
    let folder = staging.join(&package.manifest.name);
    std::fs::rename(&root, &folder)?;
    let package = scan_plugin(&folder, &FileHashCache::default())
        .map_err(|error| anyhow::anyhow!("plugins.invalid: {error}"))?;
    Ok(Staged { staging: staging.to_path_buf(), folder, package })
}
