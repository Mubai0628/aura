//! Project event merges share the Vault writer and retry only pre-publication hash conflicts.
mod format;
use crate::*;
use aura_domain_common::{SessionId, UtcTimestamp, WorkspacePath};
use aura_fs_tools::vault::VaultFileError;
use format::{Block, parse_blocks, render_block};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct ProjectEvent {
    pub id: String,
    pub at: UtcTimestamp,
    pub title: String,
    pub summary: String,
    pub session_id: Option<SessionId>,
}
impl Vault {
    pub fn append_events(
        &self,
        events: &[ProjectEvent],
        retention_days: u32,
        actor: &WriteActor,
    ) -> MemoryResult<()> {
        if self.scope().scope() != MemoryScope::Project
            || !(1..=3650).contains(&retention_days)
            || events.len() > 20
        {
            return Err(MemoryError::Invalid("project events scope or retention bound"));
        }
        for event in events {
            aura_domain_common::ContentHash::new(
                aura_domain_common::ContentHashAlgorithm::Blake3,
                event.id.clone(),
            )
            .map_err(|_| MemoryError::Invalid("event identity"))?;
            if event.title.trim().is_empty()
                || event.title.chars().count() > 200
                || event.title.contains(['\r', '\n'])
                || event.summary.len() > 8000
            {
                return Err(MemoryError::Invalid("event content bound"));
            }
            if let Some(kind) =
                aura_secret_scanner::detect_text(&format!("{}\n{}", event.title, event.summary))
            {
                return Err(MemoryError::Secret { kind });
            }
        }
        for _ in 0..4 {
            let writes = self.prepare_events(events, retention_days, actor)?;
            if writes.is_empty() {
                return Ok(());
            }
            match self.write(&writes, actor) {
                Ok(_) => return Ok(()),
                Err(MemoryError::Conflict { .. }) => continue,
                Err(error) => return Err(error),
            }
        }
        Err(MemoryError::Invalid("event merge remained busy; no automatic replay"))
    }
    pub(crate) fn prepare_events(
        &self,
        events: &[ProjectEvent],
        retention_days: u32,
        actor: &WriteActor,
    ) -> MemoryResult<Vec<MemoryWrite>> {
        let recent = WorkspacePath::parse("events/recent.md").expect("fixed path");
        let old = optional(self, &recent)?;
        if old.is_none() && events.is_empty() {
            return Ok(vec![]);
        }
        let (prefix, mut blocks) =
            parse_blocks(old.as_ref().map_or("# 最近项目事件\n\n", |r| r.note.body.as_str()))?;
        for event in events {
            if blocks.iter().all(|b| b.id != event.id) {
                blocks.push(render_block(event)?);
            }
        }
        blocks.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.id.cmp(&b.id)));
        let cutoff = UtcTimestamp::now().value() - time::Duration::days(i64::from(retention_days));
        let mut recent_body = prefix;
        let mut archives: BTreeMap<String, Vec<Block>> = BTreeMap::new();
        for block in blocks {
            if block.at.value() < cutoff {
                let at = block.at.value();
                archives
                    .entry(format!(
                        "events/archive/{:04}-{:02}.md",
                        at.year(),
                        u8::from(at.month())
                    ))
                    .or_default()
                    .push(block);
            } else {
                recent_body.push_str(&block.text);
            }
        }
        let mut writes = vec![];
        if old.as_ref().is_none_or(|r| r.note.body != recent_body) {
            writes.push(event_write(recent, old, recent_body, false, actor));
        }
        for (name, moved) in archives {
            let path =
                WorkspacePath::parse(name).map_err(|_| MemoryError::Invalid("archive path"))?;
            let old = optional(self, &path)?;
            let (prefix, mut existing) =
                parse_blocks(old.as_ref().map_or("# 项目事件归档\n\n", |r| r.note.body.as_str()))?;
            for block in moved {
                if existing.iter().all(|b| b.id != block.id) {
                    existing.push(block);
                }
            }
            existing.sort_by(|a, b| a.at.cmp(&b.at).then_with(|| a.id.cmp(&b.id)));
            let body = prefix + &existing.into_iter().map(|b| b.text).collect::<String>();
            if old.as_ref().is_none_or(|r| r.note.body != body) {
                writes.push(event_write(path, old, body, true, actor));
            }
        }
        Ok(writes)
    }
}
fn optional(vault: &Vault, path: &WorkspacePath) -> MemoryResult<Option<MemoryRead>> {
    match vault.read_path(path) {
        Ok(read) => Ok(Some(read)),
        Err(MemoryError::File(VaultFileError::Io(e)))
            if e.kind() == std::io::ErrorKind::NotFound =>
        {
            Ok(None)
        }
        Err(error) => Err(error),
    }
}
fn event_write(
    path: WorkspacePath,
    old: Option<MemoryRead>,
    body: String,
    archived: bool,
    actor: &WriteActor,
) -> MemoryWrite {
    let expected_hash = old.as_ref().map(|r| r.hash.clone());
    let mut note = old.map_or_else(
        || MemoryDocument {
            metadata: MemoryMetadata::new(MemoryScope::Project, "events".into()),
            body: String::new(),
        },
        |r| r.note,
    );
    note.body = body;
    note.metadata.updated_at = UtcTimestamp::now();
    if archived {
        note.metadata.status = MemoryStatus::Archived;
    }
    actor.stamp(&mut note);
    MemoryWrite { path, note, expected_hash }
}
