use anyhow::{Context, Result, ensure};
use std::path::{Path, PathBuf};

use super::BudgetStage;

/// One account ledger per stage across all its real tasks and schema probes, separate from Aura
/// data.
///
/// It lives in the user profile, not AppData: a packaged (MSIX) app has
/// its AppData writes redirected into its own package folder, so a tool outside that package
/// would open a second, empty ledger and the cumulative cap would silently stop holding.
pub fn real_ledger_path(stage: BudgetStage) -> Result<PathBuf> {
    let profile = std::env::var_os("USERPROFILE").context("eval.user_profile_required")?;
    let app_data = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    real_ledger_path_in(Path::new(&profile), app_data.as_deref(), stage)
}

/// Refuses to start the shared ledger while an earlier AppData ledger has not been moved to it.
pub fn real_ledger_path_in(
    profile: &Path,
    app_data: Option<&Path>,
    stage: BudgetStage,
) -> Result<PathBuf> {
    let root = profile.join(".aura-tool-eval").join(stage.directory());
    let path = root.join("ledger.db");
    let earlier =
        app_data.map(|v| v.join("AuraToolEval").join(stage.directory()).join("ledger.db"));
    ensure!(
        path.is_file() || earlier.is_none_or(|v| !v.is_file()),
        "eval.real_ledger_migration_required"
    );
    std::fs::create_dir_all(&root)?;
    Ok(path)
}
