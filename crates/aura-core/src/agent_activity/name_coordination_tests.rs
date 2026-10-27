//! Real files + the shared admission owner + the production Buffer activity publisher.
//! These are file-boundary tests, not a claim about a desktop window or authenticated IPC.
use aura_activity_domain::{ActivityQuery, ActivityResourceTarget};
use aura_activity_runtime::{ActivityIndex, ActivityObserverHandle, ActivityVisibility};
use aura_domain_common::{Revision, RuntimeInstanceId, SessionId, WorkspaceId, WorkspacePath};
use aura_workspace::{BufferStore, ResourceFacts, SecurePathResolver};
use aura_workspace_domain::{
    BufferEditCommand, CatalogFreshness, CommitRequest, EligibilityOwner, ExecutionId,
    ResourceAccess, WorkspaceState, WorkspaceSummary,
};
use std::{
    path::Path,
    sync::{Arc, Condvar, Mutex, mpsc},
    time::Duration,
};

fn resolver(root: &Path, workspace_id: WorkspaceId) -> SecurePathResolver {
    let info = SecurePathResolver::inspect_selected_root(root).expect("actual selected root");
    SecurePathResolver::from_workspace(&WorkspaceSummary {
        workspace_id,
        display_name: "name coordination".into(),
        selected_root: root.display().to_string(),
        canonical_root: info.final_path.display().to_string(),
        root_identity: Some(aura_workspace::domain_identity(&info.identity)),
        state: WorkspaceState::Online,
        revision: Revision::INITIAL,
        catalog_freshness: CatalogFreshness::Unavailable,
        authorization_epoch: Revision::INITIAL,
    })
    .expect("verified resolver")
}

#[derive(Default)]
struct Hold {
    released: Mutex<bool>,
    changed: Condvar,
}
impl Hold {
    fn wait(&self) {
        let released = self.released.lock().expect("hold");
        let _ = self
            .changed
            .wait_timeout_while(released, Duration::from_secs(5), |done| !*done)
            .expect("bounded hold");
    }
    fn release(&self) {
        *self.released.lock().expect("release") = true;
        self.changed.notify_all();
    }
}
struct ReleaseOnDrop(Arc<Hold>);
impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        self.0.release();
    }
}

fn entry(facts: &ResourceFacts, path: &WorkspacePath) -> ActivityResourceTarget {
    let resource = facts.slot(path).expect("actual parent facts");
    super::resources::project(&[aura_workspace_domain::ResourceRequest::new(
        resource,
        ResourceAccess::Write,
    )])
    .remove(0)
    .target
}

#[test]
fn distinct_unicode_names_really_overlap_and_the_live_index_keeps_them_separate() {
    for (first, second) in
        [("ß.txt", "ss.txt"), ("ﬃ.txt", "ffi.txt"), ("\u{10428}.txt", "\u{10400}.txt")]
    {
        let root = tempfile::tempdir().expect("new synthetic root");
        std::fs::write(root.path().join(first), b"old first").expect("first seed");
        std::fs::write(root.path().join(second), b"old second").expect("second seed");
        let native_first =
            aura_windows_platform::inspect_existing_path(&root.path().join(first), false)
                .expect("first ID");
        let native_second =
            aura_windows_platform::inspect_existing_path(&root.path().join(second), false)
                .expect("second ID");
        assert_ne!(
            native_first.identity, native_second.identity,
            "the filesystem distinguishes these names"
        );
        let index = Arc::new(ActivityIndex::default());
        let observer = ActivityObserverHandle::default();
        let generation = RuntimeInstanceId::new();
        let owner = Arc::new(EligibilityOwner::new());
        let first_session = SessionId::new();
        let second_session = SessionId::new();
        let first_workspace = WorkspaceId::new();
        let second_workspace = WorkspaceId::new();
        let first_resolver = resolver(root.path(), first_workspace);
        let second_resolver = resolver(root.path(), second_workspace);
        let first_facts = ResourceFacts::root(Arc::new(first_resolver.clone()), first_session);
        let second_facts = ResourceFacts::root(Arc::new(second_resolver.clone()), second_session);
        let first_path: WorkspacePath = first.parse().expect("first path");
        let second_path: WorkspacePath = second.parse().expect("second path");
        let first_target = entry(&first_facts, &first_path);
        // The reader knows its own spelling, rather than the other session's original alias.
        let known_second_path: WorkspacePath =
            format!("{}.TXT", second.trim_end_matches(".txt")).parse().expect("known alias");
        let second_target = entry(&second_facts, &known_second_path);
        assert!(!aura_activity_runtime::targets_related(&first_target, &second_target));
        let copy: ActivityResourceTarget =
            serde_json::from_value(serde_json::to_value(&first_target).expect("wire"))
                .expect("query target round trip");
        assert_eq!(
            aura_activity_runtime::ActivityResourceKey::of(&copy),
            aura_activity_runtime::ActivityResourceKey::of(&first_target)
        );
        let publisher_index = Arc::clone(&index);
        let store = Arc::new(
            BufferStore::with_eligibility_owner(Arc::clone(&owner)).with_activity_publisher(
                Arc::new(move |fact| {
                    super::publish_buffer_commit_activity(
                        &fact,
                        generation,
                        &publisher_index,
                        &observer,
                    )
                }),
            ),
        );
        let mut prepared = Vec::new();
        for (path, session, workspace, resolver, text) in [
            (first_path.clone(), first_session, first_workspace, first_resolver, "new first"),
            (second_path.clone(), second_session, second_workspace, second_resolver, "new second"),
        ] {
            let opened = store
                .open(&resolver, session, path.clone(), true)
                .expect("independent writable open");
            let edited = store
                .apply_edit(BufferEditCommand {
                    workspace_id: workspace,
                    session_id: session,
                    path: path.clone(),
                    reservation_id: opened.reservation_id.expect("reservation"),
                    reservation_epoch: opened.reservation_epoch,
                    buffer_revision: Revision::new(opened.buffer_revision.value() + 1),
                    base_core_revision: opened.buffer_revision,
                    content: text.into(),
                    active_editing: false,
                    ime_composition_active: false,
                })
                .expect("edit");
            prepared.push((path, session, resolver, edited));
        }
        assert_eq!(
            owner.snapshot().claims.len(),
            2,
            "two workspaces hold two different real targets"
        );
        let hold = Arc::new(Hold::default());
        let release = ReleaseOnDrop(Arc::clone(&hold));
        let (arrived_tx, arrived_rx) = mpsc::channel();
        let mut hooks = Vec::new();
        for name in [first, second] {
            let hold = Arc::clone(&hold);
            let arrived = arrived_tx.clone();
            let label = name.to_owned();
            hooks.push(aura_windows_platform::publication_fault::observe_commit_window(
                &root.path().join(name),
                Box::new(move |_| {
                    if arrived.send(label.clone()).is_ok() {
                        hold.wait();
                    }
                }),
                Box::new(|_| {}),
            ));
        }
        let mut threads = Vec::new();
        let mut arrivals = Vec::new();
        // Publish A before starting B. The index may legitimately drop a simultaneous
        // publication instead of blocking either worker; this proof needs complete records.
        // A remains inside its actual disk commit while B enters its own commit.
        for (path, session, resolver, edited) in prepared {
            let store = Arc::clone(&store);
            threads.push(std::thread::spawn(move || {
                store.save(
                    &resolver,
                    session,
                    &path,
                    edited.reservation_id.expect("reservation"),
                    edited.reservation_epoch,
                    edited.buffer_revision,
                )
            }));
            arrivals.push(
                arrived_rx
                    .recv_timeout(Duration::from_secs(3))
                    .expect("this real commit must enter while all earlier commits remain held"),
            );
        }
        assert_ne!(arrivals[0], arrivals[1]);
        assert!(!*hold.released.lock().expect("hold"));
        assert_eq!(owner.snapshot().commits, 2, "two real disk commit windows overlap");
        assert_eq!(index.len(), 2, "both real publications were independently witnessed");
        for (target, expected_owner) in
            [(first_target.clone(), first_session), (second_target, second_session)]
        {
            let result = super::query::page(
                &index,
                ActivityVisibility::for_session(generation, first_session, first_workspace),
                ActivityQuery::Resource { target },
                None,
                0,
                10,
                false,
            )
            .expect("real query consumer");
            assert_eq!(
                result.header.coverage.state,
                aura_activity_domain::ActivityCoverageState::Complete,
                "an unavailable observation must never stand in for this positive proof"
            );
            assert_eq!(
                result.rows.len(),
                1,
                "an unrelated name must not leak into the exact-key query"
            );
            assert_eq!(result.rows[0].session_id, expected_owner);
            if expected_owner == second_session {
                assert!(
                    !serde_json::to_string(&result.rows).expect("wire").contains(second),
                    "the cross-session query must not return the producer's original spelling"
                );
            }
        }
        hold.release();
        drop(release);
        for thread in threads {
            thread.join().expect("save worker").expect("real save");
        }
        drop(hooks);
        assert_eq!(std::fs::read(root.path().join(first)).expect("first result"), b"new first");
        assert_eq!(std::fs::read(root.path().join(second)).expect("second result"), b"new second");
        assert_eq!(owner.snapshot().commits, 0);
        assert_eq!(index.len(), 0, "both real settlements ended their own records");
        eprintln!(
            "different-name proof {first:?}/{second:?}: physical_objects=2 overlapping_commits=2 exact_query_rows=1/1"
        );
    }
}

#[test]
fn aliases_of_one_actual_target_coordinate_even_before_the_file_exists() {
    for (first, alias) in [("note.txt", "NOTE.txt"), ("\u{1f80}.txt", "\u{1f88}.txt")] {
        let root = tempfile::tempdir().expect("new root");
        let a = SessionId::new();
        let b = SessionId::new();
        let facts_a = ResourceFacts::root(Arc::new(resolver(root.path(), WorkspaceId::new())), a);
        let facts_b = ResourceFacts::root(Arc::new(resolver(root.path(), WorkspaceId::new())), b);
        let first_path: WorkspacePath = first.parse().expect("path");
        let alias_path: WorkspacePath = alias.parse().expect("alias");
        let first_entry = facts_a.slot(&first_path).expect("absent entry");
        let alias_entry = facts_b.slot(&alias_path).expect("absent alias");
        let owner = EligibilityOwner::new();
        let held = owner
            .commit(
                &CommitRequest::one(first_entry, ResourceAccess::Write),
                a,
                ExecutionId::next(),
                None,
            )
            .expect("first name hold");
        let refused = owner
            .commit(
                &CommitRequest::one(alias_entry.clone(), ResourceAccess::Write),
                b,
                ExecutionId::next(),
                None,
            )
            .expect_err("one absent destination cannot acquire two writable qualifications");
        assert!(refused.retryable);
        assert_eq!(refused.held_by, Some(a));
        assert!(aura_activity_runtime::targets_related(
            &entry(&facts_a, &first_path),
            &entry(&facts_b, &alias_path)
        ));
        std::fs::write(root.path().join(first), b"first")
            .expect("effect under the held destination");
        let first_id =
            aura_windows_platform::inspect_existing_path(&root.path().join(first), false)
                .expect("first ID")
                .identity;
        let alias_id =
            aura_windows_platform::inspect_existing_path(&root.path().join(alias), false)
                .expect("alias ID")
                .identity;
        assert_eq!(first_id, alias_id, "the OS proves that these spellings are one target");
        assert!(owner.release_commit(held));
        let next = owner
            .commit(
                &CommitRequest::one(alias_entry, ResourceAccess::Write),
                b,
                ExecutionId::next(),
                None,
            )
            .expect("same target resumes after its holder releases");
        std::fs::write(root.path().join(alias), b"second").expect("next effect");
        assert_eq!(std::fs::read(root.path().join(first)).expect("one object"), b"second");
        assert!(owner.release_commit(next));
        eprintln!(
            "same-target proof {first:?}/{alias:?}: absent_slot_refused=true actual_identity_equal=true resumed_after_release=true"
        );
    }
}

#[test]
fn lost_comparison_facts_refuse_only_that_target_and_never_round_trip_as_authority() {
    let root = tempfile::tempdir().expect("new root");
    std::fs::write(root.path().join("other.txt"), b"unchanged").expect("other object");
    let session = SessionId::new();
    let facts = ResourceFacts::root(Arc::new(resolver(root.path(), WorkspaceId::new())), session);
    let slot = facts.slot(&"ß.txt".parse().expect("path")).expect("slot");
    let serialized = serde_json::to_value(&slot).expect("resource wire");
    assert!(
        serialized["parent"].get("name_comparison").is_none(),
        "comparison facts never enter operation fingerprints"
    );
    let restored: aura_workspace_domain::FileResource =
        serde_json::from_value(serialized).expect("old resource wire shape");
    let owner = EligibilityOwner::new();
    let refused = owner
        .commit(
            &CommitRequest::one(restored, ResourceAccess::Write),
            session,
            ExecutionId::next(),
            None,
        )
        .expect_err("serialized raw names do not establish a comparison table");
    assert!(!refused.retryable);
    assert_eq!(refused.held, "file.name_comparison_unavailable");
    let unrelated =
        facts.object(&"other.txt".parse().expect("other path")).expect("facts").expect("object");
    let commit = owner
        .commit(
            &CommitRequest::one(unrelated, ResourceAccess::Write),
            SessionId::new(),
            ExecutionId::next(),
            None,
        )
        .expect("the unavailable target cannot block another file or session");
    assert!(owner.release_commit(commit));
}
