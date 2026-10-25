use super::*;

pub(super) fn create(
    vault: Arc<Vault>,
    input: &MemoryCreateInput,
    actor: &WriteActor,
) -> MemoryResult<Value> {
    let writes = input
        .items
        .iter()
        .map(|item| {
            let mut metadata = MemoryMetadata::new(input.scope, item.category.clone());
            metadata.tags = item.tags.clone();
            if let Some(value) = item.confidence {
                metadata.confidence = value;
            }
            if let Some(value) = item.importance {
                metadata.importance = value;
            }
            if let Some(value) = item.recall_weight {
                metadata.recall_weight = value;
            }
            metadata.related_ids = item.related_ids.clone();
            metadata.supersedes_ids = item.supersedes_ids.clone();
            metadata.conflicts_with_ids = item.conflicts_with_ids.clone();
            if !metadata.conflicts_with_ids.is_empty() {
                metadata.status = MemoryStatus::Conflicted;
            }
            let mut note = MemoryDocument { metadata, body: item.body.clone() };
            origin(&mut note, actor);
            MemoryWrite { path: item.path.clone(), note, expected_hash: None }
        })
        .collect::<Vec<_>>();
    committed(&vault, &writes, actor)
}

pub(super) fn update(
    vault: Arc<Vault>,
    input: &MemoryUpdateInput,
    actor: &WriteActor,
) -> MemoryResult<Value> {
    let mut writes = vec![];
    for item in &input.items {
        aura_domain_common::ContentHash::new(
            aura_domain_common::ContentHashAlgorithm::Blake3,
            item.expected_hash.clone(),
        )
        .map_err(|_| MemoryError::Invalid("expected_hash must be a lowercase BLAKE3 hex digest"))?;
        let mut read = read_target(&vault, &item.target)?;
        if read.hash != item.expected_hash {
            return Err(MemoryError::Conflict { current_hash: read.hash });
        }
        if let Some(body) = &item.body {
            read.note.body = body.clone();
        }
        let m = &mut read.note.metadata;
        if let Some(value) = &item.category {
            m.category = value.clone();
        }
        if let Some(value) = item.confidence {
            m.confidence = value;
        }
        if let Some(value) = item.importance {
            m.importance = value;
        }
        if let Some(value) = item.recall_weight {
            m.recall_weight = value;
        }
        if let Some(value) = &item.tags {
            m.tags = value.clone();
        }
        if let Some(value) = &item.related_ids {
            m.related_ids = value.clone();
        }
        if let Some(value) = &item.supersedes_ids {
            m.supersedes_ids = value.clone();
        }
        if let Some(value) = &item.conflicts_with_ids {
            m.conflicts_with_ids = value.clone();
        }
        if let Some(value) = item.status {
            m.status = value;
        }
        m.updated_at = aura_domain_common::UtcTimestamp::now();
        origin(&mut read.note, actor);
        writes.push(MemoryWrite {
            path: read.path,
            note: read.note,
            expected_hash: Some(item.expected_hash.clone()),
        });
    }
    committed(&vault, &writes, actor)
}
