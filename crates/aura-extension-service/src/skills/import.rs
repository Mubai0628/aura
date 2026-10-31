//! Importing a Skill into Aura's own folder: from a folder (only its checked files are copied)
//! or from a ZIP file (unpacked only after every entry is checked). Either way the new copy is
//! checked again in a hidden staging folder before it gets its name, and the source is not
//! changed.
use super::runtime::{STORE_SKILLS, SkillRuntime};
use aura_skills::{FileHashCache, RootStatus, SkillPackage, root_status, scan_skill};
use aura_windows_platform::{NativeFileAccess, open_verified_import_source};
use std::path::{Path, PathBuf};

/// A checked Skill in a staging folder under Aura's own. `folder` is moved to its final name;
/// `staging` is removed afterwards (it is `folder` itself or the folder that holds it).
pub(super) struct Staged {
    pub staging: PathBuf,
    pub folder: PathBuf,
    pub package: SkillPackage,
}

pub(super) fn stage(runtime: &SkillRuntime, source: &Path) -> anyhow::Result<Staged> {
    anyhow::ensure!(source.is_absolute(), "skills.import_path: 请选择一个 Skill 文件夹或 ZIP 文件");
    let store = runtime.store_root();
    std::fs::create_dir_all(&store)?;
    let local = store.parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_default();
    anyhow::ensure!(
        root_status(&local, STORE_SKILLS) == RootStatus::Ready,
        "skills.store_linked: Aura 的 Skill 文件夹是链接或挂载点，不能导入"
    );
    let metadata = std::fs::symlink_metadata(source)
        .map_err(|_| anyhow::anyhow!("skills.import_path: 找不到这个文件夹或文件"))?;
    let staging = store.join(format!(".import-{}", aura_domain_common::CorrelationId::new()));
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

/// Copies the checked files, and only those, then checks that the copy is the same content.
fn from_folder(runtime: &SkillRuntime, source: &Path, staging: &Path) -> anyhow::Result<Staged> {
    let package = scan_skill(source, &runtime.hashes)
        .map_err(|error| anyhow::anyhow!("skills.invalid: {error}"))?;
    std::fs::create_dir(staging)?;
    for file in &package.files {
        let relative = aura_skills::resource_path(&file.path)
            .ok_or_else(|| anyhow::anyhow!("skills.import_path: {}", file.path))?
            .replace('/', "\\");
        let destination = staging.join(&relative);
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(source.join(&relative), destination)?;
    }
    let copied = scan_skill(staging, &FileHashCache::default())
        .map_err(|error| anyhow::anyhow!("skills.invalid: {error}"))?;
    anyhow::ensure!(
        copied.fingerprint == package.fingerprint,
        "skills.import_changed: 复制时文件夹内容在变，请稍后再试"
    );
    Ok(Staged { staging: staging.to_path_buf(), folder: staging.to_path_buf(), package: copied })
}

/// Unpacks a ZIP file opened without following links and held so it cannot change meanwhile.
fn from_archive(source: &Path, staging: &Path) -> anyhow::Result<Staged> {
    let file = open_verified_import_source(source, NativeFileAccess::Read)
        .map_err(|_| anyhow::anyhow!("skills.import_path: 这个文件打不开，或是链接，不能导入"))?;
    anyhow::ensure!(
        !file.info().is_directory,
        "skills.import_path: 请选择一个 Skill 文件夹或 ZIP 文件"
    );
    std::fs::create_dir(staging)?;
    let folder = aura_skills::unpack_skill_archive(file.into_file(), staging)
        .map_err(|error| anyhow::anyhow!("skills.invalid_archive: {error}"))?;
    let package = scan_skill(&folder, &FileHashCache::default())
        .map_err(|error| anyhow::anyhow!("skills.invalid: {error}"))?;
    Ok(Staged { staging: staging.to_path_buf(), folder, package })
}

/// A folder name in Aura's own folder for this Skill: its name, numbered when taken.
pub(super) fn free_name(store: &Path, skill_name: &str) -> String {
    let base: String = skill_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_owned();
    let base = if base.is_empty() { "skill".to_owned() } else { base };
    let mut name = base.clone();
    let mut counter = 2;
    while std::fs::symlink_metadata(store.join(&name)).is_ok() {
        name = format!("{base}-{counter}");
        counter += 1;
    }
    name
}
