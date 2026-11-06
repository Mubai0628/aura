//! The Skills that come with Aura. Their files are built into Core and written, once per start,
//! to a folder of Aura's own, where they are read like any other Skill. Only a file that is
//! missing or differs is written, and anything else in that folder is removed: it holds nothing
//! of the user's.
use aura_skills::{RootStatus, root_status};
use std::path::Path;

/// Under Aura's local data root.
pub(crate) const BUILTIN_SKILLS: &str = "skills/builtin";

/// Each file, `/`-separated under that folder.
const FILES: &[(&str, &str)] = &[
    (
        "write-aura-plugin/SKILL.md",
        include_str!("../../../../resources/skills/write-aura-plugin/SKILL.md"),
    ),
    (
        "write-aura-plugin/reference/manifest.md",
        include_str!("../../../../resources/skills/write-aura-plugin/reference/manifest.md"),
    ),
    (
        "write-aura-plugin/reference/panel.md",
        include_str!("../../../../resources/skills/write-aura-plugin/reference/panel.md"),
    ),
    (
        "write-aura-plugin/reference/parts.md",
        include_str!("../../../../resources/skills/write-aura-plugin/reference/parts.md"),
    ),
    (
        "write-aura-plugin/reference/theme.md",
        include_str!("../../../../resources/skills/write-aura-plugin/reference/theme.md"),
    ),
    (
        "write-aura-plugin/reference/service.md",
        include_str!("../../../../resources/skills/write-aura-plugin/reference/service.md"),
    ),
    (
        "write-aura-plugin/reference/channels.md",
        include_str!("../../../../resources/skills/write-aura-plugin/reference/channels.md"),
    ),
    (
        "write-aura-plugin/sdk/python/aura_plugin.py",
        include_str!("../../../../resources/skills/write-aura-plugin/sdk/python/aura_plugin.py"),
    ),
    (
        "write-aura-plugin/sdk/node/aura-plugin.mjs",
        include_str!("../../../../resources/skills/write-aura-plugin/sdk/node/aura-plugin.mjs"),
    ),
    (
        "write-aura-plugin/examples/fake-chat/README.md",
        include_str!("../../../../resources/skills/write-aura-plugin/examples/fake-chat/README.md"),
    ),
    (
        "write-aura-plugin/examples/fake-chat/platform/fake_platform.py",
        include_str!(
            "../../../../resources/skills/write-aura-plugin/examples/fake-chat/platform/fake_platform.py"
        ),
    ),
    (
        "write-aura-plugin/examples/fake-chat/plugin/aura-plugin.toml",
        include_str!(
            "../../../../resources/skills/write-aura-plugin/examples/fake-chat/plugin/aura-plugin.toml"
        ),
    ),
    (
        "write-aura-plugin/examples/fake-chat/plugin/service/main.py",
        include_str!(
            "../../../../resources/skills/write-aura-plugin/examples/fake-chat/plugin/service/main.py"
        ),
    ),
];

/// A start's early reading and its runtime may both list at once; one writes at a time.
static WRITING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Writes the files under `local_root`. A link anywhere on the way stops it: nothing is written
/// through one.
pub(crate) fn write_all(local_root: &Path) -> anyhow::Result<()> {
    let _writing = WRITING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root =
        BUILTIN_SKILLS.split('/').fold(local_root.to_path_buf(), |path, part| path.join(part));
    std::fs::create_dir_all(&root)?;
    anyhow::ensure!(
        root_status(local_root, BUILTIN_SKILLS) == RootStatus::Ready,
        "a folder on the way to {} is a link",
        root.display()
    );
    for (relative, text) in FILES {
        let parts: Vec<&str> = relative.split('/').collect();
        let mut path = root.clone();
        for folder in &parts[..parts.len() - 1] {
            path.push(folder);
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) if super::reparse_point(&metadata) || !metadata.is_dir() => {
                    anyhow::bail!("{} is not a plain folder", path.display())
                }
                Ok(_) => {}
                Err(_) => std::fs::create_dir(&path)?,
            }
        }
        path.push(parts[parts.len() - 1]);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if super::reparse_point(&metadata) || !metadata.is_file() => {
                anyhow::bail!("{} is not a plain file", path.display())
            }
            Ok(_) if std::fs::read(&path)? == text.as_bytes() => continue,
            _ => std::fs::write(&path, text)?,
        }
    }
    remove_others(&root, "")
}

/// Removes what is not one of the files, a folder at a time. A link is removed itself, never
/// followed.
fn remove_others(folder: &Path, prefix: &str) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let relative = if prefix.is_empty() { name } else { format!("{prefix}/{name}") };
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)?;
        let under = format!("{relative}/");
        if super::reparse_point(&metadata) {
            if metadata.is_dir() {
                std::fs::remove_dir(&path)?;
            } else {
                std::fs::remove_file(&path)?;
            }
        } else if metadata.is_dir() {
            if FILES.iter().any(|(file, _)| file.starts_with(&under)) {
                remove_others(&path, &relative)?;
            } else {
                std::fs::remove_dir_all(&path)?;
            }
        } else if !FILES.iter().any(|(file, _)| *file == relative) {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}
