use aura_agent_collaboration::{AgentSessionPolicySetRequest, SCHEMA, policy};
use aura_domain_common::{SessionId, UtcTimestamp};
use rusqlite::{Connection, params};

#[test]
fn session_agent_policy_is_off_by_default_and_stale_writes_cannot_change_it() {
    let mut db = Connection::open_in_memory().expect("database");
    db.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE sessions(id TEXT PRIMARY KEY);")
        .expect("sessions");
    db.execute_batch(SCHEMA).expect("schema");
    let session = SessionId::new();
    let other = SessionId::new();
    for id in [session, other] {
        db.execute("INSERT INTO sessions VALUES(?1)", params![id.to_string()]).expect("session");
    }
    let initial = policy::load(&db, session).expect("old session");
    assert!(!initial.allow_proactive_temporary_agents && initial.revision == 0);
    let tx = db.transaction().expect("transaction");
    let input = AgentSessionPolicySetRequest {
        session_id: session,
        expected_revision: 0,
        allow_proactive_temporary_agents: true,
    };
    let changed = policy::set(&tx, &input, UtcTimestamp::now()).expect("save");
    assert_eq!(changed.revision, 1);
    assert!(
        policy::set(
            &tx,
            &AgentSessionPolicySetRequest {
                allow_proactive_temporary_agents: false,
                ..input.clone()
            },
            UtcTimestamp::now()
        )
        .is_err()
    );
    let same = policy::set(
        &tx,
        &AgentSessionPolicySetRequest { expected_revision: 1, ..input },
        UtcTimestamp::now(),
    )
    .expect("no change");
    assert_eq!(same, changed);
    tx.commit().expect("commit");
    assert!(policy::load(&db, session).expect("reload").allow_proactive_temporary_agents);
    assert_eq!(policy::load(&db, other).expect("isolated").revision, 0);
}
