use super::graph_fixture::Fixture;
use aura_agent_collaboration::AgentGraphAction as Action;

#[test]
fn session_agent_graph_allows_cycles_but_refuses_self_duplicate_and_cross_graph_edges() {
    let mut fixture = Fixture::new();
    let [a, b, c, _] = fixture.roots;
    let joined = fixture.merge(a, b);
    let edge = Action::Connect { source_session_id: a, target_session_id: b };
    let forward = fixture.change(a, joined.expected(), edge.clone()).expect("forward");
    let before = fixture.count("agent_collaboration_graph_history");
    for invalid in [
        edge,
        Action::Connect { source_session_id: a, target_session_id: a },
        Action::Connect { source_session_id: a, target_session_id: c },
    ] {
        assert!(fixture.change(a, forward.expected(), invalid).is_err());
        assert_eq!(fixture.read(a), forward);
    }
    assert_eq!(fixture.count("agent_collaboration_graph_history"), before);
    let cycle = fixture
        .change(
            b,
            forward.expected(),
            Action::Connect { source_session_id: b, target_session_id: a },
        )
        .expect("a valid return path");
    assert_eq!(cycle.edges.len(), 2);
    let cut = fixture
        .change(
            a,
            cycle.expected(),
            Action::Disconnect { source_session_id: a, target_session_id: b },
        )
        .expect("cut edge");
    assert_eq!(cut.sessions.len(), 2);
    assert_eq!(cut.edges.len(), 1);
    assert_eq!(fixture.read(b), cut);
    let again = fixture
        .change(
            a,
            cut.expected(),
            Action::Disconnect { source_session_id: a, target_session_id: b },
        )
        .expect("already disconnected");
    assert_eq!(again, cut);
}

#[test]
fn session_agent_graph_stale_edit_never_rewrites_newer_topology() {
    let mut fixture = Fixture::new();
    let [a, b, _, _] = fixture.roots;
    let joined = fixture.merge(a, b);
    let latest = fixture
        .change(
            a,
            joined.expected(),
            Action::Connect { source_session_id: a, target_session_id: b },
        )
        .expect("new edge");
    assert!(fixture.change(b, joined.expected(), Action::Remove { session_id: a }).is_err());
    assert_eq!(fixture.read(a), latest);
    assert_eq!(fixture.read(b), latest);
}
