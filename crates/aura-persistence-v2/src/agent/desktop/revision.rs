//! The history poll's revision. It moves whenever the session's page may have changed: every
//! visible event advances the first part, and a turn's or run's state change that writes no event
//! advances the others. Each part only grows, so the revision never goes back. The turns and runs
//! summed are this session's, read through `turn_tasks_session` and `agent_runs_turn`, so a poll
//! costs what the session holds rather than what the whole store holds.
use aura_domain_common::{Revision, SessionId};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use rusqlite::Connection;

const REVISION: &str = concat!(
    "SELECT 1 + COALESCE((SELECT MAX(sequence) FROM events),0) ",
    "+ COALESCE((SELECT SUM(revision) FROM turn_tasks WHERE session_id=?1),0) ",
    "+ COALESCE((SELECT SUM(r.revision) FROM turn_tasks t JOIN agent_runs r ",
    "ON r.turn_task_id=t.id WHERE t.session_id=?1),0)"
);

pub(super) fn revision(db: &Connection, session: SessionId) -> StoreResult<Revision> {
    let value: i64 = db.query_row(REVISION, [session.to_string()], |r| r.get(0))?;
    Ok(Revision::new(u64::try_from(value).map_err(|_| Error::Conflict)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_revision_reads_the_sessions_turns_and_runs_through_their_indexes() {
        let root = tempfile::tempdir().expect("directory");
        let store = crate::V2Store::create(&root.path().join("v2")).expect("store");
        let db = store.database.connection();
        let mut plan = db.prepare(&format!("EXPLAIN QUERY PLAN {REVISION}")).expect("plan");
        let details = plan
            .query_map([SessionId::new().to_string()], |r| r.get::<_, String>(3))
            .expect("plan rows")
            .collect::<Result<Vec<_>, _>>()
            .expect("plan details");
        let text = details.join("\n");
        assert!(text.contains("turn_tasks_session") && text.contains("agent_runs_turn"), "{text}");
        assert!(
            !details.iter().any(|line| line.starts_with("SCAN")
                && (line.contains("turn_tasks")
                    || line.contains("agent_runs")
                    || line == "SCAN t"
                    || line == "SCAN r")),
            "{text}"
        );
    }
}
