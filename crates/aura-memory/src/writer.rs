mod preparation;
use crate::vault::{VaultState, read_path};
use crate::{
    MAX_BATCH, MemoryDocument, MemoryError, MemoryHistory, MemoryResult, MemoryScope, Vault,
};
use aura_domain_common::{MemoryId, SessionId, UtcTimestamp, WorkspacePath};
use aura_fs_tools::vault::{VaultChange, VaultFileError};
use aura_persistence_v2::memory::IndexedFile;
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub struct MemoryWrite {
    pub path: WorkspacePath,
    pub note: MemoryDocument,
    pub expected_hash: Option<String>,
}
#[derive(Clone, Debug, Default)]
pub struct WriteActor {
    pub kind: String,
    pub session_id: Option<SessionId>,
    pub turn_id: Option<String>,
}

impl WriteActor {
    pub fn stamp(&self, note: &mut MemoryDocument) {
        if let Some(session) = self.session_id
            && !note.metadata.source_sessions.contains(&session)
        {
            note.metadata.source_sessions.push(session);
        }
        if let Some(turn) = &self.turn_id
            && !note.metadata.source_events.contains(turn)
        {
            note.metadata.source_events.push(turn.clone());
        }
    }
}

impl Vault {
    pub fn write(&self, writes: &[MemoryWrite], actor: &WriteActor) -> MemoryResult<Vec<String>> {
        if writes.is_empty() || writes.len() > MAX_BATCH {
            return Err(MemoryError::Invalid("batch size must be 1..100"));
        }
        let mut state = self.lock();
        if state.recovery_required {
            return Err(MemoryError::RecoveryRequired);
        }
        let requested = writes.len();
        let preparation::Prepared { mut changes, files, history } =
            preparation::prepare(self, &state, writes, actor)?;
        layout(&state, self.scope().scope(), &mut changes)?;
        crate::journal::publish(&mut state, &changes, &history)?;
        refresh_cache(&mut state, &files, &[], &history);
        Ok(files.into_iter().take(requested).map(|f| f.hash).collect())
    }
    pub fn validate_writes(&self, writes: &[MemoryWrite], actor: &WriteActor) -> MemoryResult<()> {
        preparation::prepare(self, &self.lock(), writes, actor).map(|_| ())
    }
    pub fn delete(&self, entries: &[(MemoryId, String)], actor: &WriteActor) -> MemoryResult<()> {
        if entries.is_empty() || entries.len() > MAX_BATCH {
            return Err(MemoryError::Invalid("delete batch size must be 1..100"));
        }
        let mut state = self.lock();
        if state.recovery_required {
            return Err(MemoryError::RecoveryRequired);
        }
        let mut ids = BTreeSet::new();
        let mut changes = vec![];
        let mut history = vec![];
        let mut paths = vec![];
        for (id, hash) in entries {
            if !ids.insert(*id) {
                return Err(MemoryError::Invalid("duplicate delete"));
            }
            let path = state.index.path_for_id(&id.to_string())?.ok_or(MemoryError::NotFound)?;
            let path =
                WorkspacePath::parse(&path).map_err(|_| MemoryError::Invalid("indexed path"))?;
            let read = read_path(&state, &path, self.scope().scope())?;
            if read.note.metadata.id != *id {
                return Err(MemoryError::NotFound);
            }
            if &read.hash != hash {
                return Err(MemoryError::Conflict { current_hash: read.hash });
            }
            changes.push(VaultChange {
                path: path.clone(),
                expected_hash: Some(hash.clone()),
                bytes: None,
            });
            paths.push(path.as_str().to_owned());
            history.push(MemoryHistory {
                id: Some(*id),
                path: path.as_str().into(),
                action: "delete".into(),
                actor: actor.kind.clone(),
                session_id: actor.session_id,
                turn_id: actor.turn_id.clone(),
                before_hash: Some(hash.clone()),
                after_hash: None,
                title: read.note.title(),
                at: UtcTimestamp::now(),
            });
        }
        crate::journal::publish(&mut state, &changes, &history)?;
        refresh_cache(&mut state, &[], &paths, &history);
        Ok(())
    }
}

/// Files are authoritative and the batch has committed. A derived cache failure
/// never turns that into a failed write; it is reported until a rebuild succeeds.
pub(crate) fn refresh_cache(
    state: &mut VaultState,
    files: &[IndexedFile],
    removed: &[String],
    history: &[MemoryHistory],
) {
    if state
        .index
        .update_files(files, removed)
        .and_then(|()| state.index.append_history(history))
        .is_err()
    {
        let _ = state.index.set_state("watcher_error:索引未能更新；请重建索引", None);
    }
}

/// The note text every new Vault starts with. It also marks the Vault as set up:
/// a Vault whose README exists never gets a removed `.gitignore` back.
pub(crate) fn readme(scope: MemoryScope) -> &'static str {
    match scope {
        MemoryScope::Global => {
            "# Aura 全局记忆\n\n这里的 Markdown 文件就是记忆本身，可以直接用 Obsidian 打开和编辑。每条记忆的 ID 写在文件开头，与文件名无关，改名或移动不会丢失。Aura 的索引随时可以从这些文件重建；记忆之间的链接只在本记忆库内有效。\n\n请保留这个说明文件，Aura 以它判断记忆库已经建好。\n"
        }
        MemoryScope::Project => {
            "# Aura 项目记忆\n\n这里的 Markdown 文件就是记忆本身，可以直接用 Obsidian 打开和编辑。每条记忆的 ID 写在文件开头，与文件名无关，改名或移动不会丢失。Aura 的索引随时可以从这些文件重建；记忆之间的链接只在本记忆库内有效。\n\n同目录的 .gitignore 让这些记忆默认不进入 Git。想把记忆提交到仓库时，删除那个 .gitignore 即可，Aura 不会再加回来，也从不修改仓库根目录的 .gitignore。\n\n请保留这个说明文件，Aura 以它判断记忆库已经建好。\n"
        }
    }
}

fn existing(
    state: &VaultState,
    path: &WorkspacePath,
    scope: MemoryScope,
) -> MemoryResult<Option<crate::MemoryRead>> {
    match read_path(state, path, scope) {
        Ok(r) => Ok(Some(r)),
        Err(MemoryError::File(VaultFileError::Io(e)))
            if e.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

fn validate_relations(
    state: &VaultState,
    note: &MemoryDocument,
    batch: &BTreeSet<MemoryId>,
) -> MemoryResult<()> {
    let m = &note.metadata;
    for id in m.related_ids.iter().chain(&m.supersedes_ids).chain(&m.conflicts_with_ids) {
        if !batch.contains(id) && state.index.path_for_id(&id.to_string())?.is_none() {
            return Err(MemoryError::Invalid("relation missing or in another Vault"));
        }
    }
    for link in note.body.split("[[").skip(1).filter_map(|s| s.split_once("]]").map(|(l, _)| l)) {
        let target = link.split(['|', '#']).next().unwrap_or_default();
        if !target.is_empty() && WorkspacePath::parse(target).is_err() {
            return Err(MemoryError::Invalid("cross-Vault or absolute wiki-link"));
        }
    }
    Ok(())
}

pub(crate) fn ensure_layout_directories(
    state: &VaultState,
    scope: MemoryScope,
) -> MemoryResult<()> {
    state.fs.ensure_directories(&["experience", "rules", "events/archive", "archive", "views"])?;
    state.fs.ensure_directories(match scope {
        MemoryScope::Global => &["user", "environment"],
        MemoryScope::Project => &[
            "architecture",
            "conventions",
            "decisions",
            "build",
            "commands",
            "modules",
            "known-issues",
        ],
    })?;
    Ok(())
}

fn layout(
    state: &VaultState,
    scope: MemoryScope,
    changes: &mut Vec<VaultChange>,
) -> MemoryResult<()> {
    // Deleting .gitignore is the documented way for a user to version a Vault, so
    // only a Vault without its README is set up. The README and .gitignore publish
    // in the same batch as the first note: an interrupted or failed first write
    // leaves neither, and the next write sets the Vault up again.
    for (name, bytes) in layout_files(state, scope)? {
        let path = WorkspacePath::parse(name).expect("layout leaf");
        changes.push(VaultChange { path, expected_hash: None, bytes: Some(bytes.to_vec()) });
    }
    if changes.iter().any(|c| c.path.as_str() == "README.md") {
        ensure_layout_directories(state, scope)?;
    }
    Ok(())
}

/// The layout files a Vault without README still needs. A `.gitignore` is only
/// added while the Vault holds no notes: a Vault someone filled before Aura's
/// first write may be versioned on purpose, and one with a README was set up.
pub(crate) fn layout_files(
    state: &VaultState,
    scope: MemoryScope,
) -> MemoryResult<Vec<(&'static str, &'static [u8])>> {
    if present(state, "README.md")? {
        return Ok(vec![]);
    }
    let mut files = vec![("README.md", readme(scope).as_bytes())];
    if scope == MemoryScope::Project
        && !present(state, ".gitignore")?
        && state.index.files()?.is_empty()
    {
        files.push((".gitignore", b"*\n".as_slice()));
    }
    Ok(files)
}
fn present(state: &VaultState, name: &str) -> MemoryResult<bool> {
    match state.fs.read(&WorkspacePath::parse(name).expect("layout leaf")) {
        Ok(_) => Ok(true),
        Err(VaultFileError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
