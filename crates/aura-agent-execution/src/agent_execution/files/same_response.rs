//! Calls written after other changes of their file in the same response.
//!
//! The batch starts a call on a file only after the calls written before it on that file
//! finished. Their operations were approved together, before any of them ran, so a later call's
//! approved target names the file as it was then. An earlier call that replaced the file left
//! another object at the same entry: when the file now is the very object those calls published,
//! the target is unchanged in what it means. A model also writes every patch of one response
//! against the hash it last read, so a later patch applies to the file as the earlier changes left
//! it: the hash it gives counts when one of those changes started from it or passed through it,
//! and the file layer is given the hash the last of them left, which it checks as it checks any
//! hash. Anything else that changed the file in between still stops the call.
use super::*;
use aura_domain_common::ContentHash;
use aura_execution_contract::OperationContent;
use aura_security_contract::file_object_key;
use std::collections::BTreeMap;
use std::sync::Mutex;

/// What calls of each running batch published, in the order published. Kept in memory: after a
/// restart a later call of a batch is checked as if no earlier call had replaced its file.
static REPLACED: Mutex<BTreeMap<ToolBatchId, Vec<Replacement>>> = Mutex::new(BTreeMap::new());

/// How many batches' replacements are kept; the oldest batch's go first.
const BATCHES_KEPT: usize = 64;

/// One entry's object before and after a publication.
#[derive(Clone, Debug)]
struct Replacement {
    entry_key: String,
    before: String,
    after: String,
}

/// Records what a call of `batch` published.
pub(super) fn record(batch: ToolBatchId, receipts: &[aura_fs_tools::FilePublicationReceipt]) {
    let Ok(mut replaced) = REPLACED.lock() else {
        return;
    };
    let list = replaced.entry(batch).or_default();
    for receipt in receipts {
        list.push(Replacement {
            entry_key: receipt.entry_key.clone(),
            before: receipt
                .previous_identity
                .as_ref()
                .map(file_object_key)
                .unwrap_or_else(|| receipt.entry_key.clone()),
            after: file_object_key(&receipt.published_identity),
        });
    }
    while replaced.len() > BATCHES_KEPT {
        replaced.pop_first();
    }
}

/// Whether `fresh` differs from the approved `frozen` only by files that earlier calls of the
/// batch replaced: the same entry, holding the object those calls published.
pub(in crate::agent_execution) fn replaced_since(
    batch: ToolBatchId,
    frozen: &OperationContent,
    fresh: &OperationContent,
) -> bool {
    if frozen.resources.len() != fresh.resources.len() {
        return false;
    }
    let Ok(replaced) = REPLACED.lock() else {
        return false;
    };
    let Some(list) = replaced.get(&batch) else {
        return false;
    };
    let mut adopted = fresh.clone();
    let mut any = false;
    for (was, now) in frozen.resources.iter().zip(adopted.resources.iter_mut()) {
        if was == now {
            continue;
        }
        let (
            ResourceDescriptor::WorkspacePath {
                workspace_id,
                canonical_relative_path,
                file_target: Some(before),
            },
            ResourceDescriptor::WorkspacePath {
                workspace_id: now_workspace,
                canonical_relative_path: now_path,
                file_target: Some(after),
            },
        ) = (was, &*now)
        else {
            return false;
        };
        let mut same = before.clone();
        same.identity_key.clone_from(&after.identity_key);
        if workspace_id != now_workspace
            || canonical_relative_path != now_path
            || same != *after
            || !reached(list, &before.entry_key, &before.identity_key, &after.identity_key)
        {
            return false;
        }
        *now = was.clone();
        any = true;
    }
    any && adopted == *frozen
}

/// Whether the entry went from the object `from` to the object `to` through the replacements.
fn reached(list: &[Replacement], entry: &str, from: &str, to: &str) -> bool {
    let mut current = from;
    for replacement in list.iter().filter(|replacement| replacement.entry_key == entry) {
        if replacement.before == current {
            current = &replacement.after;
        }
    }
    current == to
}

/// The hash a patch applies on: the one it gives, or the one the earlier changes of its file in
/// this response left.
pub(super) fn base_hash(
    service: &AgentExecution,
    call: &CallContext,
    given: ContentHash,
) -> Result<ContentHash> {
    let Some(file) = file_of(&call.operation) else {
        return Ok(given);
    };
    let changes = {
        let store = service.journal.lock()?;
        aura_persistence_v2::tool::same_response::earlier_changes(
            store.database.connection(),
            call.batch.tool_batch_id,
            call.validated.call.ordinal,
        )?
    };
    let mut passed = Vec::new();
    let mut left = None;
    for change in changes {
        if file_of(&change.operation).as_ref() != Some(&file) {
            continue;
        }
        passed.extend(started_from(&change.operation));
        if let Some(previous) = left.replace(change.left) {
            passed.push(previous);
        }
    }
    Ok(match left {
        Some(left) if left != given && passed.contains(&given) => left,
        _ => given,
    })
}

/// The file a change names: its workspace path, its granted file or its outside path.
fn file_of(operation: &SecuredOperation) -> Option<Vec<Value>> {
    let files = operation
        .content
        .resources
        .iter()
        .filter_map(|resource| match resource {
            ResourceDescriptor::WorkspacePath { workspace_id, canonical_relative_path, .. } => {
                Some(json!({"workspace": workspace_id, "path": canonical_relative_path}))
            }
            ResourceDescriptor::ExternalGrant { grant_id } => {
                Some(json!({"grant": grant_id, "path": operation.content.action["path"]}))
            }
            ResourceDescriptor::ExternalPath { canonical_path, .. } => {
                Some(json!({"outside": canonical_path}))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    (!files.is_empty()).then_some(files)
}

/// The hash a change was written against, when it gave one.
fn started_from(operation: &SecuredOperation) -> Option<ContentHash> {
    let action = operation.content.action.clone();
    match operation.content.action_kind.as_str() {
        "fs.patch" => serde_json::from_value::<AgentFilePatchInput>(action)
            .ok()
            .map(|input| input.expected_hash),
        "fs.write" => match serde_json::from_value::<AgentFileWriteInput>(action).ok()?.expected {
            FsExpectedState::FileHash { hash } => Some(hash),
            _ => None,
        },
        _ => None,
    }
}
