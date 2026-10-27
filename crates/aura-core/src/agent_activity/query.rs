//! One read-only projection policy shared by Main IPC, actual Agent tools and the P07 adapter.
use aura_activity_domain::{
    ActivityCoverageState, ActivityOmissionReason, ActivityQuery, ActivityQueryResult,
    ActivityResourceTarget,
};
use aura_activity_runtime::{ActivityIndex, ActivityResourceKey, ActivityVisibility};
use aura_domain_common::Revision;

pub(crate) fn page(
    index: &ActivityIndex,
    mut view: ActivityVisibility,
    query: ActivityQuery,
    expected: Option<Revision>,
    offset: u32,
    limit: u32,
    own_only: bool,
) -> anyhow::Result<ActivityQueryResult> {
    if let ActivityQuery::Resource { target } = &query {
        anyhow::ensure!(target.is_within_request_bounds(), "activity.query_target_over_limit");
        // A request hint selects minimal metadata. It never grants another session's path/parameters.
        view.relevant_targets.insert(ActivityResourceKey::of(target));
        view.relevant_resources.push(target.clone());
    }
    let mut snapshot = index.snapshot_query(&view, &query);
    if own_only {
        // The model reads its own session's activities here. Another session's task reaches it
        // through the step's task state, named by the files it may write, and is counted here
        // as a row this reader does not see.
        let before = snapshot.rows.len();
        snapshot.rows.retain(|row| row.session_id == view.session_id);
        let hidden = u64::try_from(before - snapshot.rows.len()).unwrap_or(u64::MAX);
        if hidden > 0 {
            let coverage = &mut snapshot.header.coverage;
            coverage.visibility_filtered = coverage.visibility_filtered.saturating_add(hidden);
            if coverage.state == ActivityCoverageState::Complete {
                coverage.state = ActivityCoverageState::Partial;
            }
            if !coverage.omission_reasons.contains(&ActivityOmissionReason::VisibilityFiltered) {
                coverage.omission_reasons.push(ActivityOmissionReason::VisibilityFiltered);
            }
        }
    }
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
        let initial = page(&index, view.clone(), ActivityQuery::Session, None, 0, 1, true)
            .expect("valid activity query fixture");
        assert_eq!(initial.rows[0].id, first_id);
        let mut second = crate::agent_activity::tests_support::synthetic_record();
        second.owner = first.owner;
        second.runtime_instance_id = first.runtime_instance_id;
        assert!(index.publish(second).was_recorded());
        let stale = page(
            &index,
            view.clone(),
            ActivityQuery::Session,
            Some(initial.header.revision),
            1,
            1,
            true,
        )
        .expect("valid activity query fixture");
        assert!(stale.header.stale && stale.rows.is_empty() && !stale.has_more);
        let refreshed = page(&index, view, ActivityQuery::Session, None, 0, 100, true)
            .expect("valid activity query fixture");
        assert!(
            !refreshed.header.stale && refreshed.rows.len() == 2,
            "starting at offset zero obtains the new complete view"
        );
    }
}
