//! One read-only projection policy shared by Main IPC, actual Agent tools and the P07 adapter.
use aura_activity_domain::{ActivityQuery, ActivityQueryResult, ActivityResourceTarget};
use aura_activity_runtime::{ActivityIndex, ActivityResourceKey, ActivityVisibility};
use aura_domain_common::Revision;

pub(crate) fn page(
    index: &ActivityIndex,
    mut view: ActivityVisibility,
    query: ActivityQuery,
    expected: Option<Revision>,
    offset: u32,
    limit: u32,
) -> anyhow::Result<ActivityQueryResult> {
    if let ActivityQuery::Resource { target } = &query {
        anyhow::ensure!(target.is_within_request_bounds(), "activity.query_target_over_limit");
        // A request hint selects minimal metadata. It never grants another session's path/parameters.
        view.relevant_targets.insert(ActivityResourceKey::of(target));
        view.relevant_resources.push(target.clone());
    }
    let snapshot = index.snapshot_query(&view, &query);
    let mut page = snapshot.query(ActivityQuery::Session, offset, limit);
    page.query = query;
    page.header.stale = expected.is_some_and(|revision| revision != snapshot.header.revision);
    if page.header.stale {
        // Do not combine offsets from different snapshots. The caller restarts from offset zero.
        page.rows.clear();
        page.has_more = false;
    }
    Ok(page)
}

pub(crate) fn keys<'a>(
    targets: impl IntoIterator<Item = &'a ActivityResourceTarget>,
) -> std::collections::BTreeSet<ActivityResourceKey> {
    targets.into_iter().map(ActivityResourceKey::of).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activity_paging_rejects_offsets_from_a_changed_snapshot() {
        let index = ActivityIndex::default();
        let first = crate::agent_activity::tests_support::synthetic_record();
        let view = ActivityVisibility::for_session(
            first.runtime_instance_id,
            first.owner.session_id,
            first.owner.workspace_id,
        );
        let first_id = first.id;
        assert!(index.publish(first.clone()).was_recorded());
        let initial = page(&index, view.clone(), ActivityQuery::Session, None, 0, 1)
            .expect("valid activity query fixture");
        assert_eq!(initial.rows[0].id, first_id);
        let mut second = crate::agent_activity::tests_support::synthetic_record();
        second.owner = first.owner;
        second.runtime_instance_id = first.runtime_instance_id;
        assert!(index.publish(second).was_recorded());
        let stale =
            page(&index, view.clone(), ActivityQuery::Session, Some(initial.header.revision), 1, 1)
                .expect("valid activity query fixture");
        assert!(stale.header.stale && stale.rows.is_empty() && !stale.has_more);
        let refreshed = page(&index, view, ActivityQuery::Session, None, 0, 100)
            .expect("valid activity query fixture");
        assert!(
            !refreshed.header.stale && refreshed.rows.len() == 2,
            "starting at offset zero obtains the new complete view"
        );
    }
}
