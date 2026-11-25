use super::graph_fixture::Fixture;
use aura_agent_collaboration::{AgentGraphAction as Action, AgentGraphPosition, graph};
use rusqlite::{Connection, OpenFlags};

#[test]
fn session_agent_graph_reopening_the_database_keeps_current_members_edges_coordinates_and_revision()
{
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("current-graph.sqlite");
    let mut fixture = Fixture::using(Connection::open(&path).expect("file database"));
    let [a, b, _, _] = fixture.roots;
    let workspace = fixture.access.workspace_id;
    let joined = fixture.merge(a, b);
    let linked = fixture
        .change(
            a,
            joined.expected(),
            Action::Connect { source_session_id: a, target_session_id: b },
        )
        .expect("edge");
    let current = fixture
        .change(
            a,
            linked.expected(),
            Action::Place {
                positions: vec![
                    AgentGraphPosition { session_id: a, x: -300, y: 120 },
                    AgentGraphPosition { session_id: fixture.children[1], x: 620, y: -40 },
                ],
            },
        )
        .expect("layout");
    drop(fixture);
    let reader = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("reopened reader");
    assert_eq!(graph::load(&reader, a, workspace).expect("first view"), current);
    assert_eq!(graph::load(&reader, b, workspace).expect("second view"), current);
    let sessions: i64 = reader
        .query_row("SELECT count(*) FROM sessions", [], |row| row.get(0))
        .expect("original sessions");
    assert_eq!(sessions, 6);
}
