//! Durable multi-file publication for one Vault.
//!
//! Vault files stay the only authority. This journal exists so that a batch which
//! changes several files is all or nothing even when the process ends between two
//! of them. Before the first file changes, the prior bytes of every target and a
//! manifest are written beside the derived cache, outside the Vault. The batch
//! commits when its manifest is marked committed. Until then a failure, or the next
//! start, restores every file the batch had already changed and only those: a file
//! someone else changed in the meantime is kept and recorded, never overwritten.
//! A committed journal only repeats its metadata history, which is deduplicated.
//! Prior bytes stay here only until the batch settles.
use crate::{MemoryError, MemoryHistory, MemoryResult, audit::AuditLog, vault::VaultState};
use aura_domain_common::{MemoryId, UtcTimestamp, WorkspacePath};
use aura_fs_tools::vault::{VaultChange, VaultFileError, VaultFileSystem, VaultRead};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const DIRECTORY: &str = "pending";
/// Shown in the settings page while writes are stopped for an unsettled batch.
pub(crate) const NOTICE: &str =
    "watcher_error:上次中断的记忆写入还没能撤回，写入已暂停；重启 Aura 后会自动再试";
/// One batch at a time per Vault; more than this is a damaged journal, not work.
const MAX_PENDING: usize = 64;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    batch: String,
    committed: bool,
    entries: Vec<Entry>,
    history: Vec<MemoryHistory>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: WorkspacePath,
    before_hash: Option<String>,
    after_hash: Option<String>,
    /// Journal-relative copy of the prior bytes; absent when the file did not exist.
    image: Option<WorkspacePath>,
}
struct Journal {
    manifest: Manifest,
    path: WorkspacePath,
    hash: String,
    images: Vec<Option<Vec<u8>>>,
}
#[derive(Default)]
struct Undone {
    /// Each restored entry with the hash it held before the restore (None: absent).
    restored: Vec<(Entry, Option<String>)>,
    kept: Vec<Entry>,
}

/// Publish a prepared batch under the Vault's single writer. On success every
/// change is in place. On error nothing of the batch remains, except when that
/// cannot be confirmed: then the error is `RecoveryRequired`, writes stop and the
/// next start finishes the undo from the journal.
pub(crate) fn publish(
    state: &mut VaultState,
    changes: &[VaultChange],
    history: &[MemoryHistory],
) -> MemoryResult<()> {
    if state.recovery_required {
        return Err(MemoryError::RecoveryRequired);
    }
    if changes.is_empty() {
        return Ok(());
    }
    let batch = MemoryId::new().to_string();
    let mut entries = Vec::with_capacity(changes.len());
    let mut images = Vec::with_capacity(changes.len());
    let mut seen = BTreeSet::new();
    for (index, change) in changes.iter().enumerate() {
        if !seen.insert(change.path.as_str().to_lowercase()) {
            return Err(MemoryError::Invalid("duplicate batch path"));
        }
        let before = current(&state.fs, &change.path)?;
        let before_hash = before.as_ref().map(|r| r.hash.clone());
        if before_hash != change.expected_hash {
            return Err(conflict(before_hash));
        }
        entries.push(Entry {
            path: change.path.clone(),
            before_hash,
            after_hash: change.bytes.as_ref().map(|b| blake3::hash(b).to_hex().to_string()),
            image: before.as_ref().map(|_| image_path(&batch, index)).transpose()?,
        });
        images.push(before.map(|r| r.bytes));
    }
    let mut journal = Journal::begin(&state.audit, batch, entries, images, history)?;
    for (done, change) in changes.iter().enumerate() {
        crash::point(crash::Stage::Published(done))?;
        if let Err(error) = state.fs.apply(std::slice::from_ref(change)) {
            // The failed publication itself may or may not have landed.
            return Err(settle_failure(state, &journal, done + 1, error.into()));
        }
    }
    crash::point(crash::Stage::Published(changes.len()))?;
    if let Err(error) = journal.commit(&state.audit) {
        return Err(settle_failure(state, &journal, changes.len(), error));
    }
    crash::point(crash::Stage::Committed)?;
    // Repeated from the committed journal at the next open if it does not complete.
    if state.audit.append(history).is_ok() {
        crash::point(crash::Stage::Recorded)?;
        let _ = journal.discard(&state.audit);
    }
    Ok(())
}

/// Undo what this batch changed, then report the original failure. A conflict
/// stays a conflict so the caller can reread; other causes say that the files
/// were restored. If the undo cannot be confirmed, writes stop until a restart.
fn settle_failure(
    state: &mut VaultState,
    journal: &Journal,
    touched: usize,
    error: MemoryError,
) -> MemoryError {
    let entries = &journal.manifest.entries[..touched];
    match undo(&state.fs, entries, |index| Ok(journal.images[index].clone())) {
        Ok(_) => {
            let _ = journal.discard(&state.audit);
            match error {
                MemoryError::File(VaultFileError::Conflict { current_hash }) => {
                    conflict(current_hash)
                }
                MemoryError::Conflict { .. } => error,
                other => MemoryError::RolledBack { cause: other.to_string() },
            }
        }
        Err(_) => {
            state.recovery_required = true;
            let _ = state.index.set_state(NOTICE, None);
            MemoryError::RecoveryRequired
        }
    }
}

/// Settle every journal an earlier process left, before the Vault is indexed. A
/// journal that cannot be settled keeps writes stopped; reads stay available.
pub(crate) fn recover(state: &mut VaultState) {
    if recover_all(state).is_err() {
        state.recovery_required = true;
    }
}

fn recover_all(state: &VaultState) -> MemoryResult<()> {
    let root = state.audit.fs.root().join(DIRECTORY);
    if !root.try_exists()? {
        return Ok(());
    }
    let (mut manifests, mut images) = (vec![], vec![]);
    for item in std::fs::read_dir(root)? {
        let name = item?.file_name().to_string_lossy().into_owned();
        let path = journal_path(&name)?;
        if name.ends_with(".json") {
            manifests.push(path);
        } else if name.ends_with(".before") {
            images.push(path);
        }
        if manifests.len() > MAX_PENDING || images.len() > MAX_PENDING * crate::MAX_BATCH {
            return Err(MemoryError::Invalid("memory journal bound exceeded"));
        }
    }
    // Batch IDs sort by time: settle the newest first, as an undo would.
    manifests.sort();
    for path in manifests.into_iter().rev() {
        let manifest: Manifest = serde_json::from_slice(&state.audit.fs.read(&path)?.bytes)
            .map_err(|_| MemoryError::Invalid("memory journal manifest"))?;
        if manifest.schema_version != 1 || manifest_path(&manifest.batch)? != path {
            return Err(MemoryError::Invalid("memory journal manifest"));
        }
        if manifest.committed {
            state.audit.append(&manifest.history)?;
        } else {
            let undone = undo(&state.fs, &manifest.entries, |index| {
                manifest.entries[index]
                    .image
                    .as_ref()
                    .map(|image| state.audit.fs.read(image).map(|r| r.bytes))
                    .transpose()
                    .map_err(Into::into)
            })?;
            state.audit.append(&recovery_history(&manifest, &undone))?;
        }
        remove(&state.audit, &path)?;
        for image in manifest.entries.iter().filter_map(|e| e.image.as_ref()) {
            remove(&state.audit, image)?;
        }
    }
    // Copies whose manifest was never written, or was removed first while settling.
    for image in images {
        remove(&state.audit, &image)?;
    }
    Ok(())
}

impl Journal {
    fn begin(
        audit: &AuditLog,
        batch: String,
        entries: Vec<Entry>,
        images: Vec<Option<Vec<u8>>>,
        history: &[MemoryHistory],
    ) -> MemoryResult<Self> {
        let manifest = Manifest {
            schema_version: 1,
            batch,
            committed: false,
            entries,
            history: history.to_vec(),
        };
        let mut written = vec![];
        let result = (|| {
            for (entry, bytes) in manifest.entries.iter().zip(&images) {
                if let (Some(path), Some(bytes)) = (&entry.image, bytes) {
                    create(audit, path, bytes.clone())?;
                    written.push(path.clone());
                }
            }
            let bytes = encode(&manifest)?;
            let path = manifest_path(&manifest.batch)?;
            create(audit, &path, bytes.clone())?;
            Ok((path, blake3::hash(&bytes).to_hex().to_string()))
        })();
        match result {
            Ok((path, hash)) => Ok(Self { manifest, path, hash, images }),
            Err(error) => {
                // Nothing in the Vault changed yet; leftovers would be swept at the next open.
                for path in written {
                    let _ = remove(audit, &path);
                }
                Err(error)
            }
        }
    }
    fn commit(&mut self, audit: &AuditLog) -> MemoryResult<()> {
        let mut manifest = self.manifest.clone();
        manifest.committed = true;
        let bytes = encode(&manifest)?;
        audit.fs.apply(&[VaultChange {
            path: self.path.clone(),
            expected_hash: Some(self.hash.clone()),
            bytes: Some(bytes.clone()),
        }])?;
        self.manifest = manifest;
        self.hash = blake3::hash(&bytes).to_hex().to_string();
        Ok(())
    }
    /// The manifest goes first: copies without one are swept, never replayed.
    fn discard(&self, audit: &AuditLog) -> MemoryResult<()> {
        remove(audit, &self.path)?;
        for image in self.manifest.entries.iter().filter_map(|e| e.image.as_ref()) {
            remove(audit, image)?;
        }
        Ok(())
    }
}

/// Restore, newest first, every listed file that still holds this batch's bytes.
fn undo(
    fs: &VaultFileSystem,
    entries: &[Entry],
    image: impl Fn(usize) -> MemoryResult<Option<Vec<u8>>>,
) -> MemoryResult<Undone> {
    let mut undone = Undone::default();
    for (index, entry) in entries.iter().enumerate().rev() {
        let now = current(fs, &entry.path)?.map(|r| r.hash);
        if now == entry.before_hash {
            continue;
        }
        // A replacement moves the old file aside before the new one takes its
        // name; a process ended between the two leaves the name absent. That is
        // this batch's doing, so an absent file that had prior bytes is restored.
        if now != entry.after_hash && !(now.is_none() && entry.image.is_some()) {
            undone.kept.push(entry.clone());
            continue;
        }
        let bytes = match &entry.image {
            Some(_) => {
                let bytes = image(index)?.ok_or(MemoryError::Invalid("memory journal copy"))?;
                if Some(blake3::hash(&bytes).to_hex().to_string()) != entry.before_hash {
                    return Err(MemoryError::Invalid("memory journal copy hash"));
                }
                Some(bytes)
            }
            None => None,
        };
        fs.apply(&[VaultChange { path: entry.path.clone(), expected_hash: now.clone(), bytes }])?;
        undone.restored.push((entry.clone(), now));
    }
    Ok(undone)
}

fn recovery_history(manifest: &Manifest, undone: &Undone) -> Vec<MemoryHistory> {
    let at = UtcTimestamp::now();
    let record = |entry: &Entry, action: &str, before: Option<String>, after: Option<String>| {
        let source = manifest.history.iter().find(|h| h.path == entry.path.as_str());
        MemoryHistory {
            id: source.and_then(|h| h.id),
            path: entry.path.as_str().into(),
            action: action.into(),
            actor: "recovery".into(),
            session_id: source.and_then(|h| h.session_id),
            turn_id: source.and_then(|h| h.turn_id.clone()),
            before_hash: before,
            after_hash: after,
            title: source.map_or_else(String::new, |h| h.title.clone()),
            at,
        }
    };
    let restored = undone
        .restored
        .iter()
        .map(|(e, held)| record(e, "interrupted_undo", held.clone(), e.before_hash.clone()));
    // Changed by someone else after the interruption: their bytes are kept.
    let kept = undone.kept.iter().map(|e| record(e, "interrupted_kept", None, None));
    restored.chain(kept).collect()
}

fn current(fs: &VaultFileSystem, path: &WorkspacePath) -> MemoryResult<Option<VaultRead>> {
    match fs.read(path) {
        Ok(read) => Ok(Some(read)),
        Err(VaultFileError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn create(audit: &AuditLog, path: &WorkspacePath, bytes: Vec<u8>) -> MemoryResult<()> {
    audit.fs.apply(&[VaultChange {
        path: path.clone(),
        expected_hash: None,
        bytes: Some(bytes),
    }])?;
    Ok(())
}
fn remove(audit: &AuditLog, path: &WorkspacePath) -> MemoryResult<()> {
    if let Some(read) = current(&audit.fs, path)? {
        audit.fs.apply(&[VaultChange {
            path: path.clone(),
            expected_hash: Some(read.hash),
            bytes: None,
        }])?;
    }
    Ok(())
}
fn conflict(current_hash: Option<String>) -> MemoryError {
    MemoryError::Conflict { current_hash: current_hash.unwrap_or_else(|| "absent".into()) }
}
fn encode(manifest: &Manifest) -> MemoryResult<Vec<u8>> {
    serde_json::to_vec(manifest).map_err(|_| MemoryError::Invalid("memory journal manifest"))
}
fn journal_path(name: &str) -> MemoryResult<WorkspacePath> {
    WorkspacePath::parse(format!("{DIRECTORY}/{name}"))
        .map_err(|_| MemoryError::Invalid("memory journal path"))
}
fn manifest_path(batch: &str) -> MemoryResult<WorkspacePath> {
    journal_path(&format!("{batch}.json"))
}
fn image_path(batch: &str, index: usize) -> MemoryResult<WorkspacePath> {
    journal_path(&format!("{batch}-{index}.before"))
}

#[cfg(test)]
pub(crate) mod crash {
    //! Ends a publication at a stage exactly as a terminated process would: no
    //! undo, no discard, nothing after it runs.
    use std::cell::Cell;
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub(crate) enum Stage {
        Published(usize),
        Committed,
        Recorded,
    }
    thread_local!(static AT: Cell<Option<Stage>> = const { Cell::new(None) });
    pub(crate) fn arm(stage: Stage) {
        AT.with(|at| at.set(Some(stage)));
    }
    pub(super) fn point(stage: Stage) -> crate::MemoryResult<()> {
        if AT.with(|at| at.get()) == Some(stage) {
            AT.with(|at| at.set(None));
            return Err(crate::MemoryError::Invalid("simulated end of process"));
        }
        Ok(())
    }
}
#[cfg(not(test))]
mod crash {
    pub(super) enum Stage {
        Published(#[allow(dead_code)] usize),
        Committed,
        Recorded,
    }
    #[inline(always)]
    pub(super) fn point(_: Stage) -> crate::MemoryResult<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
