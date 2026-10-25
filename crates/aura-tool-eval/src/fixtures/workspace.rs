use super::Task;
use anyhow::{Result, ensure};
use std::path::{Path, PathBuf};

pub struct PreparedTask {
    pub workspace: PathBuf,
    pub grading: PathBuf,
    pub task: Task,
}

/// The editable fixture is a copy; graders are only introduced after the Agent has stopped.
pub fn prepare(root: &Path, task: Task) -> Result<PreparedTask> {
    ensure!(task.implemented, "eval.fixture_planned_for_later_batch");
    let aura = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize()?;
    let excluded = aura.parent().expect("repository parent").join("AuraTest");
    let root = prospective_root(root)?;
    ensure!(!within(&root, &aura), "eval.workspace_must_be_outside_aura_tree");
    ensure!(!within(&root, &excluded), "eval.auratest_workspace_refused");
    std::fs::create_dir_all(&root)?;
    let root = root.canonicalize()?;
    ensure!(!within(&root, &aura) && !within(&root, &excluded), "eval.workspace_identity_changed");
    let workspace = root.join("workspaces").join(task.id);
    let grading = root.join("grading").join(task.id);
    ensure!(!workspace.exists() && !grading.exists(), "eval.run_directory_already_used");
    std::fs::create_dir_all(&workspace)?;
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures").join(task.id);
    copy_tree(&source.join("workspace"), &workspace)?;

    Ok(PreparedTask { workspace, grading, task })
}

fn prospective_root(root: &Path) -> Result<PathBuf> {
    ensure!(
        !root.components().any(|v| v == std::path::Component::ParentDir),
        "eval.parent_traversal_refused"
    );
    let root = std::path::absolute(root)?;
    let mut existing = root.as_path();
    let mut tail = vec![];
    while !existing.try_exists()? {
        tail.push(
            existing.file_name().ok_or_else(|| anyhow::anyhow!("eval.invalid_root"))?.to_owned(),
        );
        existing = existing.parent().ok_or_else(|| anyhow::anyhow!("eval.invalid_root"))?;
    }
    let mut canonical = existing.canonicalize()?;
    for name in tail.into_iter().rev() {
        canonical.push(name);
    }
    Ok(canonical)
}
// Conservative deny-only comparison for evaluation roots, never an authorization key.
fn within(path: &Path, base: &Path) -> bool {
    let text = |path: &Path| {
        path.to_string_lossy().trim_start_matches(r"\\?\").replace('/', "\\").to_lowercase()
    };
    let (path, base) = (text(path), text(base));
    path == base || path.starts_with(&format!("{base}\\"))
}
pub(super) fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for item in std::fs::read_dir(from)? {
        let item = item?;
        let kind = item.file_type()?;
        ensure!(!kind.is_symlink(), "eval.fixture_links_refused");
        if item.file_name() == "target"
            || item.file_name() == "node_modules"
            || item.file_name() == ".git"
            || item.file_name() == "__pycache__"
        {
            continue;
        }
        if kind.is_dir() {
            copy_tree(&item.path(), &to.join(item.file_name()))?;
        } else if kind.is_file() {
            std::fs::copy(item.path(), to.join(item.file_name()))?;
        }
    }
    Ok(())
}
