//! Pure batch preparation, shared by publication and consolidation preflight.
use super::*;
pub(super) struct Prepared {
    pub changes: Vec<VaultChange>,
    pub files: Vec<IndexedFile>,
    pub history: Vec<MemoryHistory>,
}
pub(super) fn prepare(
    vault: &Vault,
    state: &VaultState,
    writes: &[MemoryWrite],
    actor: &WriteActor,
) -> MemoryResult<Prepared> {
    if writes.is_empty() || writes.len() > MAX_BATCH {
        return Err(MemoryError::Invalid("batch size must be 1..100"));
    }
    if state.recovery_required {
        return Err(MemoryError::RecoveryRequired);
    }
    let mut expanded = writes.to_vec();
    let superseded = writes
        .iter()
        .flat_map(|w| w.note.metadata.supersedes_ids.iter().copied())
        .collect::<BTreeSet<_>>();
    for id in superseded {
        if let Some(old) = expanded.iter_mut().find(|w| w.note.metadata.id == id) {
            old.note.metadata.status = crate::MemoryStatus::Superseded;
            old.note.metadata.updated_at = UtcTimestamp::now();
        } else {
            let path = state
                .index
                .path_for_id(&id.to_string())?
                .ok_or(MemoryError::Invalid("superseded note missing or in another Vault"))?;
            let path =
                WorkspacePath::parse(path).map_err(|_| MemoryError::Invalid("indexed path"))?;
            let mut old = read_path(state, &path, vault.scope().scope())?;
            old.note.metadata.status = crate::MemoryStatus::Superseded;
            old.note.metadata.updated_at = UtcTimestamp::now();
            expanded.push(MemoryWrite { path, note: old.note, expected_hash: Some(old.hash) });
        }
    }
    if expanded.len() > MAX_BATCH {
        return Err(MemoryError::Invalid("batch including superseded notes exceeds 100"));
    }
    let writes = expanded.as_slice();
    let mut ids = BTreeSet::new();
    let mut paths = BTreeSet::new();
    let mut changes = vec![];
    let mut files = vec![];
    let mut history = vec![];
    for write in writes {
        if !ids.insert(write.note.metadata.id) || !paths.insert(write.path.as_str().to_lowercase())
        {
            return Err(MemoryError::Invalid("duplicate batch entry"));
        }
        if write.note.metadata.scope != vault.scope().scope()
            || write.path.is_root()
            || !write.path.as_str().to_ascii_lowercase().ends_with(".md")
            || write.path.as_str().eq_ignore_ascii_case("README.md")
        {
            return Err(MemoryError::Invalid("Vault path or scope"));
        }
        let text = crate::render(&write.note)?;
        if let Some(kind) = aura_secret_scanner::detect_text(&text) {
            return Err(MemoryError::Secret { kind });
        }
        let old = existing(state, &write.path, vault.scope().scope())?;
        let current = old.as_ref().map(|r| r.hash.clone());
        if current != write.expected_hash {
            return Err(MemoryError::Conflict {
                current_hash: current.unwrap_or_else(|| "absent".into()),
            });
        }
        if let Some(old) = &old {
            if old.note.metadata.id != write.note.metadata.id
                || old.note.metadata.created_at != write.note.metadata.created_at
            {
                return Err(MemoryError::Invalid("stable memory identity/creation time"));
            }
        } else if state.index.contains_id(&write.note.metadata.id.to_string())? {
            return Err(MemoryError::Invalid("memory id already exists"));
        }
        let hash = blake3::hash(text.as_bytes()).to_hex().to_string();
        files.push(IndexedFile {
            path: write.path.as_str().into(),
            hash: hash.clone(),
            note: Some(write.note.clone()),
            invalid_reason: None,
        });
        changes.push(VaultChange {
            path: write.path.clone(),
            expected_hash: write.expected_hash.clone(),
            bytes: Some(text.into_bytes()),
        });
        history.push(MemoryHistory {
            id: Some(write.note.metadata.id),
            path: write.path.as_str().into(),
            action: if old.is_some() { "update" } else { "create" }.into(),
            actor: actor.kind.clone(),
            session_id: actor.session_id,
            turn_id: actor.turn_id.clone(),
            before_hash: write.expected_hash.clone(),
            after_hash: Some(hash),
            title: write.note.title(),
            at: UtcTimestamp::now(),
        });
    }
    for write in writes {
        validate_relations(state, &write.note, &ids)?;
    }
    Ok(Prepared { changes, files, history })
}
