use aura_persistence_core::{DatabaseConnection, open_reader};

#[test]
fn new_database_never_reuses_an_existing_file_or_creates_a_missing_reader_target() {
    let temp = tempfile::tempdir().expect("isolated directory");
    let path = temp.path().join("existing.db");
    std::fs::write(&path, b"user-data-canary").expect("fixture");
    assert!(DatabaseConnection::create_new(&path).is_err());
    assert_eq!(std::fs::read(&path).expect("untouched fixture"), b"user-data-canary");
    let missing = temp.path().join("missing.db");
    assert!(open_reader(&missing).is_err());
    assert!(!missing.exists());
}

#[test]
fn transactions_commit_state_and_outbox_together_and_roll_back_faults_and_duplicates() {
    let temp = tempfile::tempdir().expect("isolated directory");
    let path = temp.path().join("v2-test.db");
    let mut db = DatabaseConnection::create_new(&path).expect("new database");
    db.connection().execute_batch("CREATE TABLE state_fixture(id INTEGER PRIMARY KEY); CREATE TABLE outbox_fixture(id INTEGER PRIMARY KEY);").expect("test-owned schema");
    let failed: rusqlite::Result<()> = db.unit_of_work(|tx| {
        tx.execute("INSERT INTO state_fixture VALUES(1)", [])?;
        Err(rusqlite::Error::InvalidQuery)
    });
    assert!(failed.is_err());
    let commit = |tx: &rusqlite::Transaction<'_>| -> rusqlite::Result<()> {
        tx.execute("INSERT INTO state_fixture VALUES(1)", [])?;
        tx.execute("INSERT INTO outbox_fixture VALUES(1)", [])?;
        Ok(())
    };
    db.unit_of_work(commit).expect("atomic commit");
    assert!(db.unit_of_work(commit).is_err());
    let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: rusqlite::Result<()> = db.unit_of_work(|tx| {
            tx.execute("INSERT INTO state_fixture VALUES(2)", [])?;
            panic!("fault before outbox insertion");
        });
    }));
    assert!(panicked.is_err());
    drop(db);
    let reader = open_reader(&path).expect("reopen read-only");
    let counts: (i64, i64) = reader
        .query_row(
            "SELECT (SELECT count(*) FROM state_fixture), (SELECT count(*) FROM outbox_fixture)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("durable state");
    assert_eq!(counts, (1, 1));
    assert!(reader.execute("DELETE FROM state_fixture", []).is_err());
}
