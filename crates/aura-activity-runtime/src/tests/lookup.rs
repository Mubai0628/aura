use super::*;
use aura_domain_common::FileIdentity;

fn identity(value: u32) -> FileIdentity {
    FileIdentity {
        volume_serial_number: "18446744073709551614".into(),
        file_id: format!("{value:032x}"),
    }
}

fn object(value: u32, ancestors: &[u32], subtree: bool) -> ActivityResourceTarget {
    ActivityResourceTarget::VerifiedObject {
        identity: identity(value),
        ancestors: ancestors.iter().copied().map(identity).collect(),
        subtree,
    }
}

fn entry(parts: &[&str], folded: bool) -> ActivityResourceTarget {
    ActivityResourceTarget::VerifiedEntry {
        parent: identity(10),
        pending: parts.iter().map(|part| (*part).into()).collect(),
        case_insensitive: folded,
        comparison_keys: Some(
            parts
                .iter()
                .map(|part| if folded { part.to_ascii_uppercase() } else { (*part).to_owned() })
                .collect(),
        ),
        ancestors: vec![identity(1)],
    }
}

struct Fixture {
    index: ActivityIndex,
    runtime: RuntimeInstanceId,
    session: SessionId,
    workspace: WorkspaceId,
}

impl Fixture {
    fn new() -> Self {
        Self {
            index: ActivityIndex::new(100),
            runtime: runtime(),
            session: SessionId::new(),
            workspace: WorkspaceId::new(),
        }
    }
    fn publish(&self, target: ActivityResourceTarget) -> ActivityId {
        publish_agent_tool(
            &self.index,
            self.runtime,
            self.session,
            self.workspace,
            ActivityToolGroup::FileRead,
            1,
            vec![resource(target, ActivityResourceRelation::Read, "same.txt")],
        )
        .0
    }
    fn query(&self, target: ActivityResourceTarget) -> ActivitySnapshot {
        self.index.snapshot_query(
            &view(self.runtime, self.session, self.workspace),
            &ActivityQuery::Resource { target },
        )
    }
}

#[test]
fn actual_snapshot_hash_lookup_does_not_inspect_unrelated_records() {
    let fixture = Fixture::new();
    let first = fixture.publish(object(20, &[10, 1], false));
    let second = fixture.publish(object(20, &[10, 1], false));
    for value in 100..164 {
        fixture.publish(object(value, &[99, 1], false));
    }
    resource_index::INSPECTED.with(|count| count.set(0));
    let snapshot = fixture.query(object(20, &[10, 1], false));
    assert_eq!(snapshot.header.coverage.state, ActivityCoverageState::Complete);
    assert_eq!(
        snapshot.rows.iter().map(|row| row.id).collect::<BTreeSet<_>>(),
        BTreeSet::from([first, second])
    );
    resource_index::INSPECTED.with(|count| {
        assert_eq!(count.get(), 2, "actual consumer inspects matching IDs only, not 66 records")
    });
}

#[test]
fn full_ancestry_lookup_finds_covering_directories_and_descendants_both_ways() {
    let fixture = Fixture::new();
    let parent = fixture.publish(object(10, &[1], true));
    let child = fixture.publish(object(20, &[10, 1], false));
    let prospective = fixture.publish(entry(&["new", "a.txt"], false));
    fixture.publish(object(21, &[11, 1], false));
    assert_eq!(
        fixture
            .query(object(20, &[10, 1], false))
            .rows
            .iter()
            .map(|row| row.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([parent, child])
    );
    assert_eq!(
        fixture
            .query(object(10, &[1], true))
            .rows
            .iter()
            .map(|row| row.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([parent, child, prospective])
    );
    assert_eq!(
        fixture
            .query(entry(&["new", "a.txt"], false))
            .rows
            .iter()
            .map(|row| row.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([parent, prospective])
    );
}

#[test]
fn prospective_trie_uses_whole_segments_and_the_verified_case_rule() {
    let fixture = Fixture::new();
    let parent = fixture.publish(entry(&["folder"], true));
    let child = fixture.publish(entry(&["FOLDER", "a.txt"], true));
    fixture.publish(entry(&["Folder2", "a.txt"], true));
    fixture.publish(entry(&["FOLDER", "a.txt"], false));
    assert_eq!(
        fixture
            .query(entry(&["Folder", "A.TXT"], true))
            .rows
            .iter()
            .map(|row| row.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([parent, child])
    );
    assert_eq!(
        fixture
            .query(entry(&["folder"], true))
            .rows
            .iter()
            .map(|row| row.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([parent, child])
    );
    assert!(fixture.query(entry(&["Folder", "another.txt"], false)).rows.is_empty());
}

#[test]
fn ending_all_owners_reclaims_full_identity_and_prefix_associations() {
    let fixture = Fixture::new();
    let first = fixture.publish(entry(&["new", "a.txt"], true));
    let second = fixture.publish(entry(&["new", "b.txt"], true));
    let directory = fixture.publish(object(10, &[1], true));
    assert_eq!(fixture.index.end(first, 1), ActivityEndOutcome::Ended);
    assert_eq!(fixture.query(entry(&["new"], true)).rows.len(), 2);
    assert_eq!(fixture.index.end(second, 1), ActivityEndOutcome::Ended);
    assert_eq!(fixture.index.end(directory, 1), ActivityEndOutcome::Ended);
    assert!(fixture.query(object(10, &[1], true)).rows.is_empty());
    assert!(fixture.index.0.lock().expect("index free").resources.is_empty());
    assert_eq!(fixture.index.index_entries(), (0, 0));
}

#[test]
fn an_unproven_resource_query_is_unavailable_instead_of_complete_empty() {
    let fixture = Fixture::new();
    fixture.publish(object(20, &[], false));
    let query = ActivityQuery::Resource {
        target: ActivityResourceTarget::Unproven { hint: "a.txt".into() },
    };
    let snapshot = fixture
        .index
        .snapshot_query(&view(fixture.runtime, fixture.session, fixture.workspace), &query);
    assert!(snapshot.rows.is_empty());
    assert_eq!(snapshot.header.coverage.state, ActivityCoverageState::Unavailable);
    assert!(
        snapshot
            .header
            .coverage
            .omission_reasons
            .contains(&ActivityOmissionReason::IndexUnavailable)
    );
    let fixed = fixture.index.snapshot(&view(fixture.runtime, fixture.session, fixture.workspace));
    assert_eq!(fixed.query(query, 0, 20).header.coverage.state, ActivityCoverageState::Unavailable);
}

#[test]
fn lost_end_reset_reclaims_every_new_association_without_claiming_complete() {
    let fixture = Fixture::new();
    fixture.publish(entry(&["new", "a.txt"], true));
    fixture.index.1.reset.store(true, std::sync::atomic::Ordering::Release);
    fixture.index.count_unavailable();
    let snapshot = fixture.query(entry(&["new"], true));
    assert!(snapshot.rows.is_empty());
    assert_eq!(snapshot.header.coverage.state, ActivityCoverageState::Partial);
    assert!(fixture.index.0.lock().expect("index free").resources.is_empty());
}
