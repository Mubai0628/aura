use super::graph_fixture::Fixture;
use aura_agent_collaboration::{
    AgentGraphAction as Action, AgentGraphMutationRequest, AgentGraphPosition as Position,
};

#[test]
fn session_agent_graph_merge_uses_one_revision_for_members_and_explicit_world_geometry() {
    let mut fixture = Fixture::new();
    let [a, b, _, _] = fixture.roots;
    let [child_a, child_b] = fixture.children;
    let one = fixture
        .change(
            a,
            fixture.read(a).expected(),
            Action::Place {
                positions: vec![
                    Position { session_id: a, x: -120, y: 96 },
                    Position { session_id: child_a, x: 200, y: 100 },
                ],
            },
        )
        .expect("first layout");
    let two = fixture
        .change(
            b,
            fixture.read(b).expected(),
            Action::Place {
                positions: vec![
                    Position { session_id: b, x: -120, y: 96 },
                    Position { session_id: child_b, x: 200, y: 100 },
                ],
            },
        )
        .expect("overlapping independent layout");
    let input: AgentGraphMutationRequest = serde_json::from_value(serde_json::json!({"session_id":a,"expected_graph":one.expected(),"action":{"action":"merge","other_session_id":b,"expected_other":two.expected(),"confirmed":true,"positions":[{"session_id":a,"x":-120,"y":96},{"session_id":child_a,"x":200,"y":100},{"session_id":b,"x":800,"y":96},{"session_id":child_b,"x":1120,"y":100}]}})).expect("merge carries explicit world geometry");
    let merged = fixture.change(a, input.expected_graph, input.action).expect("atomic merge");
    assert_eq!(merged.revision, one.revision + 1);
    assert_eq!(merged.positions.len(), 4);
    assert!(merged.positions.contains(&Position { session_id: a, x: -120, y: 96 }));
    assert!(merged.positions.contains(&Position { session_id: b, x: 800, y: 96 }));
    assert!(merged.positions.contains(&Position { session_id: child_b, x: 1120, y: 100 }));
    assert_eq!(fixture.read(b), merged);
}
