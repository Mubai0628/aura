mod create_update;
mod mutations;
use crate::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::sync::Arc;

impl MemoryService {
    pub fn tool_scope(
        &self,
        action: &str,
        input: &Value,
        workspace: &MemoryWorkspace,
    ) -> MemoryResult<MemoryScope> {
        if action == "memory.read" {
            return self.selector_scope(&decode::<MemoryReadInput>(input)?.target, workspace);
        }
        serde_json::from_value(
            input.get("scope").cloned().ok_or(MemoryError::Invalid("memory scope"))?,
        )
        .map_err(|_| MemoryError::Invalid("memory scope"))
    }
    pub fn invoke(
        &self,
        action: &str,
        input: &Value,
        workspace: &MemoryWorkspace,
        actor: &WriteActor,
    ) -> MemoryResult<Value> {
        let scope = self.tool_scope(action, input, workspace)?;
        let vault = self.vault(scope, workspace)?;
        match action {
            "memory.search" => {
                let i: MemorySearchInput = decode(input)?;
                encode(&vault.search(&MemorySearch {
                    query: i.query,
                    category: i.category,
                    limit: i.limit,
                    include_inactive: i.include_inactive,
                })?)
            }
            "memory.read" => {
                let i: MemoryReadInput = decode(input)?;
                encode(&match i.target {
                    MemorySelector::Id { id } => vault.read_id(id)?,
                    MemorySelector::Path { path, .. } => vault.read_path(&path)?,
                })
            }
            "memory.list_recent" => {
                let i: MemoryRecentInput = decode(input)?;
                encode(&vault.recent(i.limit)?)
            }
            "memory.create" => create_update::create(vault, &decode(input)?, actor),
            "memory.update" => create_update::update(vault, &decode(input)?, actor),
            "memory.link" => mutations::link(vault, &decode(input)?, actor),
            "memory.forget" | "memory.archive" => {
                mutations::state(vault, &decode(input)?, action, actor)
            }
            "memory.delete" => mutations::delete(vault, &decode(input)?, actor),
            "memory.validate" => {
                let _: MemoryScopeInput = decode(input)?;
                encode(&vault.validation_report()?)
            }
            "memory.repair" => {
                let input = decode::<MemoryRepairInput>(input)?;
                match input.phase {
                    MemoryRepairPhase::Preview => encode(&MemoryRepairResult {
                        preview: vault.preview_repair(actor)?,
                        applied: false,
                    }),
                    MemoryRepairPhase::Apply => encode(&MemoryRepairResult {
                        preview: vault.apply_repair(
                            &input
                                .preview_token
                                .ok_or(MemoryError::Invalid("apply requires preview_token"))?,
                            actor,
                        )?,
                        applied: true,
                    }),
                }
            }
            _ => Err(MemoryError::Invalid("unknown memory tool")),
        }
    }
}

fn decode<T: DeserializeOwned>(value: &Value) -> MemoryResult<T> {
    serde_json::from_value(value.clone()).map_err(|_| MemoryError::Invalid("memory tool input"))
}
fn encode<T: Serialize>(value: &T) -> MemoryResult<Value> {
    serde_json::to_value(value).map_err(|_| MemoryError::Invalid("memory tool output"))
}
fn read_target(vault: &Vault, target: &MemoryTarget) -> MemoryResult<MemoryRead> {
    match target {
        MemoryTarget::Id { id } => vault.read_id(*id),
        MemoryTarget::Path { path } => vault.read_path(path),
    }
}
fn receipt(vault: &Vault, read: MemoryRead) -> MemoryReceipt {
    MemoryReceipt {
        id: read.note.metadata.id,
        scope: vault.scope().scope(),
        path: read.path,
        title: read.note.title(),
        hash: read.hash,
    }
}
fn origin(note: &mut MemoryDocument, actor: &WriteActor) {
    actor.stamp(note);
}
fn committed(
    vault: &Arc<Vault>,
    writes: &[MemoryWrite],
    actor: &WriteActor,
) -> MemoryResult<Value> {
    let hashes = vault.write(writes, actor)?;
    let entries = writes
        .iter()
        .zip(hashes)
        .map(|(write, hash)| MemoryReceipt {
            id: write.note.metadata.id,
            scope: vault.scope().scope(),
            path: write.path.clone(),
            title: write.note.title(),
            hash,
        })
        .collect();
    encode(&MemoryMutationResult { entries })
}
