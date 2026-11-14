//! The text index keeps every entry a replaced file had until its segments merge, which for a
//! workspace edited all day is seldom: the index grows far past its text. The run rewrites it
//! (FTS5 `optimize`) once it is many times its text, and leaves a healthy one alone.
use super::*;

/// About `bytes` of text made of words from a large made-up vocabulary, the same for each `seed`.
fn text(seed: u64, bytes: usize) -> String {
    let mut state = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    let mut text = String::with_capacity(bytes + 16);
    while text.len() < bytes {
        state =
            state.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        text.push_str(&format!("w{:x} ", (state >> 33) % 60_000));
    }
    text
}

/// Writes `files` texts of about `bytes` each into the index, then replaces them all `rounds`
/// times, each round in its own transaction, with no merging in between.
fn churn(store: &Store, files: u64, bytes: usize, rounds: u64) {
    let mut db = store.raw();
    db.execute_batch(
        "INSERT INTO workspace_content_fts(workspace_content_fts, rank) VALUES('automerge', 0);
         INSERT INTO workspace_content_fts(workspace_content_fts, rank) VALUES('crisismerge', 1999);",
    )
    .unwrap();
    for round in 0..=rounds {
        let tx = db.transaction().unwrap();
        tx.execute("DELETE FROM workspace_content_fts", []).unwrap();
        for file in 0..files {
            tx.execute(
                "INSERT INTO workspace_content_fts(rowid, workspace_id, relative_path, content)
                 VALUES (?1, 'workspace', ?2, ?3)",
                rusqlite::params![
                    file as i64 + 1,
                    format!("src/file-{file}.rs"),
                    text(file + round, bytes)
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
    db.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
}

fn index_bytes(db: &Connection) -> u64 {
    count(db, "SELECT SUM(length(block)) FROM workspace_content_fts_data")
}

fn text_bytes(db: &Connection) -> u64 {
    count(db, "SELECT SUM(length(CAST(c2 AS BLOB))) FROM workspace_content_fts_content")
}

fn found(db: &Connection, word: &str) -> Vec<(i64, String)> {
    let mut statement = db
        .prepare(
            "SELECT rowid, relative_path FROM workspace_content_fts
             WHERE workspace_content_fts MATCH ?1 ORDER BY rowid",
        )
        .unwrap();
    statement
        .query_map([word], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn an_index_many_times_its_text_is_rewritten_and_finds_the_same_files() {
    let store = Store::new();
    churn(&store, 400, 6_000, 14);
    let db = store.raw();
    let (before, text) = (index_bytes(&db), text_bytes(&db));
    assert!(before > 16 * MIB && before > 2 * text, "index {before}, text {text}");
    let words = ["w1f", "w2a7", "wbeef"].map(|word| found(&db, word));
    assert!(words.iter().all(|files| !files.is_empty()), "{words:?}");
    drop(db);

    let report = store.slim(u64::MAX / 4);
    let index = report.text_index.expect("the index was rewritten");
    let db = store.raw();
    assert_eq!((index.before_bytes, index.after_bytes), (before, index_bytes(&db)));
    // Back to about the size of an index written afresh, and no longer owed a rewrite.
    assert!(index.after_bytes * 10 < before, "{index:?}, text {text}");
    assert!(index.after_bytes <= 2 * text, "{index:?}, text {text}");
    assert_eq!(["w1f", "w2a7", "wbeef"].map(|word| found(&db, word)), words);
    assert_eq!(text_bytes(&db), text, "the text itself is kept");
}

#[test]
fn an_index_near_the_size_of_its_text_is_left_alone() {
    let store = Store::new();
    churn(&store, 300, 6_000, 0);
    let before = index_bytes(&store.raw());
    let report = store.slim(u64::MAX / 4);
    assert_eq!(report.text_index, None);
    assert_eq!(index_bytes(&store.raw()), before);
}
