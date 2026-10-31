//! A kept read-only connection's number names that one connection for the life of the process.
//! What a numbered connection read is kept with what it saw (`agent_application::history_memo`),
//! and `PRAGMA data_version` is the connection's own, so a store opened again must never lend a
//! number an earlier connection had: the new connection could report the same data version and be
//! handed the page the earlier one read before the store changed.
use super::super::readers::Readers;

fn database() -> (tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("store.db");
    rusqlite::Connection::open(&path)
        .and_then(|db| db.execute_batch("CREATE TABLE facts(x)"))
        .expect("a store to read");
    (directory, path)
}

#[test]
fn a_store_opened_again_never_lends_a_number_an_earlier_connection_had() {
    let (_directory, path) = database();
    let first = Readers::open(path.clone()).expect("the first store's readers");
    let mut lent = Vec::new();
    for _ in 0..2 {
        lent.push(first.take().expect("a reader of the first store").0);
    }
    drop(first);
    let again = Readers::open(path).expect("the store opened again");
    for _ in 0..2 {
        let (number, _) = again.take().expect("a reader of the store opened again");
        assert!(!lent.contains(&number), "number {number} was lent before: {lent:?}");
        lent.push(number);
    }
}
