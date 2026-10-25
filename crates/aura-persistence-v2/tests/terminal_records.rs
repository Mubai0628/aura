//! Retention, UTF-8 byte paging and deletion use the real artifact store.
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use aura_domain_common::*;
use aura_persistence_v2::{V2Store, terminal::*};
use aura_windows_user_contract::*;

#[test]
fn bounded_segments_page_on_utf8_boundaries_and_delete_with_the_session() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut store = V2Store::create(&root.path().join("v2")).expect("real schema");
    let fixture = support::Fixture::new(&mut store, aura_agent_domain::AgentTaskMode::Normal);
    let record = AgentTerminalRecord {
        view: TerminalViewV1 {
            session_id: fixture.turn.session_id,
            terminal_id: "term_test".into(),
            shell: ManagedShell::Cmd,
            process_id: 1,
            started_at: UtcTimestamp::now(),
            end: None,
            owner: TerminalOwnerV1::Agent,
            controller: TerminalControllerV1::Agent,
            title: "fixture".into(),
            tool_call_id: None,
            revision: Revision::INITIAL,
        },
        operation_id: SecurityOperationId::new(),
        runtime_instance_id: RuntimeInstanceId::new(),
        working_directory: "C:/temporary".into(),
        requires_network: false,
        tab_closed: false,
    };
    store
        .database
        .unit_of_work(|tx| {
            save(tx, &record)?;
            append(tx, record.view.session_id, &record.view.terminal_id, &"早".repeat(6_000_000))?;
            append(tx, record.view.session_id, &record.view.terminal_id, "末尾中文\n")
        })
        .expect("persist bounded output segments");
    let db = store.database.connection();
    let first = read(db, &record, 0, 8192).expect("first retained page");
    assert!(first.truncated && first.start_offset > 0);
    assert_eq!(first.next_offset - first.start_offset, first.output.len() as u64);
    let middle =
        read(db, &record, first.start_offset + 1, 8192).expect("offset inside a UTF-8 character");
    assert!(middle.start_offset > first.start_offset + 1);
    assert_eq!(middle.next_offset - middle.start_offset, middle.output.len() as u64);
    let retained:i64=db.query_row("SELECT sum(next_offset-start_offset) FROM terminal_output_segments WHERE terminal_id='term_test'",[],|row|row.get(0)).expect("retained byte count");
    assert!(retained <= OUTPUT_LIMIT as i64);
    let other = SessionId::new();
    assert!(get(db, other, "term_test").is_err());
    assert!(read(db, &record, 0, 0).is_err());
    let end = read(db, &record, u64::MAX, 16).expect("end cursor");
    let tail = read(db, &record, end.next_offset - 13, 32).expect("tail");
    assert!(tail.output.contains("末尾中文"));
    store
        .database
        .unit_of_work(|tx| delete_session(tx, record.view.session_id))
        .expect("delete only terminal records and their artifacts");
    assert!(get(store.database.connection(), record.view.session_id, "term_test").is_err());
    let outputs: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT count(*) FROM artifacts WHERE media_type='text/plain; charset=utf-8'",
            [],
            |row| row.get(0),
        )
        .expect("artifact GC");
    assert_eq!(outputs, 0);
}

#[test]
fn l03_b0_small_output_appends_merge_segments_and_collect_replaced_artifacts() {
    let root = tempfile::tempdir().expect("temporary root");
    let mut store = V2Store::create(&root.path().join("v2")).expect("real schema");
    let fixture = support::Fixture::new(&mut store, aura_agent_domain::AgentTaskMode::Normal);
    let record = AgentTerminalRecord {
        view: TerminalViewV1 {
            session_id: fixture.turn.session_id,
            terminal_id: "term_merged".into(),
            shell: ManagedShell::Cmd,
            process_id: 1,
            started_at: UtcTimestamp::now(),
            end: None,
            owner: TerminalOwnerV1::Agent,
            controller: TerminalControllerV1::Agent,
            title: "fixture".into(),
            tool_call_id: None,
            revision: Revision::INITIAL,
        },
        operation_id: SecurityOperationId::new(),
        runtime_instance_id: RuntimeInstanceId::new(),
        working_directory: "C:/temporary".into(),
        requires_network: false,
        tab_closed: false,
    };
    store.database.unit_of_work(|tx| save(tx, &record)).expect("record");
    for _ in 0..100 {
        store
            .database
            .unit_of_work(|tx| {
                append(tx, record.view.session_id, &record.view.terminal_id, "中文\n")
            })
            .expect("independent output capture");
    }
    let db = store.database.connection();
    let segments: i64 = db
        .query_row(
            "SELECT count(*) FROM terminal_output_segments WHERE terminal_id='term_merged'",
            [],
            |row| row.get(0),
        )
        .expect("segment count");
    assert_eq!(segments, 1, "small captures share the bounded tail segment");
    let artifacts: i64 = db
        .query_row(
            "SELECT count(*) FROM artifacts WHERE media_type='text/plain; charset=utf-8'",
            [],
            |row| row.get(0),
        )
        .expect("artifact count");
    assert_eq!(artifacts, 1, "obsolete immutable tail artifacts are collected");
    let output = read(db, &record, 0, 65536).expect("merged output");
    assert_eq!(output.output, "中文\n".repeat(100));
    assert_eq!(output.next_offset, 700);
    let middle = read(db, &record, 1, 7).expect("UTF-8 cursor remains exact after merge");
    assert_eq!(middle.start_offset, 3);
    assert_eq!(middle.output, "文\n中");
    store
        .database
        .unit_of_work(|tx| {
            append(tx, record.view.session_id, &record.view.terminal_id, &"尾".repeat(22000))
        })
        .expect("tail crosses segment boundary");
    let count: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT count(*) FROM terminal_output_segments WHERE terminal_id='term_merged'",
            [],
            |row| row.get(0),
        )
        .expect("bounded segments");
    assert_eq!(count, 3);
    let end = read(store.database.connection(), &record, u64::MAX, 1).expect("end cursor");
    assert_eq!(end.next_offset, 66700);
}

#[test]
fn opening_the_frozen_tool_schema_predecessor_adds_terminal_and_package_tables() {
    use aura_persistence_core::{DatabaseConnection, schema};
    let root = tempfile::tempdir().expect("temporary root");
    let directory = root.path().join("v2");
    std::fs::create_dir(&directory).expect("directory");
    let sql = include_str!("../src/tool.sql");
    let predecessor = format!(
        "{}\n",
        sql.split_once("\nCREATE TABLE agent_terminals").expect("frozen predecessor").0
    );
    let modules = aura_persistence_v2::MODULES
        .iter()
        .copied()
        .map(|module| {
            if module.owner == "tool" {
                schema::SchemaModule {
                    owner: "tool",
                    sql: Box::leak(predecessor.clone().into_boxed_str()),
                }
            } else {
                module
            }
        })
        .collect::<Vec<_>>();
    let mut database = DatabaseConnection::create_new(&directory.join("aura-v2.db"))
        .expect("predecessor database");
    schema::initialize(&mut database, &modules).expect("exact predecessor schema");
    database
        .connection()
        .execute(
            "INSERT INTO events(event_id,owner,kind,version,payload,created_at) VALUES('evt_keep','owner','note',1,'{}','2026-10-14T00:00:00Z')",
            [],
        )
        .expect("unrelated event");
    drop(database);
    std::fs::write(
        directory.join("v2-development.json"),
        b"{\"schema_version\":1,\"namespace\":\"aura-v2-development\"}\n",
    )
    .expect("marker");
    let store = V2Store::open(&directory).expect("reviewed append-only migration");
    let preserved: i64 = store
        .database
        .connection()
        .query_row("SELECT count(*) FROM events WHERE event_id='evt_keep'", [], |row| row.get(0))
        .expect("preserved event");
    assert_eq!(preserved, 1);
    let tables:i64=store.database.connection().query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('agent_terminals','terminal_output_segments')",[],|row|row.get(0)).expect("new tables");
    assert_eq!(tables, 2);
    let packages:i64=store.database.connection().query_row("SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('tool_packages','tool_package_versions','tool_package_payloads','tool_package_files','tool_package_references','tool_package_trust','tool_package_deletions')",[],|row|row.get(0)).expect("reviewed package tables");
    assert_eq!(packages, 7);
}
