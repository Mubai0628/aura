use aura_agent_collaboration::*;
use aura_domain_common::{AgentRunId, SessionId, TurnTaskId, UtcTimestamp, WorkspaceId};
use rusqlite::{Connection, params};

fn fixture() -> (Connection, SessionId, WorkspaceId) {
    let db = Connection::open_in_memory().expect("database");
    db.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE sessions(id TEXT PRIMARY KEY);")
        .expect("sessions");
    db.execute_batch(SCHEMA).expect("schema");
    let session = SessionId::new();
    db.execute("INSERT INTO sessions VALUES(?1)", params![session.to_string()]).expect("session");
    (db, session, WorkspaceId::new())
}
fn generation() -> AgentProfileGeneration {
    AgentProfileGeneration {
        source_run_id: AgentRunId::new(),
        source_turn_id: TurnTaskId::new(),
        model_profile_id: None,
        state: AgentProfileGenerationState::Running,
        input_tokens: 0,
        output_tokens: 0,
        usage_complete: false,
        message: None,
        preview: None,
    }
}
fn reserve(
    db: &mut Connection,
    session: SessionId,
    revision: u64,
    record: &AgentProfileGeneration,
) {
    let tx = db.transaction().expect("transaction");
    assert!(
        profile::reserve_generation(&tx, session, revision, record, UtcTimestamp::now())
            .expect("reserve")
    );
    tx.commit().expect("commit");
}

#[test]
fn session_agent_late_generation_keeps_user_description_but_counts_real_usage() {
    let (mut db, session, workspace) = fixture();
    let mut record = generation();
    reserve(&mut db, session, 0, &record);
    let tx = db.transaction().expect("transaction");
    profile::set_user(
        &tx,
        workspace,
        "旧标题",
        &AgentProfileSetRequest {
            session_id: session,
            expected_revision: 0,
            description: "用户保留的职责".into(),
        },
        UtcTimestamp::now(),
    )
    .expect("user edit");
    record.state = AgentProfileGenerationState::Completed;
    record.input_tokens = 50;
    record.output_tokens = 10;
    record.usage_complete = true;
    assert!(
        !profile::finish_generation(
            &tx,
            session,
            workspace,
            "旧标题",
            Some("迟到总结"),
            &record,
            UtcTimestamp::now()
        )
        .expect("finish")
    );
    tx.commit().expect("commit");
    let read = profile::load(&db, session, workspace, "新标题").expect("profile");
    assert_eq!(read.description, "用户保留的职责");
    assert_eq!(read.name, "新标题");
    assert_eq!(read.revision, 1);
    assert_eq!((read.input_tokens, read.output_tokens), (50, 10));
    assert_eq!(read.generation.expect("record").state, AgentProfileGenerationState::Superseded);
}

#[test]
fn session_agent_same_run_is_reserved_once_and_never_counted_twice() {
    let (mut db, session, workspace) = fixture();
    let mut record = generation();
    reserve(&mut db, session, 0, &record);
    let tx = db.transaction().expect("transaction");
    assert!(
        !profile::reserve_generation(&tx, session, 0, &record, UtcTimestamp::now())
            .expect("dedupe")
    );
    record.state = AgentProfileGenerationState::Completed;
    record.input_tokens = 7;
    record.usage_complete = true;
    assert!(
        profile::finish_generation(
            &tx,
            session,
            workspace,
            "标题",
            Some("职责"),
            &record,
            UtcTimestamp::now()
        )
        .expect("finish")
    );
    assert!(
        !profile::finish_generation(
            &tx,
            session,
            workspace,
            "标题",
            Some("不能重放"),
            &record,
            UtcTimestamp::now()
        )
        .expect("repeat")
    );
    tx.commit().expect("commit");
    let read = profile::load(&db, session, workspace, "标题").expect("profile");
    assert_eq!(read.description, "职责");
    assert_eq!(read.input_tokens, 7);
    assert_eq!(read.revision, 1);
}

#[test]
fn session_agent_unchanged_generation_does_not_invalidate_identity_prefix() {
    let (mut db, session, workspace) = fixture();
    for revision in [0, 1] {
        let mut record = generation();
        reserve(&mut db, session, revision, &record);
        record.state = AgentProfileGenerationState::Completed;
        record.usage_complete = true;
        let tx = db.transaction().expect("transaction");
        profile::finish_generation(
            &tx,
            session,
            workspace,
            "标题",
            Some("职责"),
            &record,
            UtcTimestamp::now(),
        )
        .expect("finish");
        tx.commit().expect("commit");
    }
    assert_eq!(profile::load(&db, session, workspace, "标题").expect("profile").revision, 1);
}

#[test]
fn session_agent_failed_or_interrupted_generation_keeps_previous_text_and_usage_unknown() {
    let (mut db, session, workspace) = fixture();
    let record = generation();
    reserve(&mut db, session, 0, &record);
    let tx = db.transaction().expect("transaction");
    profile::interrupt_generations(&tx, UtcTimestamp::now()).expect("recovery");
    tx.commit().expect("commit");
    let read = profile::load(&db, session, workspace, "标题").expect("profile");
    assert!(read.description.is_empty());
    assert!(!read.usage_complete);
    assert_eq!(read.generation.expect("record").state, AgentProfileGenerationState::Interrupted);
}

#[test]
fn session_agent_stale_user_edit_is_rejected_and_identity_fields_cannot_be_supplied() {
    let (mut db, session, workspace) = fixture();
    let request = AgentProfileSetRequest {
        session_id: session,
        expected_revision: 0,
        description: "职责".into(),
    };
    let tx = db.transaction().expect("transaction");
    profile::set_user(&tx, workspace, "标题", &request, UtcTimestamp::now()).expect("first");
    assert!(profile::set_user(&tx, workspace, "标题", &request, UtcTimestamp::now()).is_err());
    assert!(
        serde_json::from_value::<GeneratedAgentDescription>(
            serde_json::json!({"description":"职责", "session_id":session})
        )
        .is_err()
    );
}
