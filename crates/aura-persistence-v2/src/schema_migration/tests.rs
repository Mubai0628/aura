use super::*;
use aura_persistence_core::DatabaseConnection;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};

#[test]
fn failure_after_prior_ddl_rolls_back_every_module_and_digest() {
    let temp = tempfile::tempdir().expect("temporary database");
    let path = temp.path().join("aura-v2.db");
    let security = include_str!("../security.sql")
        .split_once("\nCREATE TABLE native_session_consents")
        .expect("security predecessor")
        .0;
    let security: &'static str = Box::leak(format!("{security}\n").into_boxed_str());
    let modules = MODULES
        .iter()
        .map(|module| SchemaModule {
            owner: module.owner,
            sql: match module.owner {
                "tool" => include_str!("../../tests/fixtures/tool-before-l04.sql"),
                "security" => security,
                _ => module.sql,
            },
        })
        .collect::<Vec<_>>();
    let mut database = DatabaseConnection::create_new(&path).expect("db");
    schema::initialize(&mut database, &modules).expect("predecessors");
    drop(database);
    let planned = plan_migration(&path, Check::Whole).expect("plan").expect("two module upgrade");
    assert_eq!(planned.migrations.len(), 2);
    rollback_and_retry(&path, &planned);
}

fn rollback_and_retry(path: &Path, planned: &PlannedMigrations) {
    rollback_and_retry_at(path, planned, "tool_package_trust", "tool_package_files");
}

/// Refuses `CREATE TABLE <denied>` once, after `<earlier>` was created in the same
/// transaction, and checks that nothing of the migration stays and a retry succeeds.
fn rollback_and_retry_at(
    path: &Path,
    planned: &PlannedMigrations,
    denied: &'static str,
    earlier: &'static str,
) {
    let before = live_structure(&aura_persistence_core::open_reader(path).expect("reader"))
        .expect("structure");
    let mut writer = rusqlite::Connection::open(path).expect("writer");
    writer
        .authorizer(Some(move |context: AuthContext<'_>| match context.action {
            AuthAction::CreateTable { table_name } if table_name == denied => Authorization::Deny,
            _ => Authorization::Allow,
        }))
        .expect("install temporary SQLite fault injection");
    let transaction = writer
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .expect("transaction");
    let error = apply_plan(&transaction, planned).expect_err("DDL refused after earlier creates");
    assert!(
        matches!(error, Error::Sqlite(ref failure) if failure.sqlite_error_code()==Some(rusqlite::ErrorCode::AuthorizationForStatementDenied)),
        "test must reach SQLite's actual authorization refusal"
    );
    let during = live_structure(&transaction).expect("partially applied transaction");
    assert!(during.iter().any(|(name, _)| name == earlier), "earlier tool DDL really ran");
    if planned.migrations.iter().any(|migration| migration.owner == "security") {
        assert!(
            during.iter().any(|(name, _)| name == "native_session_consents"),
            "earlier security module really ran"
        );
    }
    drop(transaction);
    drop(writer);
    let reader = aura_persistence_core::open_reader(path).expect("reader");
    assert_eq!(live_structure(&reader).expect("after rollback"), before);
    for migration in &planned.migrations {
        let digest: String = reader
            .query_row(
                "SELECT digest FROM v2_schema_metadata WHERE owner=?1",
                [migration.owner],
                |row| row.get(0),
            )
            .expect("digest");
        assert_eq!(digest, migration.digest, "no half-migrated module");
    }
    // A normal retry still works; no recovery rewrite or partial-table fallback.
    drop(reader);
    migrate(path).expect("retry after transient failure");
    assert!(plan_migration(path, Check::Whole).expect("reopen").is_none());
}

#[test]
#[ignore = "requires the separately authorized TEMP owner copy for rollback"]
fn owner_copy_rolls_back_partial_ddl_and_can_restart_after_retry() {
    let root = std::fs::canonicalize(
        std::env::var_os("AURA_L04_OWNER_ROLLBACK_COPY").expect("explicit COPY path"),
    )
    .expect("copy");
    assert!(
        root.starts_with(std::fs::canonicalize(std::env::temp_dir()).expect("temporary root"))
            && root
                .file_name()
                .expect("name")
                .to_string_lossy()
                .starts_with("aura-l04-owner-copy-rollback-"),
        "never accept an original or arbitrary database"
    );
    let path = root.join("aura-v2.db");
    let event_count = || {
        aura_persistence_core::open_reader(&path)
            .expect("copy reader")
            .query_row("SELECT count(*) FROM events", [], |row| row.get::<_, i64>(0))
            .expect("event count only")
    };
    let before = event_count();
    let planned = plan_migration(&path, Check::Whole)
        .expect("owner copy plan")
        .expect("unmodified predecessor");
    assert_eq!(planned.migrations.len(), 1);
    assert_eq!(planned.migrations[0].owner, "tool");
    rollback_and_retry(&path, &planned);
    crate::V2Store::open(&root).expect("normal startup after rollback and retry");
    assert_eq!(event_count(), before);
    println!(
        "L04 OWNER COPY rollback/retry/startup: events {before}->{}; no existing event rewritten",
        event_count()
    );
}

/// A store that already has the package tables: this migration only adds the Hook
/// tables; the refusal is placed on the second of them.
#[test]
#[ignore = "requires the separately authorized TEMP owner copy for rollback"]
fn owner_copy_rolls_back_a_partial_hook_migration_and_can_restart_after_retry() {
    let root = std::fs::canonicalize(
        std::env::var_os("AURA_L07_OWNER_ROLLBACK_COPY").expect("explicit COPY path"),
    )
    .expect("copy");
    assert!(
        root.starts_with(std::fs::canonicalize(std::env::temp_dir()).expect("temporary root"))
            && root
                .file_name()
                .expect("name")
                .to_string_lossy()
                .starts_with("aura-l07-owner-copy-rollback-"),
        "never accept an original or arbitrary database"
    );
    let path = root.join("aura-v2.db");
    let event_count = || {
        aura_persistence_core::open_reader(&path)
            .expect("copy reader")
            .query_row("SELECT count(*) FROM events", [], |row| row.get::<_, i64>(0))
            .expect("event count only")
    };
    let before = event_count();
    let planned = plan_migration(&path, Check::Whole)
        .expect("owner copy plan")
        .expect("unmodified predecessor");
    assert_eq!(planned.migrations.len(), 1);
    assert_eq!(planned.migrations[0].owner, "tool");
    rollback_and_retry_at(&path, &planned, "hook_decisions", "hook_invocations");
    crate::V2Store::open(&root).expect("normal startup after rollback and retry");
    assert_eq!(event_count(), before);
    println!(
        "L07 OWNER COPY rollback/retry/startup: events {before}->{}; no existing event rewritten",
        event_count()
    );
}

/// From the frozen schema before MCP, a refusal of the second MCP table after the first was
/// created leaves nothing behind, and a retry and a normal startup succeed.
#[test]
fn a_refused_second_mcp_table_rolls_back_and_the_retry_succeeds() {
    let temp = tempfile::tempdir().expect("temporary database");
    let path = temp.path().join("aura-v2.db");
    let modules = MODULES
        .iter()
        .map(|module| SchemaModule {
            owner: module.owner,
            sql: match module.owner {
                "tool" => include_str!("../../tests/fixtures/tool-before-l08.sql"),
                _ => module.sql,
            },
        })
        .collect::<Vec<_>>();
    let mut database = DatabaseConnection::create_new(&path).expect("db");
    schema::initialize(&mut database, &modules).expect("L07 predecessor");
    drop(database);
    let planned =
        plan_migration(&path, Check::Whole).expect("plan").expect("the tool module upgrades");
    assert_eq!(planned.migrations.len(), 1);
    rollback_and_retry_at(&path, &planned, "mcp_tool_revisions", "mcp_catalogs");
}

/// From the frozen schema before MCP sign-ins, a refusal of the OAuth table after the secret table
/// was created leaves nothing behind, and a retry and a normal startup succeed.
#[test]
fn a_refused_mcp_oauth_table_rolls_back_and_the_retry_succeeds() {
    let temp = tempfile::tempdir().expect("temporary database");
    let path = temp.path().join("aura-v2.db");
    let modules = MODULES
        .iter()
        .map(|module| SchemaModule {
            owner: module.owner,
            sql: match module.owner {
                "tool" => include_str!("../../tests/fixtures/tool-before-l08b5.sql"),
                _ => module.sql,
            },
        })
        .collect::<Vec<_>>();
    let mut database = DatabaseConnection::create_new(&path).expect("db");
    schema::initialize(&mut database, &modules).expect("L08 predecessor");
    drop(database);
    let planned =
        plan_migration(&path, Check::Whole).expect("plan").expect("the tool module upgrades");
    assert_eq!(planned.migrations.len(), 1);
    rollback_and_retry_at(&path, &planned, "mcp_oauth_sessions", "mcp_secrets");
}

/// On the owner's store: the refusal is placed on the second MCP table.
#[test]
#[ignore = "requires the separately authorized TEMP owner copy for rollback"]
fn owner_copy_rolls_back_a_partial_mcp_migration_and_can_restart_after_retry() {
    let root = std::fs::canonicalize(
        std::env::var_os("AURA_L08_OWNER_ROLLBACK_COPY").expect("explicit COPY path"),
    )
    .expect("copy");
    assert!(
        root.starts_with(std::fs::canonicalize(std::env::temp_dir()).expect("temporary root"))
            && root
                .file_name()
                .expect("name")
                .to_string_lossy()
                .starts_with("aura-l08-owner-copy-rollback-"),
        "never accept an original or arbitrary database"
    );
    let path = root.join("aura-v2.db");
    let event_count = || {
        aura_persistence_core::open_reader(&path)
            .expect("copy reader")
            .query_row("SELECT count(*) FROM events", [], |row| row.get::<_, i64>(0))
            .expect("event count only")
    };
    let before = event_count();
    let planned = plan_migration(&path, Check::Whole)
        .expect("owner copy plan")
        .expect("unmodified predecessor");
    assert_eq!(planned.migrations.len(), 1);
    assert_eq!(planned.migrations[0].owner, "tool");
    rollback_and_retry_at(&path, &planned, "mcp_tool_revisions", "mcp_catalogs");
    crate::V2Store::open(&root).expect("normal startup after rollback and retry");
    assert_eq!(event_count(), before);
    println!(
        "L08 OWNER COPY rollback/retry/startup: events {before}->{}; no existing event rewritten",
        event_count()
    );
}

/// What an update asks before it copies this store: a predecessor needs migrating, a store this
/// build created does not, and a missing one is new.
#[test]
fn migration_pending_answers_as_migrate_would_decide() {
    let temp = tempfile::tempdir().expect("temporary database");
    let path = temp.path().join("aura-v2.db");
    assert!(!migration_pending(&path).expect("missing"));
    let modules = MODULES
        .iter()
        .map(|module| SchemaModule {
            owner: module.owner,
            sql: match module.owner {
                "tool" => include_str!("../../tests/fixtures/tool-before-l08.sql"),
                _ => module.sql,
            },
        })
        .collect::<Vec<_>>();
    let mut database = DatabaseConnection::create_new(&path).expect("db");
    schema::initialize(&mut database, &modules).expect("L07 predecessor");
    drop(database);
    assert!(migration_pending(&path).expect("predecessor"));
    migrate(&path).expect("migrated");
    assert!(!migration_pending(&path).expect("current"));
}
