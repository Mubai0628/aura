//! A stored index that no longer describes its folder. Archiving marks it untrusted instead of
//! deleting it inside the lifecycle's transaction, and its rows go in jobs that other registry
//! writes run between; a whole write in jobs stops between jobs when asked; replacing a large
//! stored index with a small one runs in jobs as well.
mod index_support;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use aura_domain_common::{CorrelationId, Revision, WorkspaceId};
use aura_persistence::{CatalogCommitOutcome, CatalogCommitRefusal, PersistenceCoordinator};
use aura_workspace_domain::{CatalogFreshness, WorkspaceState};
use index_support::{ENTRIES, entries, partition, start, workspace};

/// Runs `work` on its own thread while interactive registry writes go on. Returns what it
/// returned, the longest interactive write, how many ran, and whether the index of `watched`
/// was seen `rebuilding` meanwhile.
fn beside_writes<T: Send + 'static>(
    persistence: &PersistenceCoordinator,
    watched: WorkspaceId,
    work: impl FnOnce() -> T + Send + 'static,
) -> (T, Duration, usize, bool) {
    let worker = std::thread::spawn(work);
    let (mut longest, mut writes, mut rebuilding) = (Duration::ZERO, 0, false);
    while !worker.is_finished() {
        let started = Instant::now();
        workspace(persistence, &format!("other{writes}"));
        longest = longest.max(started.elapsed());
        writes += 1;
        rebuilding |= persistence.workspace(watched).expect("workspace").catalog_freshness
            == CatalogFreshness::Rebuilding;
        std::thread::sleep(Duration::from_millis(20));
    }
    (worker.join().expect("worker"), longest, writes, rebuilding)
}

fn large_index(persistence: &PersistenceCoordinator) -> WorkspaceId {
    let indexed = workspace(persistence, "large");
    persistence
        .replace_workspace_catalog(
            indexed,
            entries(ENTRIES),
            partition(ENTRIES),
            CorrelationId::new(),
        )
        .expect("the large index");
    indexed
}

#[test]
fn archiving_leaves_a_large_index_to_jobs_other_writes_run_between() {
    let directory = tempfile::tempdir().expect("data root");
    let persistence = start(directory.path());
    let indexed = large_index(&persistence);
    persistence.claim_workspace_catalog_instance(indexed).expect("an open index");
    let before = persistence.workspace_catalog_commit_position(indexed).expect("position");

    let mut archived = persistence.workspace(indexed).expect("workspace");
    archived.state = WorkspaceState::Archived;
    archived.catalog_freshness = CatalogFreshness::Unavailable;
    archived.revision = Revision::new(archived.revision.value() + 1);
    let started = Instant::now();
    persistence
        .update_workspace(archived, "workspace.archived", CorrelationId::new(), true)
        .expect("archive");
    let took = started.elapsed();
    eprintln!("archived a {ENTRIES}-entry index in {took:?}");
    assert!(took < Duration::from_millis(500), "the archive held the writer for {took:?}");

    // The rows are untrusted, and a write still in flight for them is refused at its next job.
    let after = persistence.workspace(indexed).expect("workspace");
    assert_eq!(after.catalog_freshness, CatalogFreshness::Rebuilding);
    let moved = persistence.workspace_catalog_commit_position(indexed).expect("position");
    assert_ne!(moved.catalog_revision, before.catalog_revision);
    assert_ne!(moved.instance_seq, before.instance_seq, "the open index's writer gives up");
    assert_eq!(persistence.archived_workspaces_with_index().expect("leftovers"), vec![indexed]);

    let clearing = persistence.clone();
    let (removed, longest, writes, _) = beside_writes(&persistence, indexed, move || {
        clearing.clear_archived_workspace_index(indexed).expect("cleared")
    });
    eprintln!("{removed} rows removed; {writes} writes meanwhile, the longest {longest:?}");
    assert!(removed >= ENTRIES as u64 * 2, "the entries and their text rows ({removed})");
    assert!(longest < Duration::from_millis(800), "a write waited {longest:?}");
    assert!(persistence.archived_workspaces_with_index().expect("leftovers").is_empty());
    assert!(persistence.load_workspace_catalog(indexed).expect("index").0.is_empty());
    assert!(persistence.workspace_content_paths(indexed).expect("text").is_empty());
}

#[test]
fn a_whole_write_in_jobs_stops_between_jobs_when_asked() {
    let directory = tempfile::tempdir().expect("data root");
    let persistence = start(directory.path());
    let indexed = workspace(&persistence, "large");
    let position = persistence.workspace_catalog_commit_position(indexed).expect("position");
    let stop = Arc::new(AtomicBool::new(false));
    let (writing, writer_stop) = (persistence.clone(), Arc::clone(&stop));
    let writer = std::thread::spawn(move || {
        writing.replace_workspace_catalog_guarded(
            indexed,
            position,
            entries(ENTRIES),
            partition(ENTRIES),
            CorrelationId::new(),
            Some(writer_stop),
        )
    });
    let deadline = Instant::now() + Duration::from_secs(60);
    while persistence.workspace(indexed).expect("workspace").catalog_freshness
        != CatalogFreshness::Rebuilding
    {
        assert!(Instant::now() < deadline && !writer.is_finished(), "the write never claimed");
        std::thread::sleep(Duration::from_millis(1));
    }
    let stopped = Instant::now();
    stop.store(true, Ordering::Release);
    let outcome = writer.join().expect("writer").expect("the stopped write");
    let took = stopped.elapsed();
    assert!(
        matches!(outcome, CatalogCommitOutcome::Refused(CatalogCommitRefusal::Stopped)),
        "{outcome:?}"
    );
    assert!(took < Duration::from_secs(2), "the write ran {took:?} after it was stopped");
    // Not trusted, so the next open walks the folder again.
    assert_eq!(
        persistence.workspace(indexed).expect("workspace").catalog_freshness,
        CatalogFreshness::Rebuilding
    );
}

#[test]
fn replacing_a_large_index_with_a_small_one_runs_in_jobs() {
    let directory = tempfile::tempdir().expect("data root");
    let persistence = start(directory.path());
    let indexed = large_index(&persistence);

    let writing = persistence.clone();
    let (_, longest, writes, rebuilding) = beside_writes(&persistence, indexed, move || {
        writing
            .replace_workspace_catalog(indexed, entries(10), partition(10), CorrelationId::new())
            .expect("the small index")
    });
    eprintln!(
        "{ENTRIES} entries replaced by 10; {writes} writes meanwhile, the longest {longest:?}"
    );
    assert!(rebuilding, "the old rows were removed in jobs");
    assert!(longest < Duration::from_millis(800), "a write waited {longest:?}");
    let (stored, _, _) = persistence.load_workspace_catalog(indexed).expect("the stored index");
    assert_eq!(stored.len(), 10);
    assert_eq!(persistence.workspace_content_paths(indexed).expect("text paths").len(), 10);
    assert_eq!(
        persistence.workspace(indexed).expect("workspace").catalog_freshness,
        CatalogFreshness::Fresh
    );
}
