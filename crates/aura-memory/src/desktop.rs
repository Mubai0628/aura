use crate::*;
use aura_domain_common::RuntimeInstanceId;
use std::sync::Arc;
impl MemoryService {
    pub fn desktop_vault(
        &self,
        scope: MemoryScope,
        workspace: Option<&MemoryWorkspace>,
    ) -> MemoryResult<Arc<Vault>> {
        match scope {
            MemoryScope::Global => self.global_vault(),
            MemoryScope::Project => self.vault(
                scope,
                workspace
                    .ok_or(MemoryError::Invalid("select a workspace for its project Vault"))?,
            ),
        }
    }
    pub fn desktop_snapshot(
        &self,
        workspace: Option<&MemoryWorkspace>,
        runtime: RuntimeInstanceId,
    ) -> MemoryResult<MemoryDesktopSnapshot> {
        let mut vaults = vec![self.global_vault()?.desktop_status()?];
        let mut recent_runs = vec![];
        if let Some(workspace) = workspace {
            let project = self.vault(MemoryScope::Project, workspace)?;
            vaults.push(project.desktop_status()?);
            recent_runs = project.maintenance_runs(runtime)?;
        }
        Ok(MemoryDesktopSnapshot { vaults, recent_runs })
    }
}
impl Vault {
    pub fn desktop_status(&self) -> MemoryResult<MemoryVaultStatus> {
        let status = self.status()?;
        let exists = self.lock().fs.exists()?;
        let log = crate::maintenance_log::path(self.lock().audit.fs.root());
        Ok(MemoryVaultStatus {
            scope: self.scope().scope(),
            directory: {
                let root = self.root();
                root.to_str().ok_or(MemoryError::Invalid("Vault directory encoding"))?;
                // Held in Windows' verbatim form; people read, and Explorer opens, the ordinary one.
                aura_domain_common::display_path(root)
            },
            exists,
            valid: status.valid,
            invalid: status.invalid,
            rebuilding: status.rebuilding,
            watcher_error: status.watcher_error,
            recent_changes: self.history(None, 20)?,
            maintenance_log: log.exists().then(|| aura_domain_common::display_path(&log)),
        })
    }
}
