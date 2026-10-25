use crate::vault::VaultState;
use crate::{MemoryHistory, MemoryResult, MemoryScope, Vault};
use aura_domain_common::{UtcTimestamp, WorkspacePath};
use aura_persistence_v2::memory::IndexedFile;
use std::collections::BTreeMap;

const MAX_FILES: usize = 10000;
impl Vault {
    pub fn rebuild(&self) -> MemoryResult<()> {
        use std::sync::atomic::Ordering;
        if self.rebuilding.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        struct Reset<'a>(&'a std::sync::atomic::AtomicBool);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Release);
            }
        }
        let _reset = Reset(&self.rebuilding);
        let mut state = self.lock();
        state.index.set_state("rebuilding", None)?;
        let result = (|| {
            let paths = state.fs.list_markdown(MAX_FILES)?;
            let old = state.index.files()?.into_iter().collect::<BTreeMap<_, _>>();
            let mut files = Vec::with_capacity(paths.len());
            for path in paths {
                files.push(index_file(&state, &path, self.scope().scope()));
            }
            let present =
                files.iter().map(|f| f.path.as_str()).collect::<std::collections::BTreeSet<_>>();
            let removed =
                old.keys().filter(|p| !present.contains(p.as_str())).cloned().collect::<Vec<_>>();
            let records = state.audit.all()?;
            let history = external_history(&old, &files, &removed, &records);
            state.audit.append(&history)?;
            state.index.replace_all(&files)?;
            state.index.append_history(&records)?;
            state.index.append_history(&history)?;
            // Run records only feed the settings list; they never block the index.
            if let Ok(runs) = state.audit.runs() {
                state.index.replace_maintenance_runs(&runs)?;
            }
            let identity = if state.fs.exists()? {
                let root = state.fs.identity()?;
                Some(serde_json::json!({"volume_serial_number":root.volume_serial_number.to_string(),"file_id":root.file_id.iter().map(|b|format!("{b:02x}")).collect::<String>()}).to_string())
            } else {
                None
            };
            // Writes stay stopped until an interrupted batch is settled; say so.
            let ready = if state.recovery_required { crate::journal::NOTICE } else { "ready" };
            state.index.set_state(ready, identity.as_deref())?;
            Ok(())
        })();
        if result.is_err() {
            state.index.set_state("watcher_error:索引重建失败；请校验后重建", None)?;
        }
        result
    }
    pub(crate) fn refresh_paths(&self, paths: &[WorkspacePath]) -> MemoryResult<()> {
        let mut state = self.lock();
        let old = state.index.files()?.into_iter().collect::<BTreeMap<_, _>>();
        let mut files = vec![];
        let mut removed = vec![];
        for path in paths {
            match state.fs.root().join(path.as_str()).try_exists() {
                Ok(false) => removed.push(path.as_str().to_owned()),
                _ => files.push(index_file(&state, path, self.scope().scope())),
            }
        }
        let touched = paths.iter().map(|p| p.as_str().to_owned()).collect::<Vec<_>>();
        let latest = state.index.latest_history_for(&touched)?;
        let history = external_history(&old, &files, &removed, &latest);
        state.audit.append(&history)?;
        state.index.update_files(&files, &removed)?;
        state.index.append_history(&history)?;
        Ok(())
    }
}

pub(crate) fn index_file(
    state: &VaultState,
    path: &WorkspacePath,
    scope: MemoryScope,
) -> IndexedFile {
    let read = match state.fs.read(path) {
        Ok(r) => r,
        Err(_) => {
            return IndexedFile {
                path: path.as_str().into(),
                hash: String::new(),
                note: None,
                invalid_reason: Some("unsafe, missing or unreadable file".into()),
            };
        }
    };
    let note = std::str::from_utf8(&read.bytes).ok().and_then(|s| crate::parse(s).ok());
    match note {
        Some(note) if note.metadata.scope == scope => IndexedFile {
            path: path.as_str().into(),
            hash: read.hash,
            note: Some(note),
            invalid_reason: None,
        },
        _ => IndexedFile {
            path: path.as_str().into(),
            hash: read.hash,
            note: None,
            invalid_reason: Some("invalid UTF-8, frontmatter or scope".into()),
        },
    }
}

fn external_history(
    old: &BTreeMap<String, String>,
    files: &[IndexedFile],
    removed: &[String],
    known: &[MemoryHistory],
) -> Vec<MemoryHistory> {
    let latest = known.iter().map(|h| (h.path.as_str(), h)).collect::<BTreeMap<_, _>>();
    let mut changes = vec![];
    for file in files {
        if old.get(&file.path) == Some(&file.hash)
            || latest
                .get(file.path.as_str())
                .is_some_and(|h| h.after_hash.as_ref() == Some(&file.hash))
        {
            continue;
        }
        let previous = latest.get(file.path.as_str());
        let title = file
            .note
            .as_ref()
            .map_or_else(|| previous.map_or_else(String::new, |h| h.title.clone()), |n| n.title());
        changes.push(MemoryHistory {
            id: file.note.as_ref().map(|n| n.metadata.id).or_else(|| previous.and_then(|h| h.id)),
            path: file.path.clone(),
            action: if old.contains_key(&file.path) { "external_edit" } else { "external_create" }
                .into(),
            actor: "external".into(),
            session_id: None,
            turn_id: None,
            before_hash: old.get(&file.path).cloned(),
            after_hash: Some(file.hash.clone()),
            title: safe_title(title),
            at: UtcTimestamp::now(),
        });
    }
    for path in removed {
        let previous = latest.get(path.as_str());
        if !old.contains_key(path) || previous.is_some_and(|h| h.after_hash.is_none()) {
            continue;
        }
        changes.push(MemoryHistory {
            id: previous.and_then(|h| h.id),
            path: path.clone(),
            action: "external_delete".into(),
            actor: "external".into(),
            session_id: None,
            turn_id: None,
            before_hash: old.get(path).cloned(),
            after_hash: None,
            title: safe_title(previous.map_or_else(String::new, |h| h.title.clone())),
            at: UtcTimestamp::now(),
        });
    }
    changes
}
fn safe_title(title: String) -> String {
    if aura_secret_scanner::detect_text(&title).is_some() {
        "<含密钥的标题>".into()
    } else {
        title
    }
}
