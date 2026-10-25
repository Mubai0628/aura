//! Associations for verified physical objects and prospective entries. Queries inspect only
//! matching IDs; the resource selector never walks the complete record table. Every association
//! is bounded by admitted records/resources/ancestry, and is reclaimed with its owning record.
use super::{ActivityResourceKey, IndexState};
use aura_activity_domain::{ActivityId, ActivityQuery, ActivityRecord, ActivityResourceTarget};
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

type Associations = HashMap<ActivityResourceKey, BTreeSet<ActivityId>>;

#[derive(Default)]
pub(super) struct ResourceIndex {
    descendants: Associations,
    subtree_owners: Associations,
    entries: HashMap<EntryRoot, EntryNode>,
}

#[derive(Eq, Hash, PartialEq)]
struct EntryRoot {
    parent: ActivityResourceKey,
    folded: bool,
}

#[derive(Default)]
struct EntryNode {
    direct: BTreeSet<ActivityId>,
    descendants: BTreeSet<ActivityId>,
    children: HashMap<String, EntryNode>,
}

impl ResourceIndex {
    pub(super) fn insert(&mut self, id: ActivityId, target: &ActivityResourceTarget) {
        match target {
            ActivityResourceTarget::VerifiedObject { identity, ancestors, subtree } => {
                for identity in std::iter::once(identity).chain(ancestors) {
                    add(&mut self.descendants, ActivityResourceKey::verified(identity), id);
                }
                if *subtree {
                    add(&mut self.subtree_owners, ActivityResourceKey::verified(identity), id);
                }
            }
            ActivityResourceTarget::VerifiedEntry { parent, ancestors, .. } => {
                for identity in std::iter::once(parent).chain(ancestors) {
                    add(&mut self.descendants, ActivityResourceKey::verified(identity), id);
                }
                if let Some((root, pending)) = entry_parts(target) {
                    let mut node = self.entries.entry(root).or_default();
                    for part in pending {
                        node = node.children.entry(part).or_default();
                        node.descendants.insert(id);
                    }
                    node.direct.insert(id);
                }
            }
            _ => {}
        }
    }

    pub(super) fn remove(&mut self, id: ActivityId, target: &ActivityResourceTarget) {
        match target {
            ActivityResourceTarget::VerifiedObject { identity, ancestors, subtree } => {
                for identity in std::iter::once(identity).chain(ancestors) {
                    remove(&mut self.descendants, &ActivityResourceKey::verified(identity), id);
                }
                if *subtree {
                    remove(&mut self.subtree_owners, &ActivityResourceKey::verified(identity), id);
                }
            }
            ActivityResourceTarget::VerifiedEntry { parent, ancestors, .. } => {
                for identity in std::iter::once(parent).chain(ancestors) {
                    remove(&mut self.descendants, &ActivityResourceKey::verified(identity), id);
                }
                if let Some((root, pending)) = entry_parts(target)
                    && let Some(node) = self.entries.get_mut(&root)
                {
                    remove_entry(node, &pending, id);
                    if node.children.is_empty() {
                        self.entries.remove(&root);
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn candidates(
        &self,
        target: &ActivityResourceTarget,
        exact: &Associations,
    ) -> Option<BTreeSet<ActivityId>> {
        if matches!(ActivityResourceKey::of(target), ActivityResourceKey::Unproven { .. })
            || !target.is_within_request_bounds()
        {
            return None;
        }
        let mut ids = exact.get(&ActivityResourceKey::of(target)).cloned().unwrap_or_default();
        match target {
            ActivityResourceTarget::VerifiedObject { identity, ancestors, subtree } => {
                if *subtree {
                    extend(
                        &mut ids,
                        self.descendants.get(&ActivityResourceKey::verified(identity)),
                    );
                }
                for identity in std::iter::once(identity).chain(ancestors) {
                    extend(
                        &mut ids,
                        self.subtree_owners.get(&ActivityResourceKey::verified(identity)),
                    );
                }
            }
            ActivityResourceTarget::VerifiedEntry { parent, ancestors, .. } => {
                for identity in std::iter::once(parent).chain(ancestors) {
                    extend(
                        &mut ids,
                        self.subtree_owners.get(&ActivityResourceKey::verified(identity)),
                    );
                }
                if let Some((root, pending)) = entry_parts(target)
                    && let Some(mut node) = self.entries.get(&root)
                {
                    for (position, part) in pending.iter().enumerate() {
                        let Some(child) = node.children.get(part) else { break };
                        node = child;
                        extend(&mut ids, Some(&node.direct));
                        if position + 1 == pending.len() {
                            extend(&mut ids, Some(&node.descendants));
                        }
                    }
                }
            }
            _ => {}
        }
        Some(ids)
    }

    #[cfg(test)]
    pub(super) fn is_empty(&self) -> bool {
        self.descendants.is_empty() && self.subtree_owners.is_empty() && self.entries.is_empty()
    }
}

fn entry_parts(target: &ActivityResourceTarget) -> Option<(EntryRoot, Vec<String>)> {
    let ActivityResourceKey::VerifiedEntry { volume, file_id, pending, folded } =
        ActivityResourceKey::of(target)
    else {
        return None;
    };
    Some((
        EntryRoot { parent: ActivityResourceKey::VerifiedObject { volume, file_id }, folded },
        pending,
    ))
}

fn add(map: &mut Associations, key: ActivityResourceKey, id: ActivityId) {
    map.entry(key).or_default().insert(id);
}

fn remove(map: &mut Associations, key: &ActivityResourceKey, id: ActivityId) {
    if let Some(ids) = map.get_mut(key) {
        ids.remove(&id);
        if ids.is_empty() {
            map.remove(key);
        }
    }
}

fn extend(ids: &mut BTreeSet<ActivityId>, candidates: Option<&BTreeSet<ActivityId>>) {
    if let Some(candidates) = candidates {
        ids.extend(candidates.iter().copied());
    }
}

fn remove_entry(node: &mut EntryNode, pending: &[String], id: ActivityId) {
    let Some((part, rest)) = pending.split_first() else {
        node.direct.remove(&id);
        return;
    };
    if let Some(child) = node.children.get_mut(part) {
        remove_entry(child, rest, id);
        child.descendants.remove(&id);
        if child.children.is_empty() && child.direct.is_empty() && child.descendants.is_empty() {
            node.children.remove(part);
        }
    }
}

// This is the actual snapshot selector, called inside the single publication-state sampling
// block. Its test witness counts every inspected record, rather than relying on elapsed time.
pub(super) fn select_records(
    state: &IndexState,
    query: Option<&ActivityQuery>,
) -> (Vec<Arc<ActivityRecord>>, bool) {
    match query {
        Some(ActivityQuery::Exact { id }) => {
            (state.records.get(id).cloned().into_iter().collect(), false)
        }
        Some(ActivityQuery::Resource { target }) => {
            let Some(ids) = state.resources.candidates(target, &state.by_resource) else {
                return (Vec::new(), true);
            };
            let records = ids
                .into_iter()
                .filter_map(|id| state.records.get(&id))
                .filter(|record| {
                    #[cfg(test)]
                    INSPECTED.with(|count| count.set(count.get() + 1));
                    record
                        .resources
                        .iter()
                        .any(|resource| super::targets_related(&resource.target, target))
                })
                .cloned()
                .collect();
            (records, false)
        }
        _ => (state.records.values().cloned().collect(), false),
    }
}

#[cfg(test)]
std::thread_local! {
    pub(super) static INSPECTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
