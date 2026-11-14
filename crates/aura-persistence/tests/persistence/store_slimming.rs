//! `aura.db` keeps what nothing reads again: a write-ahead log that a big write left large, a text
//! index whose replaced entries were never merged away, every version of the model catalog, and the
//! pages all of these left free. A normal exit writes the log back and empties it, and notes a run
//! owed when enough of the file is free or in use beyond the last run; the run before the next start
//! (the launcher's `aura-core --store-slim`) rewrites the index, keeps only the current catalog and
//! rebuilds the file.
#![allow(clippy::unwrap_used)]

use std::{
    ops::ControlFlow,
    path::{Path, PathBuf},
};

use aura_domain_common::RuntimeInstanceId;
use aura_persistence::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
use rusqlite::Connection;
use tempfile::TempDir;

mod catalog;
mod exit;
mod rebuild;
mod text_index;

const MIB: u64 = 1024 * 1024;

struct Store {
    _directory: TempDir,
    database: PathBuf,
    artifacts: PathBuf,
}

impl Store {
    fn new() -> Self {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("data/aura.db");
        let artifacts = directory.path().join("data/artifact-objects");
        let store = Self { _directory: directory, database, artifacts };
        drop(store.start());
        store
    }

    fn start(&self) -> PersistenceCoordinator {
        let options = PersistenceOptions::new(
            self.database.clone(),
            self.artifacts.clone(),
            RuntimeInstanceId::new(),
        );
        match PersistenceCoordinator::start(options).unwrap() {
            PersistenceStartup::Ready(coordinator) => coordinator,
            PersistenceStartup::RecoveryRequired(recovery) => panic!("{}", recovery.reason),
        }
    }

    /// A connection of its own, as an older version or a test fixture writes the file.
    fn raw(&self) -> Connection {
        let connection = Connection::open(&self.database).unwrap();
        connection.busy_timeout(std::time::Duration::from_secs(5)).unwrap();
        connection
    }

    fn wal_bytes(&self) -> u64 {
        std::fs::metadata(format!("{}-wal", self.database.display())).map_or(0, |file| file.len())
    }

    fn free_bytes(&self) -> u64 {
        let db = self.raw();
        count(&db, "PRAGMA page_size") * count(&db, "PRAGMA freelist_count")
    }

    /// Leaves about `mib` MiB of the file free: written in one table, then dropped.
    fn leave_free(&self, mib: u64) {
        let db = self.raw();
        db.execute_batch("CREATE TABLE ballast(bytes BLOB NOT NULL)").unwrap();
        for _ in 0..mib {
            db.execute("INSERT INTO ballast VALUES (zeroblob(1048576))", []).unwrap();
        }
        db.execute_batch("DROP TABLE ballast; PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
    }

    fn slim(&self, free_space: u64) -> aura_persistence::slimming::Report {
        let room = move |_: &Path| Ok(free_space);
        aura_persistence::slimming::slim(&self.database, &self.artifacts, &room, &mut |_| {
            ControlFlow::Continue(())
        })
        .unwrap()
    }
}

fn count(db: &Connection, sql: &str) -> u64 {
    db.query_row(sql, [], |row| row.get::<_, Option<i64>>(0)).unwrap().unwrap_or(0) as u64
}
