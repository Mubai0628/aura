//! A user's terminal handed to the model is stored in the record shape every earlier Aura reads:
//! an earlier Core reads every terminal record when it starts and refuses a view field it does not
//! know, which left its whole command backend closed. The handover is kept as the record's owner.
use crate::support;
use aura_domain_common::*;
use aura_persistence_v2::{V2Store, terminal::*};
use aura_windows_user_contract::*;
use serde::Deserialize;

/// The terminal record as an earlier Core decodes it. Only decoding is checked, so most fields are
/// never read.
#[derive(Deserialize)]
#[allow(dead_code)]
struct EarlierRecord {
    view: EarlierView,
    operation_id: SecurityOperationId,
    runtime_instance_id: RuntimeInstanceId,
    working_directory: String,
    requires_network: bool,
    #[serde(default)]
    tab_closed: bool,
}

/// The terminal view before the handover existed: it refuses any field it does not know.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)]
struct EarlierView {
    session_id: SessionId,
    terminal_id: String,
    shell: ManagedShell,
    process_id: u32,
    started_at: UtcTimestamp,
    end: Option<TerminalEnd>,
    owner: TerminalOwnerV1,
    controller: TerminalControllerV1,
    title: String,
    tool_call_id: Option<ToolCallId>,
    revision: Revision,
}

fn record(
    session: SessionId,
    id: &str,
    owner: TerminalOwnerV1,
    handed_over: bool,
) -> AgentTerminalRecord {
    AgentTerminalRecord {
        view: TerminalViewV1 {
            session_id: session,
            terminal_id: id.into(),
            shell: ManagedShell::Cmd,
            process_id: 1,
            started_at: UtcTimestamp::now(),
            end: None,
            owner,
            controller: TerminalControllerV1::Agent,
            title: String::new(),
            tool_call_id: None,
            revision: Revision::INITIAL,
            handed_over,
        },
        operation_id: SecurityOperationId::new(),
        runtime_instance_id: RuntimeInstanceId::new(),
        working_directory: String::new(),
        requires_network: false,
        tab_closed: false,
    }
}

/// A store with one conversation, and these terminal records of it saved.
fn saved(
    records: impl FnOnce(SessionId) -> Vec<AgentTerminalRecord>,
) -> (tempfile::TempDir, V2Store, SessionId) {
    let root = tempfile::tempdir().expect("temporary root");
    let mut store = V2Store::create(&root.path().join("v2")).expect("real schema");
    let fixture = support::Fixture::new(&mut store, aura_agent_domain::AgentTaskMode::Normal);
    let session = fixture.turn.session_id;
    let records = records(session);
    store
        .database
        .unit_of_work(|tx| records.iter().try_for_each(|record| save(tx, record)))
        .expect("records");
    (root, store, session)
}

/// The stored record, decoded as an earlier Core decodes it.
fn as_earlier_aura_reads(db: &rusqlite::Connection, id: &str) -> EarlierRecord {
    let payload: String = db
        .query_row("SELECT payload FROM agent_terminals WHERE id=?1", [id], |row| row.get(0))
        .expect("stored record");
    serde_json::from_str(&payload).unwrap_or_else(|error| panic!("{error}: {payload}"))
}

#[test]
fn a_handed_over_terminal_is_stored_as_earlier_aura_reads_it_and_read_back_as_handed_over() {
    let (_root, store, session) = saved(|session| {
        vec![
            record(session, "term_handed", TerminalOwnerV1::User, true),
            record(session, "term_own", TerminalOwnerV1::Agent, false),
        ]
    });
    let db = store.database.connection();
    assert_eq!(as_earlier_aura_reads(db, "term_handed").view.owner, TerminalOwnerV1::User);
    assert_eq!(as_earlier_aura_reads(db, "term_own").view.owner, TerminalOwnerV1::Agent);
    assert!(get(db, session, "term_handed").expect("read").view.handed_over);
    assert!(!get(db, session, "term_own").expect("read").view.handed_over);
    let listed = list(db, Some(session)).expect("list");
    let views = listed
        .iter()
        .map(|record| (record.view.terminal_id.as_str(), record.view.handed_over))
        .collect::<Vec<_>>();
    assert_eq!(views, [("term_handed", true), ("term_own", false)]);
}

#[test]
fn a_terminal_the_model_opened_is_never_read_back_as_handed_over() {
    let (_root, store, session) =
        saved(|session| vec![record(session, "term_model", TerminalOwnerV1::Agent, true)]);
    let db = store.database.connection();
    assert_eq!(as_earlier_aura_reads(db, "term_model").view.owner, TerminalOwnerV1::Agent);
    assert!(!get(db, session, "term_model").expect("read").view.handed_over);
    assert!(list(db, Some(session)).expect("list").iter().all(|one| !one.view.handed_over));
}
