//! The newest change of one projection in `projection_outbox`. The table has no index on the
//! projection, and a lookup that sorted every change of it by revision read the whole table:
//! 35 MB on a long-used store, several times at each start and again during a conversation.
//!
//! Every change takes its projection's next revision as it is appended
//! (`next_projection_revision`), so the newest change is the last one appended, and a projection
//! without a revision has no change. Read from the end, the table is read back only as far as
//! that change; the changes of the current runtime, the only ones its keys match, are the newest.
use rusqlite::{Connection, OptionalExtension, params};

/// The payload of the newest change of the projection `kind` and `key`.
pub(crate) fn latest_payload(
    db: &Connection,
    kind: &str,
    key: &str,
) -> rusqlite::Result<Option<String>> {
    if !has_revision(db, kind, key)? {
        return Ok(None);
    }
    db.query_row(
        "SELECT payload_json FROM projection_outbox WHERE projection_kind=?1 AND projection_key=?2 \
         ORDER BY outbox_id DESC LIMIT 1",
        params![kind, key],
        |row| row.get(0),
    )
    .optional()
}

/// Whether the projection `kind` and `key` has any change.
pub(crate) fn any_change(db: &Connection, kind: &str, key: &str) -> rusqlite::Result<bool> {
    if !has_revision(db, kind, key)? {
        return Ok(false);
    }
    Ok(db
        .query_row(
            "SELECT 1 FROM projection_outbox WHERE projection_kind=?1 AND projection_key=?2 \
             ORDER BY outbox_id DESC LIMIT 1",
            params![kind, key],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .is_some())
}

fn has_revision(db: &Connection, kind: &str, key: &str) -> rusqlite::Result<bool> {
    db.query_row(
        "SELECT EXISTS(SELECT 1 FROM projection_revisions WHERE projection_kind=?1 \
         AND projection_key=?2)",
        params![kind, key],
        |row| row.get(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PersistenceCoordinator, PersistenceOptions, PersistenceStartup};
    use aura_domain_common::RuntimeInstanceId;
    use std::sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    };

    /// Changes of earlier runtimes.
    const EARLIER: u64 = 20_000;
    const STEP: i32 = 100;

    fn instructions(db: &Connection, work: impl FnOnce(&Connection)) -> u64 {
        let calls = Arc::new(AtomicU64::new(0));
        let seen = Arc::clone(&calls);
        db.progress_handler(
            STEP,
            Some(move || {
                seen.fetch_add(1, Ordering::Relaxed);
                false
            }),
        )
        .expect("progress handler");
        work(db);
        db.progress_handler(STEP, None::<fn() -> bool>).expect("progress handler off");
        calls.load(Ordering::Relaxed) * STEP as u64
    }

    fn change(db: &Connection, key: &str, payload: &str) {
        let tx = db.unchecked_transaction().expect("transaction");
        let revision = crate::coordinator::next_projection_revision(&tx, "tool_definition", key)
            .expect("revision");
        let revision = i64::try_from(revision).expect("revision");
        tx.execute(
            "INSERT INTO projection_outbox(projection_kind, projection_key, revision, \
             payload_json, created_at) VALUES ('tool_definition', ?1, ?2, ?3, 'now')",
            params![key, revision, payload],
        )
        .expect("change");
        tx.commit().expect("commit");
    }

    #[test]
    fn a_lookup_reads_back_only_to_the_newest_change_whatever_earlier_runtimes_left() {
        let directory = tempfile::tempdir().expect("directory");
        let options = PersistenceOptions::new(
            directory.path().join("data/aura.db"),
            directory.path().join("data/artifact-objects"),
            RuntimeInstanceId::new(),
        );
        match PersistenceCoordinator::start(options).expect("start") {
            PersistenceStartup::Ready(coordinator) => drop(coordinator),
            PersistenceStartup::RecoveryRequired(recovery) => panic!("{}", recovery.reason),
        }
        let db = Connection::open(directory.path().join("data/aura.db")).expect("database");
        let (key, missing) =
            ("{\"runtime\":\"now\",\"tool\":1}", "{\"runtime\":\"now\",\"tool\":2}");
        change(&db, key, "\"first\"");
        let lookups = |db: &Connection| {
            instructions(db, |db| {
                latest_payload(db, "tool_definition", key).expect("latest");
                latest_payload(db, "tool_definition", missing).expect("missing");
                any_change(db, "tool_definition", key).expect("any");
                any_change(db, "tool_definition", missing).expect("none");
            })
        };
        let alone = lookups(&db);
        db.execute_batch(&format!(
            "WITH RECURSIVE n(x) AS (SELECT 1 UNION ALL SELECT x+1 FROM n WHERE x<{EARLIER}) \
             INSERT INTO projection_outbox(projection_kind, projection_key, revision, \
             payload_json, created_at) SELECT 'tool_definition', \
             '{{\"runtime\":\"earlier\",\"tool\":'||x||'}}', 1, hex(zeroblob(600)), 'then' FROM n"
        ))
        .expect("earlier changes");
        change(&db, key, "\"second\"");
        let beside = lookups(&db);
        assert!(
            beside <= alone + 1_000,
            "the lookups ran {alone} SQLite instructions alone and {beside} beside {EARLIER} \
             changes of earlier runtimes"
        );
        assert_eq!(
            latest_payload(&db, "tool_definition", key).expect("latest").as_deref(),
            Some("\"second\"")
        );
        assert_eq!(latest_payload(&db, "tool_definition", missing).expect("missing"), None);
        assert!(any_change(&db, "tool_definition", key).expect("any"));
        assert!(!any_change(&db, "tool_definition", missing).expect("none"));
    }
}
