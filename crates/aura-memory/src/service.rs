use crate::{
    MemoryError, MemoryResult, MemoryScope, MemorySelector, Vault, VaultRoots, VaultScope,
    VaultWatcher,
};
use aura_domain_common::{MemoryId, WorkspaceId};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug)]
pub struct MemoryWorkspace {
    pub workspace_id: WorkspaceId,
    pub root: PathBuf,
}
pub(crate) struct LiveVault {
    vault: Arc<Vault>,
    _watcher: VaultWatcher,
}
pub struct MemoryService {
    roots: VaultRoots,
    pub(crate) live: Mutex<BTreeMap<String, LiveVault>>,
    pub(crate) exports: Mutex<crate::export::Exports>,
}
impl MemoryService {
    pub fn new(roots: VaultRoots) -> Self {
        Self { roots, live: Mutex::new(BTreeMap::new()), exports: Mutex::default() }
    }
    pub fn roots(&self) -> &VaultRoots {
        &self.roots
    }
    pub fn vault(
        &self,
        scope: MemoryScope,
        workspace: &MemoryWorkspace,
    ) -> MemoryResult<Arc<Vault>> {
        let (scope, root) = match scope {
            MemoryScope::Global => (VaultScope::Global, self.roots.global.clone()),
            MemoryScope::Project => (
                VaultScope::Project { workspace_id: workspace.workspace_id },
                VaultRoots::project(&workspace.root),
            ),
        };
        self.open_vault(scope, root)
    }
    pub fn global_vault(&self) -> MemoryResult<Arc<Vault>> {
        self.open_vault(VaultScope::Global, self.roots.global.clone())
    }
    fn open_vault(&self, scope: VaultScope, root: PathBuf) -> MemoryResult<Arc<Vault>> {
        let root = crate::roots::normalize_root(&root)?;
        let key = cache_key(&root);
        let mut live = self.live.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(v) = live.get(&key) {
            if v.vault.scope() != scope {
                return Err(MemoryError::Invalid("Vault already belongs to another scope"));
            }
            return Ok(v.vault.clone());
        }
        let vault = Vault::open(root, scope, &self.roots.cache.join(format!("{key}.db")))?;
        let watcher = VaultWatcher::start(&vault)?;
        live.insert(key, LiveVault { vault: vault.clone(), _watcher: watcher });
        Ok(vault)
    }
    pub fn selector_scope(
        &self,
        selector: &MemorySelector,
        workspace: &MemoryWorkspace,
    ) -> MemoryResult<MemoryScope> {
        match selector {
            MemorySelector::Path { scope, .. } => Ok(*scope),
            MemorySelector::Id { id } => self.id_scope(*id, workspace),
        }
    }
    fn id_scope(&self, id: MemoryId, workspace: &MemoryWorkspace) -> MemoryResult<MemoryScope> {
        let mut found = None;
        for scope in [MemoryScope::Project, MemoryScope::Global] {
            let vault = self.vault(scope, workspace)?;
            if vault.lock().index.path_for_id(&id.to_string())?.is_some() {
                if found.is_some() {
                    return Err(MemoryError::Invalid(
                        "ID exists in both Vaults; use scope and path",
                    ));
                }
                found = Some(scope);
            }
        }
        found.ok_or(MemoryError::NotFound)
    }
}

/// The name a Vault's index and records have in the cache directory.
pub(crate) fn cache_key(root: &std::path::Path) -> String {
    blake3::hash(root.display().to_string().to_lowercase().as_bytes()).to_hex().to_string()
}
