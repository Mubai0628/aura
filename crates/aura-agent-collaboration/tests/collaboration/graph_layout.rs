use super::graph_fixture::Fixture;
use aura_agent_collaboration::{AgentGraphAction as Action, AgentGraphPosition as Position};

#[test]
fn session_agent_graph_world_positions_follow_their_real_parent_when_removed() {
    let mut fixture = Fixture::new();
    let [a, b, c, _] = fixture.roots;
    let [child_a, child_b] = fixture.children;
    let joined = fixture.merge(a, b);
    let wanted = vec![
        Position { session_id: a, x: -120, y: 96 },
        Position { session_id: child_a, x: 420, y: 220 },
        Position { session_id: child_b, x: 680, y: -10 },
    ];
    let placed = fixture
        .change(a, joined.expected(), Action::Place { positions: wanted.clone() })
        .expect("world positions");
    assert_eq!(fixture.read(b), placed);
    for positions in [
        vec![Position { session_id: c, x: 0, y: 0 }],
        vec![wanted[0], wanted[0]],
        vec![Position { session_id: a, x: i32::MIN, y: 0 }],
    ] {
        assert!(fixture.change(a, placed.expected(), Action::Place { positions }).is_err());
        assert_eq!(fixture.read(a), placed);
    }
    let independent = fixture
        .change(a, placed.expected(), Action::Remove { session_id: a })
        .expect("remove self");
    assert_eq!(independent.sessions, [a]);
    assert_eq!(independent.positions.len(), 2);
    assert!(
        independent.positions.contains(&wanted[0]) && independent.positions.contains(&wanted[1])
    );
    let other = fixture.read(b);
    assert_eq!(other.sessions, [b]);
    assert_eq!(other.positions, [wanted[2]]);
    assert_ne!(other.graph_id, independent.graph_id);
    let empty =
        fixture.change(a, independent.expected(), Action::ResetLayout).expect("explicit reset");
    assert!(empty.positions.is_empty());
    assert_eq!(fixture.read(b), other, "reset is confined to its current graph");
}

#[test]
fn session_agent_graph_merge_failure_rolls_back_every_member_layout_and_snapshot() {
    let mut fixture = Fixture::new();
    let [a, b, c, d] = fixture.roots;
    let one = fixture.merge(a, b);
    let two = fixture.merge(c, d);
    let history = fixture.count("agent_collaboration_graph_history");
    fixture.db.execute_batch(&format!("CREATE TRIGGER fail_member_move BEFORE UPDATE OF graph_id ON agent_collaboration_graph_members WHEN OLD.session_id='{d}' BEGIN SELECT RAISE(ABORT,'test member write failed'); END;")).expect("fault injection");
    assert!(
        fixture
            .change(
                a,
                one.expected(),
                Action::Merge {
                    other_session_id: c,
                    expected_other: two.expected(),
                    confirmed: true,
                    positions: vec![]
                }
            )
            .is_err()
    );
    assert_eq!(fixture.read(a), one);
    assert_eq!(fixture.read(b), one);
    assert_eq!(fixture.read(c), two);
    assert_eq!(fixture.read(d), two);
    assert_eq!(fixture.count("agent_collaboration_graph_members"), 4);
    assert_eq!(fixture.count("agent_collaboration_graph_history"), history);
}
