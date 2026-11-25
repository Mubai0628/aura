use crate::{vault::read_path, *};
use aura_domain_common::{MemoryId, WorkspacePath};
use aura_persistence_v2::memory::MemoryIndex;
use std::{collections::BTreeSet, sync::atomic::Ordering};

impl Vault {
    pub fn timeline(&self, query: &MemoryTimelineQuery) -> MemoryResult<MemoryTimeline> {
        if query.since.zip(query.until).is_some_and(|(a, b)| a >= b)
            || query.limit.is_some_and(|n| !(1..=100).contains(&n))
        {
            return Err(MemoryError::Invalid("timeline time range or page size"));
        }
        let (mut entries, incomplete, can_resolve_names) =
            if self.rebuilding.load(Ordering::Acquire) {
                (MemoryIndex::open_reader(&self.cache)?.timeline(query)?, true, false)
            } else {
                let state = self.lock();
                let status = state.index.status()?;
                (
                    state.index.timeline(query)?,
                    status.rebuilding || status.watcher_error.is_some(),
                    true,
                )
            };
        let limit = query.limit.unwrap_or(30) as usize;
        let before = if entries.len() > limit {
            entries.truncate(limit);
            entries.last().map(|item| item.cursor.clone())
        } else {
            None
        };
        if can_resolve_names {
            for item in &mut entries {
                if item.event.session_id.is_none() {
                    continue;
                }
                let Some(note) = item.event.id.and_then(|id| self.read_id(id).ok()) else {
                    continue;
                };
                let Some(p) = note.provenance else {
                    continue;
                };
                item.session_name_at_write = p
                    .creator
                    .into_iter()
                    .chain(p.modifiers)
                    .find(|s| {
                        s.at == item.event.at
                            && s.session_id == item.event.session_id
                            && s.actor == item.event.actor
                    })
                    .and_then(|s| s.session_name_at_write);
            }
        }
        Ok(MemoryTimeline { entries, before, incomplete })
    }

    pub fn read_many(&self, ids: &[MemoryId]) -> MemoryResult<MemoryBatchRead> {
        if ids.is_empty()
            || ids.len() > MAX_BATCH
            || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
        {
            return Err(MemoryError::Invalid("batch size or duplicate memory IDs"));
        }
        let state = self.lock();
        let mut result = MemoryBatchRead { notes: Vec::new(), missing_ids: Vec::new() };
        let mut bytes = 0usize;
        for id in ids {
            let Some(path) = state.index.path_for_id(&id.to_string())? else {
                result.missing_ids.push(*id);
                continue;
            };
            let path =
                WorkspacePath::parse(path).map_err(|_| MemoryError::Invalid("indexed path"))?;
            match read_path(&state, &path, self.scope().scope()) {
                Ok(read) if read.note.metadata.id == *id => {
                    bytes = bytes.saturating_add(read.note.body.len());
                    if bytes > 2 * MAX_NOTE_BYTES {
                        return Err(MemoryError::Invalid("bulk output exceeds two MiB"));
                    }
                    result.notes.push(read);
                }
                Ok(_) | Err(MemoryError::NotFound | MemoryError::Format(_)) => {
                    result.missing_ids.push(*id)
                }
                Err(error) => return Err(error),
            }
        }
        Ok(result)
    }
}
