use super::*;
pub(super) fn link(
    vault: Arc<Vault>,
    input: &MemoryLinkInput,
    actor: &WriteActor,
) -> MemoryResult<Value> {
    let mut source = vault.read_id(input.source)?;
    let mut target = vault
        .read_id(input.target)
        .map_err(|_| MemoryError::Invalid("link target missing or in another Vault"))?;
    let ids = match input.relation {
        MemoryRelation::Related => &mut source.note.metadata.related_ids,
        MemoryRelation::Supersedes => &mut source.note.metadata.supersedes_ids,
        MemoryRelation::Conflicts => &mut source.note.metadata.conflicts_with_ids,
    };
    if !ids.contains(&input.target) {
        ids.push(input.target);
    }
    source.note.metadata.updated_at = aura_domain_common::UtcTimestamp::now();
    origin(&mut source.note, actor);
    let mut writes = vec![MemoryWrite {
        path: source.path,
        note: source.note,
        expected_hash: Some(source.hash),
    }];
    if matches!(input.relation, MemoryRelation::Conflicts) {
        writes[0].note.metadata.status = MemoryStatus::Conflicted;
        target.note.metadata.status = MemoryStatus::Conflicted;
        if !target.note.metadata.conflicts_with_ids.contains(&input.source) {
            target.note.metadata.conflicts_with_ids.push(input.source);
        }
        target.note.metadata.updated_at = aura_domain_common::UtcTimestamp::now();
        origin(&mut target.note, actor);
        writes.push(MemoryWrite {
            path: target.path,
            note: target.note,
            expected_hash: Some(target.hash),
        });
    }
    committed(&vault, &writes, actor)
}
pub(super) fn state(
    vault: Arc<Vault>,
    input: &MemoryStateInput,
    action: &str,
    actor: &WriteActor,
) -> MemoryResult<Value> {
    let mut writes = vec![];
    for target in &input.targets {
        let mut read = read_target(&vault, target)?;
        read.note.metadata.status = if action == "memory.forget" {
            MemoryStatus::Forgotten
        } else {
            MemoryStatus::Archived
        };
        read.note.metadata.updated_at = aura_domain_common::UtcTimestamp::now();
        origin(&mut read.note, actor);
        writes.push(MemoryWrite {
            path: read.path,
            note: read.note,
            expected_hash: Some(read.hash),
        });
    }
    committed(&vault, &writes, actor)
}
pub(super) fn delete(
    vault: Arc<Vault>,
    input: &MemoryDeleteInput,
    actor: &WriteActor,
) -> MemoryResult<Value> {
    if input.items.is_empty() || input.items.len() > MAX_BATCH {
        return Err(MemoryError::Invalid("delete batch size must be 1..100"));
    }
    let mut entries = vec![];
    for item in &input.items {
        aura_domain_common::ContentHash::new(
            aura_domain_common::ContentHashAlgorithm::Blake3,
            item.expected_hash.clone(),
        )
        .map_err(|_| MemoryError::Invalid("expected_hash must be a lowercase BLAKE3 hex digest"))?;
        let read = read_target(&vault, &item.target)?;
        if read.hash != item.expected_hash {
            return Err(MemoryError::Conflict { current_hash: read.hash });
        }
        entries.push(receipt(&vault, read));
    }
    vault.delete(&entries.iter().map(|r| (r.id, r.hash.clone())).collect::<Vec<_>>(), actor)?;
    encode(&MemoryMutationResult { entries })
}
