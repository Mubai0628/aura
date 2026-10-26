//! Archiving keeps a conversation; deleting it removes everything Aura holds for it — its
//! messages, its process, its audit events and the artifacts only it held — in both stores, and
//! nothing of another conversation.
use super::*;
use std::collections::BTreeMap;

const MARKER_A: &str = "AURA-L107-ALPHA-MARKER-7Q2";
const MARKER_B: &str = "AURA-L107-BRAVO-MARKER-3K9";

struct Two {
    host: crate::r03_tests::Fixture,
    runtime: crate::RuntimeState,
    _providers: (Provider, Provider),
    a: SessionId,
    b: SessionId,
}

/// Each Session talks to its own Provider: a fixture Provider answers one scripted run.
async fn two_sessions_with_a_finished_write_each() -> Two {
    let host = crate::r03_tests::Fixture::new();
    let (provider_a, provider_b) = (Provider::new(true).await, Provider::new(true).await);
    let (runtime, a, model_a) =
        waiting::prepare_in_mode(&host, &provider_a, PermissionMode::WorkspaceReadWrite).await;
    let workspace = projection(&runtime)
        .sessions
        .iter()
        .find(|s| s.session_id == a)
        .expect("L10.7 trace fixture")
        .workspace_id;
    let b = create_session(&runtime, workspace, PermissionMode::WorkspaceReadWrite).await;
    let model_b = profile_after_existing(&runtime, &provider_b.url).await;
    run(&runtime, a, model_a, MARKER_A).await;
    run(&runtime, b, model_b, MARKER_B).await;
    Two { host, runtime, _providers: (provider_a, provider_b), a, b }
}

async fn run(runtime: &crate::RuntimeState, session: SessionId, model: ModelProfileId, text: &str) {
    let started = crate::r03_tests::success(
        runtime,
        "agent.start",
        json!(AgentStartRequest {
            session_id: session,
            model_profile_id: model,
            text: format!("{text}: write the result file and finish"),
            mode: AgentTaskMode::Normal,
            goal_budget: None,
            explicit_backend_id: None,
            skill_ids: Vec::new(),
            reasoning_effort: None,
            attachments: Vec::new(),
        }),
    )
    .await;
    let start: AgentRunProjection = serde_json::from_value(started).expect("run projection");
    let request = AgentRunRequest { session_id: session, agent_run_id: start.run.agent_run_id };
    waiting::wait(runtime, &request, |v| v.turn.state.terminal()).await;
}

fn services(runtime: &crate::RuntimeState) -> &crate::RuntimeServices {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    services
}

fn projection(runtime: &crate::RuntimeState) -> aura_workspace_domain::WorkspaceSessionProjection {
    services(runtime).persistence.workspace_session_projection().expect("projection")
}

/// `table.column` → rows whose value contains the needle, over every table of the store
/// (virtual search tables through themselves, not their shadow tables).
fn traces(db: &rusqlite::Connection, needle: &str) -> BTreeMap<String, i64> {
    let tables: Vec<String> = db
        .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")
        .expect("L10.7 trace fixture")
        .query_map([], |r| r.get(0))
        .expect("L10.7 trace fixture")
        .collect::<Result<_, _>>()
        .expect("L10.7 trace fixture");
    let shadow = |name: &str| {
        ["_data", "_idx", "_docsize", "_config", "_content"].iter().any(|s| name.ends_with(s))
            && tables.iter().any(|t| name.starts_with(t.as_str()) && t.ends_with("_fts"))
    };
    let mut found = BTreeMap::new();
    for table in tables.iter().filter(|t| !shadow(t)) {
        let columns: Vec<String> = db
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .expect("L10.7 trace fixture")
            .query_map([], |r| r.get(0))
            .expect("L10.7 trace fixture")
            .collect::<Result<_, _>>()
            .expect("L10.7 trace fixture");
        for column in columns {
            let count: i64 = db
                .query_row(
                    &format!(
                        "SELECT count(*) FROM \"{table}\"
                         WHERE instr(CAST(\"{column}\" AS BLOB), CAST(?1 AS BLOB)) > 0"
                    ),
                    [needle],
                    |r| r.get(0),
                )
                .expect("L10.7 trace fixture");
            if count > 0 {
                found.insert(format!("{table}.{column}"), count);
            }
        }
    }
    found
}

fn v2<T>(runtime: &crate::RuntimeState, read: impl FnOnce(&rusqlite::Connection) -> T) -> T {
    let store = services(runtime).broker.lock().expect("V2 store");
    read(store.database.connection())
}

fn p03(host: &crate::r03_tests::Fixture) -> rusqlite::Connection {
    rusqlite::Connection::open_with_flags(
        &host.paths.database_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .expect("P03 reader")
}

fn guards(db: &rusqlite::Connection) -> Vec<(String, String)> {
    db.prepare("SELECT name, sql FROM sqlite_master WHERE type='trigger' ORDER BY name")
        .expect("L10.7 trace fixture")
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("L10.7 trace fixture")
        .collect::<Result<_, _>>()
        .expect("L10.7 trace fixture")
}

fn turns(runtime: &crate::RuntimeState, session: SessionId) -> Vec<String> {
    v2(runtime, |db| {
        db.prepare("SELECT id FROM turn_tasks WHERE session_id=?1")
            .expect("L10.7 trace fixture")
            .query_map([session.to_string()], |r| r.get(0))
            .expect("L10.7 trace fixture")
            .collect::<Result<_, _>>()
            .expect("L10.7 trace fixture")
    })
}

/// Global records that may name a deleted Session without holding any of its conversation: the
/// model configuration journal keeps the projections it fanned out.
const P03_GLOBAL: [&str; 1] = ["mutation_journals.payload_json"];

#[tokio::test]
async fn archiving_keeps_a_session_and_deleting_it_leaves_no_trace_and_keeps_the_other() {
    let two = two_sessions_with_a_finished_write_each().await;
    let (runtime, a, b) = (&two.runtime, two.a, two.b);
    let a_turns = turns(runtime, a);
    assert!(!a_turns.is_empty(), "A ran a turn");
    assert!(!v2(runtime, |db| traces(db, MARKER_A)).is_empty(), "A's message is stored");
    let b_before = v2(runtime, |db| traces(db, &b.to_string()));
    let b_marker_before = v2(runtime, |db| traces(db, MARKER_B));
    let guards_before = v2(runtime, guards);

    // Archive and restore keep everything; the projection carries the state.
    let archived =
        crate::r03_tests::success(runtime, "session.archive", json!({"session_id": a})).await;
    assert_eq!(archived["state"], "archived");
    assert_eq!(
        projection(runtime)
            .sessions
            .iter()
            .find(|s| s.session_id == a)
            .expect("L10.7 trace fixture")
            .state,
        "archived"
    );
    let restored =
        crate::r03_tests::success(runtime, "session.restore", json!({"session_id": a})).await;
    assert_eq!(restored["state"], "idle");
    assert!(!v2(runtime, |db| traces(db, MARKER_A)).is_empty(), "archiving deleted nothing");

    // A conversation is deleted only once it is archived.
    let refused =
        crate::r03_tests::request(runtime, "session.delete", json!({"session_id": a})).await;
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .starts_with("session.not_archived: "),
        "{refused}"
    );
    assert!(!v2(runtime, |db| traces(db, MARKER_A)).is_empty(), "a refused delete removed nothing");
    crate::r03_tests::success(runtime, "session.archive", json!({"session_id": a})).await;

    // Delete: the route answers once A is hidden; its records go on Core's own task.
    let deleted =
        crate::r03_tests::success(runtime, "session.delete", json!({"session_id": a})).await;
    assert_eq!(deleted["session_id"], json!(a));
    assert!(projection(runtime).sessions.iter().all(|s| s.session_id != a));
    assert!(projection(runtime).sessions.iter().any(|s| s.session_id == b && s.state == "idle"));
    crate::session_lifecycle::deleted(runtime).await;

    // Nothing of A remains in the execution store: not its ID, not its turns, not its message.
    for needle in [a.to_string(), MARKER_A.to_owned()].into_iter().chain(a_turns.clone()) {
        assert_eq!(v2(runtime, |db| traces(db, &needle)), BTreeMap::new(), "V2 traces of {needle}");
    }
    // Nor in the primary registry, beyond global records that hold no conversation.
    let registry = p03(&two.host);
    let mut left = traces(&registry, &a.to_string());
    left.retain(|place, _| !P03_GLOBAL.contains(&place.as_str()));
    assert_eq!(left, BTreeMap::new(), "P03 traces of A");
    assert_eq!(traces(&registry, MARKER_A), BTreeMap::new(), "P03 traces of A's message");

    // B is exactly as it was, and the delete guards are back as they were.
    assert_eq!(v2(runtime, |db| traces(db, &b.to_string())), b_before, "B's rows");
    assert_eq!(v2(runtime, |db| traces(db, MARKER_B)), b_marker_before, "B's message");
    assert_eq!(v2(runtime, guards), guards_before, "delete guards restored verbatim");
    let path = services(runtime).broker.database_path().expect("V2 path");
    aura_persistence_core::schema::verify(
        &aura_persistence_core::open_reader(&path).expect("V2 reader"),
        aura_persistence_v2::MODULES,
    )
    .expect("the store still opens: same schema, foreign keys intact");
    let refused = v2(runtime, |db| {
        db.execute("DELETE FROM events WHERE sequence=(SELECT max(sequence) FROM events)", [])
    });
    assert!(
        refused.is_err_and(|e| e.to_string().contains("immutable audit event")),
        "events stay protected outside a purge"
    );

    // Deleting again finds nothing; the Workspace's files are untouched.
    let again =
        crate::r03_tests::request(runtime, "session.delete", json!({"session_id": a})).await;
    assert!(
        again["error"]["message"].as_str().unwrap_or_default().contains("session.not_found"),
        "{again}"
    );
    let root = two.host.root.path().join("waiting-workspace");
    assert!(
        std::fs::read_dir(&root).expect("L10.7 trace fixture").count() > 0,
        "the Workspace's files stay"
    );
}

#[tokio::test]
async fn a_deletion_cut_short_finishes_when_core_resumes() {
    let two = two_sessions_with_a_finished_write_each().await;
    let (runtime, a, b) = (&two.runtime, two.a, two.b);
    // The deletion was recorded, and Core stopped before removing anything.
    let marked = services(runtime).persistence.mark_session_deleting(a).expect("mark");
    assert_eq!(marked.state, "deleting");
    assert!(projection(runtime).sessions.iter().all(|s| s.session_id != a), "hidden at once");
    assert!(!v2(runtime, |db| traces(db, MARKER_A)).is_empty());

    crate::session_lifecycle::Purger::of(services(runtime)).resume().await.expect("resume");
    assert_eq!(v2(runtime, |db| traces(db, &a.to_string())), BTreeMap::new());
    assert_eq!(v2(runtime, |db| traces(db, MARKER_A)), BTreeMap::new());
    assert!(services(runtime).persistence.deleting_sessions().expect("read").is_empty());
    assert!(!v2(runtime, |db| traces(db, MARKER_B)).is_empty(), "B kept");
    let _ = b;
}

#[tokio::test]
async fn a_session_with_a_task_still_running_is_neither_archived_nor_deleted() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::new(true).await;
    // ReadOnly asks before the write, so the task waits for an answer.
    let (runtime, session, model) = waiting::prepare(&host, &provider).await;
    let started = waiting::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request = AgentRunRequest { session_id: session, agent_run_id: started.run.agent_run_id };
    waiting::wait(&runtime, &request, |v| matches!(v.turn.state, TurnTaskState::AwaitingApproval))
        .await;
    for method in ["session.archive", "session.delete"] {
        let refused =
            crate::r03_tests::request(&runtime, method, json!({"session_id": session})).await;
        let message = refused["error"]["message"].as_str().unwrap_or_default();
        assert!(message.starts_with("session.busy: "), "{method}: {refused}");
    }
    assert!(
        projection(&runtime).sessions.iter().any(|s| s.session_id == session && s.state == "idle")
    );
}
