//! Bounded startup-cost probe (temporary, evidence only).
//!
//! Builds a workspace with the scale of the reported real development data
//! (≈14 000 entries over ≈300 partitions), then measures what each open stage
//! costs and how much of a warm open is repeated work. Printed with --nocapture.
use std::sync::Arc;
use std::time::Instant;

use aura_domain_common::{CorrelationId, RuntimeInstanceId};
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use aura_workspace::{WorkspaceRuntime, WorkspaceRuntimeOptions};
use aura_workspace_domain::{WorkspaceCreateCommand, WorkspaceCreateDisposition};

const PARTITIONS: usize = 307;
const FILES_PER_PARTITION: usize = 45;
const FILE_BYTES: usize = 2048;

fn persistence(data: &std::path::Path) -> Arc<PersistenceCoordinator> {
    match PersistenceCoordinator::start(PersistenceOptions::new(
        data.join("aura.sqlite3"),
        data.join("artifacts"),
        RuntimeInstanceId::new(),
    ))
    .expect("persistence startup")
    {
        PersistenceStartup::Ready(value) => Arc::new(value),
        PersistenceStartup::RecoveryRequired(value) => {
            panic!("unexpected recovery: {}", value.reason)
        }
    }
}

#[test]
fn startup_cost_probe() {
    let data = tempfile::tempdir().expect("data root");
    let workspace = tempfile::tempdir().expect("workspace root");
    let mut body = String::with_capacity(FILE_BYTES);
    while body.len() < FILE_BYTES {
        body.push_str("the quick brown fox jumps over the lazy dog\n");
    }
    let extension = std::env::var("AURA_PROBE_EXTENSION").unwrap_or_else(|_| "txt".to_owned());
    let started = Instant::now();
    for partition in 0..PARTITIONS {
        let directory = workspace.path().join(format!("partition-{partition:03}"));
        std::fs::create_dir_all(&directory).expect("partition");
        for file in 0..FILES_PER_PARTITION {
            std::fs::write(directory.join(format!("file-{file:03}.{extension}")), body.as_bytes())
                .expect("fixture file");
        }
    }
    println!("extension: {extension}");
    println!(
        "fixture: {} entries in {} ms ({} bytes each)",
        PARTITIONS * FILES_PER_PARTITION + PARTITIONS + 1,
        started.elapsed().as_millis(),
        FILE_BYTES
    );

    let cold_started = Instant::now();
    let runtime =
        WorkspaceRuntime::open(persistence(data.path()), WorkspaceRuntimeOptions::default())
            .expect("first open");
    println!("first open total          : {} ms", cold_started.elapsed().as_millis());
    println!("  report: {:?}", runtime.open_report().expect("report"));
    let created = runtime
        .create(
            WorkspaceCreateCommand {
                selected_root: workspace.path().display().to_string(),
                display_name: "startup probe".to_owned(),
            },
            CorrelationId::new(),
        )
        .expect("create workspace");
    assert_eq!(created.disposition, WorkspaceCreateDisposition::Created);
    drop(runtime);

    // Warm open: the catalog is already persisted and untouched on disk.
    let warm_started = Instant::now();
    let runtime =
        WorkspaceRuntime::open(persistence(data.path()), WorkspaceRuntimeOptions::default())
            .expect("second open");
    println!("warm  open total          : {} ms", warm_started.elapsed().as_millis());
    println!("  report: {:?}", runtime.open_report().expect("report"));
    drop(runtime);

    // Settled open: the previous pass stamped every file's change generation, so
    // nothing has to be read at all.
    let settled_started = Instant::now();
    let runtime =
        WorkspaceRuntime::open(persistence(data.path()), WorkspaceRuntimeOptions::default())
            .expect("third open");
    println!("settled open total        : {} ms", settled_started.elapsed().as_millis());
    println!("  report: {:?}", runtime.open_report().expect("report"));
    drop(runtime);

    // Floor: listing every file's metadata with no index work at all.
    let listing_started = Instant::now();
    let mut entries = 0_u64;
    let mut bytes = 0_u64;
    for partition in 0..PARTITIONS {
        let directory = workspace.path().join(format!("partition-{partition:03}"));
        for child in std::fs::read_dir(&directory).expect("list partition") {
            let metadata = child.expect("entry").metadata().expect("metadata");
            entries += 1;
            bytes += metadata.len();
        }
    }
    println!(
        "metadata-only walk floor  : {} ms for {entries} files / {bytes} bytes",
        listing_started.elapsed().as_millis()
    );

    // Floor: reading every file's bytes with no index work at all.
    let reading_started = Instant::now();
    let mut read = 0_u64;
    for partition in 0..PARTITIONS {
        let directory = workspace.path().join(format!("partition-{partition:03}"));
        for file in 0..FILES_PER_PARTITION {
            read += std::fs::read(directory.join(format!("file-{file:03}.txt")))
                .expect("read fixture")
                .len() as u64;
        }
    }
    println!(
        "content-read floor         : {} ms for {read} bytes",
        reading_started.elapsed().as_millis()
    );
}
