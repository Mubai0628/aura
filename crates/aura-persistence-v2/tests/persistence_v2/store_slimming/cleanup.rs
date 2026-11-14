//! What goes and what stays.
use std::ops::ControlFlow;

use aura_model_domain::{TransportAttemptRecord, TransportAttemptState};
use aura_persistence_core::{artifact, record, schema};
use aura_persistence_v2::{
    V2Store,
    slimming::{self, Rebuild},
};

use super::old_store::{self, SNAPSHOTS, count, reader};

fn room(_: &std::path::Path) -> std::io::Result<u64> {
    Ok(u64::MAX / 4)
}

fn attempt(db: &rusqlite::Connection, id: &str) -> (TransportAttemptRecord, i64) {
    let (payload, revision): (String, i64) = db
        .query_row("SELECT payload, revision FROM transport_attempts WHERE id=?1", [id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .unwrap();
    (record::decode(&payload).unwrap(), revision)
}

fn artifact_exists(db: &rusqlite::Connection, id: impl ToString) -> bool {
    count(db, &format!("SELECT count(*) FROM artifacts WHERE id='{}'", id.to_string())) == 1
}

#[test]
fn what_nothing_reads_goes_and_what_is_read_stays() {
    let old = old_store::build();
    let report = slimming::slim(&old.root, &room, &mut |_| ControlFlow::Continue(())).unwrap();
    let db = reader(&old.root);

    // The outbox copies go; every event stays.
    assert_eq!(count(&db, "SELECT count(*) FROM projection_outbox"), 0);
    assert_eq!(count(&db, "SELECT count(*) FROM events"), old.events);
    assert_eq!(report.outbox_rows as i64, old.events);

    // Each `model.transport` event now says only which call, which state and when.
    assert_eq!(
        count(&db, "SELECT count(*) FROM events WHERE kind='model.transport' AND version=1"),
        0
    );
    let events = db
        .prepare(
            "SELECT payload, created_at FROM events WHERE kind='model.transport' ORDER BY sequence",
        )
        .unwrap()
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let expected = old
        .completed
        .states
        .iter()
        .map(|state| (old.completed.id, *state))
        .chain(old.failed.states.iter().map(|state| (old.failed.id, *state)))
        .collect::<Vec<_>>();
    assert_eq!(events.len(), expected.len());
    assert_eq!(report.transport_events as usize, expected.len());
    for ((payload, at), (id, state)) in events.iter().zip(&expected) {
        let compact = serde_json::json!({ "attempt_id": id, "state": state, "at": at });
        assert_eq!(payload, &record::encode(&compact).unwrap());
    }

    // Each conversation keeps its newest snapshot and the texts it was built from; the texts the
    // older ones held go, and the instructions every step shared stay with the newest.
    for conversation in &old.conversations {
        let left = count(
            &db,
            &format!(
                "SELECT count(*) FROM context_snapshots WHERE session_id='{}'",
                conversation.id
            ),
        );
        assert_eq!(left, 1);
        let newest = conversation.snapshots.last().unwrap();
        assert_eq!(
            count(&db, &format!("SELECT count(*) FROM context_snapshots WHERE id='{newest}'")),
            1
        );
        for section in &conversation.kept_sections {
            assert!(!artifact::read(&db, *section, &conversation.newest_step).unwrap().is_empty());
        }
        for section in &conversation.cleared_sections {
            assert!(!artifact_exists(&db, section), "a cleared snapshot's text stayed");
        }
    }
    assert_eq!(report.snapshots as usize, (SNAPSHOTS - 1) * old.conversations.len());
    // The shared text and each newest snapshot's two own texts, and the failed call's stream.
    assert_eq!(count(&db, "SELECT count(*) FROM artifact_objects"), 1 + 2 * 2 + 1);

    // The completed call stops naming its raw stream, which goes; the failed one keeps its own.
    let (completed, revision) = attempt(&db, &old.completed.id.to_string());
    assert_eq!(completed.state, TransportAttemptState::Completed);
    assert_eq!(completed.raw_stream_artifact_id, None);
    assert_eq!(revision, 2);
    assert!(!artifact_exists(&db, old.completed.stream));
    let (failed, revision) = attempt(&db, &old.failed.id.to_string());
    assert_eq!(failed.raw_stream_artifact_id, Some(old.failed.stream));
    assert_eq!(revision, 1);
    assert!(!artifact::read(&db, old.failed.stream, &old.failed.step).unwrap().is_empty());
    assert_eq!(report.raw_streams, 1);

    // The file was rebuilt around what stayed.
    assert_eq!(report.rebuild, Rebuild::Done);
    assert!(!report.stopped);
    assert!(report.after_bytes * 3 < report.before_bytes, "{report:?}");
    assert_eq!(count(&db, "PRAGMA freelist_count"), 0);
    drop(db);

    // Nothing is owed, the normal exit before the run still counts, and the store is whole.
    assert_eq!(slimming::owed(&old.root), None);
    let store = V2Store::open(&old.root).unwrap();
    assert_ne!(store.pending_check(), schema::PendingCheck::Full);
    schema::integrity(store.database.connection()).unwrap();
}
