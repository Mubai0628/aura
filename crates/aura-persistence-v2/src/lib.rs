#![forbid(unsafe_code)]

pub mod agent;
pub mod chrome_sites;
pub mod context;
pub mod execution;
pub mod file_consents;
pub mod hooks;
pub mod mcp;
pub mod memory;
pub mod memory_consents;
pub mod model;
pub mod network_consents;
pub mod packages;
mod record_compatibility;
pub use record_compatibility::{RECORDS_READABLE_SINCE, copy_before_start};
mod schema_migration;
pub use schema_migration::migration_pending;
pub mod security;
pub mod security_approval;
pub mod session_purge;
pub mod terminal;
pub mod tool;
pub mod workspace;

use aura_persistence_core::{
    DatabaseConnection, PersistenceCoreError as Error, StoreResult,
    schema::{self, OpenCheck, PendingCheck, SchemaModule},
};
use std::path::{Path, PathBuf};
use std::time::Duration;

// Composition only: SQL and mapping remain in their domain modules. R03/R09
// consume these stores without importing the legacy Coordinator or migrations.
pub const MODULES: &[SchemaModule] = &[
    SchemaModule { owner: "workspace", sql: include_str!("workspace.sql") },
    SchemaModule { owner: "security", sql: include_str!("security.sql") },
    SchemaModule { owner: "execution", sql: include_str!("execution.sql") },
    SchemaModule { owner: "model", sql: include_str!("model.sql") },
    SchemaModule { owner: "context", sql: include_str!("context.sql") },
    SchemaModule { owner: "tool", sql: include_str!("tool.sql") },
    SchemaModule { owner: "agent", sql: include_str!("agent.sql") },
    // `memory.sql` is deliberately absent: it is the schema of each Vault's own
    // disposable cache (see `memory`), never a table set in this shared database.
];

pub struct V2Store {
    pub database: DatabaseConnection,
    root: PathBuf,
    pending_check: PendingCheck,
}

/// Written when a process exits normally, removed by the next open. Its
/// absence means the previous process crashed or was ended without saying so, and the store owes a
/// check of every page, index and key.
const CLEAN_EXIT: &str = "clean-exit.json";
/// How long the exit's checkpoint waits for a reader still on the write-ahead log. Main
/// stops a Core that has not recorded its exit within three seconds.
const EXIT_CHECKPOINT_WAIT: Duration = Duration::from_secs(1);
/// The writer's usual wait for a lock (`DatabaseConnection`).
const LOCK_WAIT: Duration = Duration::from_secs(5);
impl V2Store {
    /// Requires a new explicit directory. No AppData lookup or legacy fallback.
    pub fn create(root: &Path) -> StoreResult<Self> {
        if !root.is_absolute() {
            return Err(Error::Invalid("absolute isolated V2 directory"));
        }
        std::fs::create_dir(root)?;
        let mut database = DatabaseConnection::create_new(&root.join("aura-v2.db"))?;
        schema::initialize(&mut database, MODULES)?;
        std::fs::write(
            root.join("v2-development.json"),
            b"{\"schema_version\":1,\"namespace\":\"aura-v2-development\"}\n",
        )?;
        Ok(Self { database, root: root.to_path_buf(), pending_check: PendingCheck::None })
    }
    pub fn open(root: &Path) -> StoreResult<Self> {
        if !root.is_absolute() {
            return Err(Error::Invalid("absolute isolated V2 directory"));
        }
        let marker: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("v2-development.json"))?)?;
        if marker != serde_json::json!({"schema_version":1,"namespace":"aura-v2-development"}) {
            return Err(Error::Invalid("V2 development namespace"));
        }
        let database_path = root.join("aura-v2.db");
        // Removed before anything else happens: a crash from here on finds no marker.
        let marker = root.join(CLEAN_EXIT);
        let pending_check = if marker.exists() && std::fs::remove_file(&marker).is_ok() {
            PendingCheck::Quick
        } else {
            PendingCheck::Full
        };
        schema_migration::migrate(&database_path)?;
        // Only the structure is checked before the writer opens.
        // The full check took 154 s on the owner's 7.1 GB store, past the two minutes Main waits
        // for Core, so Core never started; it is owed to the background instead. With WAL and
        // `synchronous=FULL` a process that ends mid-write leaves no damaged page to write into.
        let mut store = Self {
            database: schema::open_checked(&database_path, MODULES, OpenCheck::Structure)?,
            root: root.to_path_buf(),
            pending_check,
        };
        let started = std::time::Instant::now();
        let until = started + std::time::Duration::from_millis(250);
        let mut poll_cursor = agent::desktop::PollBackfillCursor::default();
        let mut repair_scan = agent::stream::RepairScan::default();
        // Each kind is its own transaction. A fault in one must not roll the
        // other back, and a large old object must not keep the database closed.
        // Finding streams only reads, and its sweep stops at the same deadline.
        loop {
            let found = agent::stream::discover_outstanding(
                store.database.connection(),
                &mut repair_scan,
                Some(until),
            );
            let streams = found
                .and_then(|()| {
                    agent::stream::repair_pass(
                        &mut store.database,
                        &mut repair_scan,
                        aura_domain_common::UtcTimestamp::now(),
                        262_144,
                    )
                })
                .map(|copied| copied.saturating_add(u32::from(repair_scan.sweeping())));
            let polls = store.database.unit_of_work(|tx| {
                agent::desktop::backfill_turn_poll(tx, 262_144, &mut poll_cursor)
            });
            let (Ok(streams), Ok(polls)) = (streams, polls) else {
                break;
            };
            if streams.saturating_add(polls) == 0
                || started.elapsed() >= std::time::Duration::from_millis(250)
            {
                break;
            }
        }
        Ok(store)
    }
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The check this store owes; its owner runs [`schema::background_check`] while the
    /// store serves.
    #[must_use]
    pub fn pending_check(&self) -> PendingCheck {
        self.pending_check
    }

    /// This process is exiting normally. The write-ahead log goes into the database
    /// first, then the marker the next open looks for. A reader still on the log (the
    /// background check, or one a request opened) is waited for a second at most; what it holds
    /// stays in the log, which the next open reads as it always does.
    pub fn mark_clean_exit(&self) -> StoreResult<()> {
        let connection = self.database.connection();
        connection.busy_timeout(EXIT_CHECKPOINT_WAIT)?;
        let checkpoint =
            connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get::<_, i64>(0));
        connection.busy_timeout(LOCK_WAIT)?;
        checkpoint?;
        std::fs::write(
            self.root.join(CLEAN_EXIT),
            format!(
                "{{\"schema_version\":1,\"at\":\"{}\"}}
",
                aura_domain_common::UtcTimestamp::now()
            ),
        )?;
        Ok(())
    }

    /// The database file this store owns, for a read-only connection to the same data.
    #[must_use]
    pub fn database_path(&self) -> PathBuf {
        self.root.join("aura-v2.db")
    }
}
