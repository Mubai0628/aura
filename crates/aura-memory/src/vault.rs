use crate::{MemoryError, MemoryHistory, MemoryResult, MemoryScope, VaultScope};
use aura_domain_common::{MemoryId, WorkspacePath};
use aura_fs_tools::vault::VaultFileSystem;
use aura_persistence_v2::memory::{
    MemoryIndex, MemorySearch, MemorySearchResult, VaultIndexStatus,
};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};

pub struct Vault {
    pub(crate) state: Mutex<VaultState>,
    root: PathBuf,
    scope: VaultScope,
    identity: String,
    cache: PathBuf,
    pub(crate) rebuilding: std::sync::atomic::AtomicBool,
}
pub(crate) struct VaultState {
    pub fs: VaultFileSystem,
    pub index: MemoryIndex,
    pub recovery_required: bool,
    pub audit: crate::audit::AuditLog,
    pub previews: std::collections::BTreeMap<String, crate::repair::PendingRepair>,
}
type Vaults = Mutex<HashMap<String, Weak<Vault>>>;
static VAULTS: OnceLock<Vaults> = OnceLock::new();

impl Vault {
    /// The caller resolves roots once and owns the cache directory. Neither this
    /// constructor nor reads, rebuilds or watchers create the Vault directory.
    pub fn open(root: PathBuf, scope: VaultScope, cache: &Path) -> MemoryResult<Arc<Self>> {
        if !cache.is_absolute() || !root.is_absolute() || cache.starts_with(&root) {
            return Err(MemoryError::Invalid("absolute cache outside Vault"));
        }
        let root = crate::roots::normalize_root(&root)?;
        let key = root.display().to_string().to_lowercase();
        let mut registry =
            VAULTS.get_or_init(Mutex::default).lock().unwrap_or_else(|e| e.into_inner());
        if let Some(vault) = registry.get(&key).and_then(Weak::upgrade) {
            if vault.scope != scope {
                return Err(MemoryError::Invalid("Vault already belongs to another scope"));
            }
            return Ok(vault);
        }
        if let Some(parent) = cache.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let fs = VaultFileSystem::new(root.clone())?;
        let identity = blake3::hash(key.as_bytes()).to_hex().to_string();
        let index = MemoryIndex::open(cache, &identity, scope)?;
        let audit = crate::audit::AuditLog::new(cache)?;
        let mut state = VaultState {
            fs,
            index,
            recovery_required: false,
            audit,
            previews: std::collections::BTreeMap::new(),
        };
        // An interrupted batch is settled before anything reads or indexes the files.
        crate::journal::recover(&mut state);
        let vault = Arc::new(Self {
            root,
            scope,
            identity,
            cache: cache.to_path_buf(),
            rebuilding: std::sync::atomic::AtomicBool::new(false),
            state: Mutex::new(state),
        });
        vault.rebuild()?;
        registry.insert(key, Arc::downgrade(&vault));
        Ok(vault)
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub const fn scope(&self) -> VaultScope {
        self.scope
    }
    pub fn identity(&self) -> &str {
        &self.identity
    }
    pub fn status(&self) -> MemoryResult<VaultIndexStatus> {
        Ok(self.lock().index.status()?)
    }
    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, VaultState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn search(&self, query: &MemorySearch) -> MemoryResult<MemorySearchResult> {
        if self.rebuilding.load(std::sync::atomic::Ordering::Acquire) {
            let mut result = MemoryIndex::open_reader(&self.cache)?.search(query)?;
            result.incomplete = true;
            return Ok(result);
        }
        Ok(self.lock().index.search(query)?)
    }
    pub fn recent(&self, limit: Option<u32>) -> MemoryResult<MemorySearchResult> {
        if self.rebuilding.load(std::sync::atomic::Ordering::Acquire) {
            let mut result = MemoryIndex::open_reader(&self.cache)?.recent(limit)?;
            result.incomplete = true;
            return Ok(result);
        }
        Ok(self.lock().index.recent(limit)?)
    }
    pub fn read_id(&self, id: MemoryId) -> MemoryResult<MemoryRead> {
        let state = self.lock();
        let path = state.index.path_for_id(&id.to_string())?.ok_or(MemoryError::NotFound)?;
        let read = read_path(
            &state,
            &WorkspacePath::parse(path).map_err(|_| MemoryError::Invalid("indexed path"))?,
            self.scope.scope(),
        )?;
        if read.note.metadata.id != id {
            return Err(MemoryError::NotFound);
        }
        Ok(read)
    }
    pub fn read_path(&self, path: &WorkspacePath) -> MemoryResult<MemoryRead> {
        read_path(&self.lock(), path, self.scope.scope())
    }
    pub fn history(&self, id: Option<MemoryId>, limit: u32) -> MemoryResult<Vec<MemoryHistory>> {
        Ok(self.lock().index.recent_history(id.map(|i| i.to_string()).as_deref(), limit)?)
    }
    pub fn validate(&self) -> MemoryResult<Vec<(String, String)>> {
        self.rebuild()?;
        Ok(self.lock().index.invalid_files()?)
    }
    pub fn validation_report(&self) -> MemoryResult<crate::MemoryValidation> {
        self.rebuild()?;
        let state = self.lock();
        // Validation counts well-formed files, including inactive notes. The
        // settings status separately counts notes eligible for default recall.
        Ok(crate::MemoryValidation {
            valid: state.index.valid_file_count()?,
            invalid: state
                .index
                .invalid_files()?
                .into_iter()
                .map(|(path, reason)| crate::MemoryInvalidFile { path, reason })
                .collect(),
            rebuilding: state.index.status()?.rebuilding,
        })
    }
}

pub use aura_memory_domain::MemoryRead;

pub(crate) fn read_path(
    state: &VaultState,
    path: &WorkspacePath,
    scope: MemoryScope,
) -> MemoryResult<MemoryRead> {
    let read = state.fs.read(path)?;
    let text =
        std::str::from_utf8(&read.bytes).map_err(|_| MemoryError::Invalid("UTF-8 Markdown"))?;
    let note = crate::parse(text)?;
    if note.metadata.scope != scope {
        return Err(MemoryError::Invalid("Vault scope"));
    }
    let history = state.index.recent_history(Some(&note.metadata.id.to_string()), 10)?;
    Ok(MemoryRead { path: path.clone(), note, hash: read.hash, history })
}
