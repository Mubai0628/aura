//! Writing a large workspace index in batches: interactive registry writes run between the
//! batches instead of waiting for the whole index, and an index whose batches did not all land
//! says so, so the next open walks the folder again instead of trusting it.
mod index_support;

use std::time::{Duration, Instant};

use aura_domain_common::{CorrelationId, Revision};
use aura_workspace_domain::CatalogFreshness;
use index_support::{ENTRIES, entries, partition, start, workspace};

#[test]
fn interactive_writes_run_between_the_batches_of_a_large_index() {
    let directory = tempfile::tempdir().expect("data root");
    let persistence = start(directory.path());
    let indexed = workspace(&persistence, "large");
    let before = persistence.workspace(indexed).expect("workspace").revision;

    let writing = persistence.clone();
    let writer = std::thread::spawn(move || {
        let started = Instant::now();
        let revision = writing
            .replace_workspace_catalog(
                indexed,
                entries(ENTRIES),
                partition(ENTRIES),
                CorrelationId::new(),
            )
            .expect("the large index");
        (revision, started.elapsed())
    });
    // Interactive writes while the index is written: each one gets the writer between batches.
    let (mut longest, mut writes) = (Duration::ZERO, 0);
    while !writer.is_finished() {
        let started = Instant::now();
        workspace(&persistence, &format!("other{writes}"));
        longest = longest.max(started.elapsed());
        writes += 1;
        std::thread::sleep(Duration::from_millis(20));
    }
    let (revision, took) = writer.join().expect("writer");
    eprintln!(
        "{ENTRIES} entries written in {took:?}; {writes} writes meanwhile, the longest {longest:?}"
    );
    assert!(writes >= 3, "the index took long enough to measure ({writes} writes)");
    assert!(longest < Duration::from_millis(800), "a write waited {longest:?} for the index");

    let (stored, partitions, stored_revision) =
        persistence.load_workspace_catalog(indexed).expect("the stored index");
    assert_eq!(stored.len(), ENTRIES);
    assert_eq!(partitions.len(), 1);
    assert_eq!(stored_revision, revision);
    assert_eq!(persistence.workspace_content_paths(indexed).expect("text paths").len(), ENTRIES);
    let after = persistence.workspace(indexed).expect("workspace");
    assert_eq!(after.catalog_freshness, CatalogFreshness::Fresh);
    assert_eq!(after.revision, Revision::new(before.value() + 1), "one workspace revision");

    // Writing it again replaces it, in batches as well.
    persistence
        .replace_workspace_catalog(
            indexed,
            entries(ENTRIES / 2),
            partition(ENTRIES / 2),
            CorrelationId::new(),
        )
        .expect("the smaller index");
    let (stored, _, _) = persistence.load_workspace_catalog(indexed).expect("the stored index");
    assert_eq!(stored.len(), ENTRIES / 2);
    assert_eq!(
        persistence.workspace_content_paths(indexed).expect("text paths").len(),
        ENTRIES / 2
    );
}

#[test]
fn an_overtaken_write_in_batches_leaves_the_other_writers_index_whole() {
    let directory = tempfile::tempdir().expect("data root");
    let persistence = start(directory.path());
    let indexed = workspace(&persistence, "large");
    let position = persistence.workspace_catalog_commit_position(indexed).expect("position");
    let writing = persistence.clone();
    let writer = std::thread::spawn(move || {
        writing.replace_workspace_catalog_guarded(
            indexed,
            position,
            entries(ENTRIES),
            partition(ENTRIES),
            CorrelationId::new(),
            None,
        )
    });
    // Once the write has claimed the index, another writer moves it.
    let deadline = Instant::now() + Duration::from_secs(60);
    while persistence.workspace(indexed).expect("workspace").catalog_freshness
        != CatalogFreshness::Rebuilding
    {
        assert!(Instant::now() < deadline && !writer.is_finished(), "the write never claimed");
        std::thread::sleep(Duration::from_millis(1));
    }
    persistence
        .replace_workspace_catalog(indexed, entries(10), partition(10), CorrelationId::new())
        .expect("another writer");
    let outcome = writer.join().expect("writer").expect("the overtaken write");
    assert!(
        matches!(
            outcome,
            aura_persistence::CatalogCommitOutcome::Refused(
                aura_persistence::CatalogCommitRefusal::RevisionMoved { .. }
            )
        ),
        "{outcome:?}"
    );
    // The other writer's small index is whole; the overtaken write left nothing of its own.
    let (stored, _, _) = persistence.load_workspace_catalog(indexed).expect("the stored index");
    assert_eq!(stored.len(), 10, "the other writer's index");
    assert_eq!(
        persistence.workspace(indexed).expect("workspace").catalog_freshness,
        CatalogFreshness::Fresh
    );
}
