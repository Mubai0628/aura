//! Structured consolidation is validated in full before any Vault file changes.
use crate::*;
use aura_domain_common::{ContentHash, ContentHashAlgorithm, MemoryId, UtcTimestamp};
use std::sync::Arc;

struct PreparedScope {
    vault: Arc<Vault>,
    writes: Vec<MemoryWrite>,
}

pub fn validate_maintenance_output(output: &MemoryMaintenanceOutput) -> MemoryResult<()> {
    if output.schema_version != 1 || output.candidates.len() > 20 || output.events.len() > 20 {
        return Err(MemoryError::Invalid("maintenance schema version or candidate bound"));
    }
    let wire =
        serde_json::to_string(output).map_err(|_| MemoryError::Invalid("maintenance contract"))?;
    if wire.len() > 512 * 1024 {
        return Err(MemoryError::Invalid("maintenance output exceeds 512 KiB"));
    }
    if let Some(kind) = aura_secret_scanner::detect_text(&wire) {
        return Err(MemoryError::Secret { kind });
    }
    for candidate in &output.candidates {
        if let Some(content) = candidate.content() {
            if content.body.is_empty()
                || content.body.len() > 64 * 1024
                || content.basis.trim().is_empty()
                || content.basis.len() > 2000
                || content.tags.len() > 32
            {
                return Err(MemoryError::Invalid("maintenance content or basis bound"));
            }
            let note = make_note(candidate.scope(), content, &WriteActor::default());
            crate::render(&note)?;
        } else if let MemoryCandidate::Ignore { basis, .. } = candidate
            && (basis.trim().is_empty() || basis.len() > 2000)
        {
            return Err(MemoryError::Invalid("ignore basis"));
        }
        if let MemoryCandidate::Update { expected_hash, .. }
        | MemoryCandidate::Supersede { expected_hash, .. }
        | MemoryCandidate::Conflict { expected_hash, .. } = candidate
        {
            ContentHash::new(ContentHashAlgorithm::Blake3, expected_hash.clone())
                .map_err(|_| MemoryError::Invalid("maintenance expected_hash"))?;
        }
    }
    for event in &output.events {
        if event.title.trim().is_empty()
            || event.title.chars().count() > 200
            || event.title.contains(['\r', '\n'])
            || event.summary.trim().is_empty()
            || event.summary.len() > 8000
            || event.basis.trim().is_empty()
            || event.basis.len() > 2000
        {
            return Err(MemoryError::Invalid("maintenance event or basis bound"));
        }
    }
    Ok(())
}

impl MemoryService {
    pub fn apply_maintenance(
        &self,
        output: &MemoryMaintenanceOutput,
        workspace: &MemoryWorkspace,
        actor: &WriteActor,
        run_id: &str,
        retention_days: u32,
        may_write: impl Fn() -> bool,
    ) -> MemoryResult<MemoryMaintenanceResult> {
        validate_maintenance_output(output)?;
        if !(1..=3650).contains(&retention_days) {
            return Err(MemoryError::Invalid("event retention bound"));
        }
        let mut result = MemoryMaintenanceResult::default();
        let mut prepared = vec![];
        for scope in [MemoryScope::Global, MemoryScope::Project] {
            let candidates =
                output.candidates.iter().filter(|c| c.scope() == scope).collect::<Vec<_>>();
            if candidates.is_empty() {
                continue;
            }
            let vault = self.vault(scope, workspace)?;
            let mut writes: Vec<MemoryWrite> = vec![];
            for candidate in candidates {
                let Some(content) = candidate.content() else {
                    result.ignored += 1;
                    continue;
                };
                match candidate {
                    MemoryCandidate::Create { path, .. } => {
                        if duplicate(&vault, content, &writes)? {
                            result.ignored += 1;
                            continue;
                        }
                        writes.push(MemoryWrite {
                            path: path.clone(),
                            note: make_note(scope, content, actor),
                            expected_hash: None,
                        });
                        result.created += 1;
                    }
                    MemoryCandidate::Update { target, expected_hash, .. } => {
                        let mut old = checked(&vault, *target, expected_hash)?;
                        if old.note.body == content.body
                            && old.note.metadata.category == content.category
                            && content.confidence.is_none()
                            && content.importance.is_none()
                            && old.note.metadata.tags == content.tags
                        {
                            result.ignored += 1;
                            continue;
                        }
                        patch(&mut old.note, content, actor);
                        writes.push(MemoryWrite {
                            path: old.path,
                            note: old.note,
                            expected_hash: Some(old.hash),
                        });
                        result.updated += 1;
                    }
                    MemoryCandidate::Supersede { target, expected_hash, path, .. }
                    | MemoryCandidate::Conflict { target, expected_hash, path, .. } => {
                        let mut old = checked(&vault, *target, expected_hash)?;
                        let mut note = make_note(scope, content, actor);
                        if matches!(candidate, MemoryCandidate::Supersede { .. }) {
                            note.metadata.supersedes_ids.push(*target);
                            old.note.metadata.status = MemoryStatus::Superseded;
                        } else {
                            note.metadata.status = MemoryStatus::Conflicted;
                            note.metadata.conflicts_with_ids.push(*target);
                            old.note.metadata.status = MemoryStatus::Conflicted;
                            old.note.metadata.conflicts_with_ids.push(note.metadata.id);
                        }
                        old.note.metadata.updated_at = UtcTimestamp::now();
                        actor.stamp(&mut old.note);
                        writes.push(MemoryWrite {
                            path: old.path,
                            note: old.note,
                            expected_hash: Some(old.hash),
                        });
                        writes.push(MemoryWrite { path: path.clone(), note, expected_hash: None });
                        result.created += 1;
                        result.updated += 1;
                    }
                    MemoryCandidate::Ignore { .. } => unreachable!(),
                }
            }
            if !writes.is_empty() {
                prepared.push(PreparedScope { vault, writes });
            }
        }
        // Apply retention even when this response contains no new event. An
        // absent recent file with no events remains absent.
        {
            let events = output
                .events
                .iter()
                .enumerate()
                .map(|(i, event)| crate::events::ProjectEvent {
                    id: blake3::hash(format!("{run_id}:{i}").as_bytes()).to_hex().to_string(),
                    at: UtcTimestamp::now(),
                    title: event.title.clone(),
                    summary: event.summary.clone(),
                    session_id: actor.session_id,
                })
                .collect::<Vec<_>>();
            let vault = self.vault(MemoryScope::Project, workspace)?;
            let writes = vault.prepare_events(&events, retention_days, actor)?;
            if let Some(project) =
                prepared.iter_mut().find(|p| p.vault.scope().scope() == MemoryScope::Project)
            {
                project.writes.extend(writes);
            } else if !writes.is_empty() {
                prepared.push(PreparedScope { vault, writes });
            }
            result.events = events.len() as u32;
        }
        for scope in &prepared {
            scope.vault.validate_writes(&scope.writes, actor)?;
        }
        for scope in prepared {
            if !may_write() {
                return Err(MemoryError::Invalid("只读会话未整理"));
            }
            scope.vault.write(&scope.writes, actor)?;
        }
        Ok(result)
    }
}

fn checked(vault: &Vault, id: MemoryId, expected_hash: &str) -> MemoryResult<MemoryRead> {
    let old = vault.read_id(id)?;
    if old.hash != expected_hash {
        return Err(MemoryError::Conflict { current_hash: old.hash });
    }
    Ok(old)
}
fn duplicate(
    vault: &Vault,
    content: &MemoryCandidateContent,
    pending: &[MemoryWrite],
) -> MemoryResult<bool> {
    if pending.iter().any(|w| w.note.body == content.body) {
        return Ok(true);
    }
    let title = content.body.lines().find_map(|l| l.strip_prefix("# ")).unwrap_or("");
    if title.is_empty() {
        return Ok(false);
    }
    for hit in vault
        .search(&MemorySearch { query: title.into(), limit: Some(100), ..Default::default() })?
        .hits
    {
        if vault.read_id(hit.metadata.id)?.note.body == content.body {
            return Ok(true);
        }
    }
    Ok(false)
}
fn make_note(
    scope: MemoryScope,
    content: &MemoryCandidateContent,
    actor: &WriteActor,
) -> MemoryDocument {
    let mut note = MemoryDocument {
        metadata: MemoryMetadata::new(scope, content.category.clone()),
        body: content.body.clone(),
    };
    patch(&mut note, content, actor);
    note
}
fn patch(note: &mut MemoryDocument, content: &MemoryCandidateContent, actor: &WriteActor) {
    note.body = content.body.clone();
    note.metadata.category = content.category.clone();
    note.metadata.tags = content.tags.clone();
    // A value outside its range is not used: one stray field must not discard the whole
    // organization.
    if let Some(v) = content.confidence.filter(|v| v.is_finite() && (0.0..=1.0).contains(v)) {
        note.metadata.confidence = v;
    }
    if let Some(v) = content.importance.filter(|v| (1..=5).contains(v)) {
        note.metadata.importance = v;
    }
    note.metadata.updated_at = UtcTimestamp::now();
    actor.stamp(note);
}
