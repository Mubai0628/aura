use super::graph_fixture::Fixture;
use aura_agent_collaboration::{AgentGraphAction as Action, AgentGraphRevision};

#[test]
fn session_agent_graph_reads_do_not_create_conversations_or_graphs() {
    let fixture = Fixture::new();
    for root in fixture.roots {
        let graph = fixture.read(root);
        assert_eq!(graph.sessions, [root]);
        assert_eq!(graph.expected(), AgentGraphRevision::default());
    }
    assert_eq!(fixture.count("agent_collaboration_graphs"), 0);
    assert_eq!(
        fixture
            .db
            .query_row("SELECT count(*) FROM sessions", [], |row| row.get::<_, i64>(0))
            .expect("sessions"),
        6
    );
}

#[test]
fn session_agent_graph_merges_whole_groups_only_after_confirmation_and_both_revisions_match() {
    let mut fixture = Fixture::new();
    let [a, b, c, d] = fixture.roots;
    let one = fixture.merge(a, b);
    let two = fixture.merge(c, d);
    let action = Action::Merge {
        other_session_id: c,
        expected_other: two.expected(),
        confirmed: false,
        positions: vec![],
    };
    assert!(fixture.change(a, one.expected(), action).is_err());
    assert_eq!(fixture.read(a), one);
    assert_eq!(fixture.read(c), two);
    assert!(
        fixture
            .change(
                a,
                one.expected(),
                Action::Merge {
                    other_session_id: c,
                    expected_other: AgentGraphRevision::default(),
                    confirmed: true,
                    positions: vec![]
                }
            )
            .is_err()
    );
    assert_eq!(fixture.read(a), one);
    let merged = fixture.merge(a, c);
    assert_eq!(merged.sessions.len(), 4);
    assert!(merged.sessions.iter().all(|root| fixture.read(*root) == merged));
    assert_eq!(fixture.count("agent_collaboration_graph_members"), 4);
    let before = fixture.count("agent_collaboration_graph_history");
    let same = fixture
        .change(
            b,
            AgentGraphRevision::default(),
            Action::Merge {
                other_session_id: d,
                expected_other: AgentGraphRevision::default(),
                confirmed: false,
                positions: vec![],
            },
        )
        .expect("duplicate drag");
    assert_eq!(same, merged);
    assert_eq!(fixture.count("agent_collaboration_graph_history"), before);
    let old: String = fixture.db.query_row("SELECT snapshot FROM agent_collaboration_graph_history WHERE graph_id=?1 AND revision=?2", rusqlite::params![two.graph_id.expect("source id").to_string(),i64::try_from(two.revision).expect("revision")], |row| row.get(0)).expect("old group preserved");
    let archived: aura_agent_collaboration::AgentGraph =
        serde_json::from_str(&old).expect("snapshot");
    assert_eq!(archived, two);
}

#[test]
fn session_agent_graph_foreign_workspace_or_temporary_root_cannot_join() {
    let mut fixture = Fixture::new();
    let [a, b, _, _] = fixture.roots;
    fixture.access.ordinary_sessions.remove(&b);
    assert!(
        fixture
            .change(
                a,
                AgentGraphRevision::default(),
                Action::Merge {
                    other_session_id: b,
                    expected_other: AgentGraphRevision::default(),
                    confirmed: true,
                    positions: vec![]
                }
            )
            .is_err()
    );
    assert!(
        fixture
            .change(fixture.children[0], AgentGraphRevision::default(), Action::ResetLayout)
            .is_err()
    );
    assert_eq!(fixture.count("agent_collaboration_graphs"), 0);
}

#[test]
fn session_agent_graph_session_deletion_invalidates_old_membership_revision() {
    let mut fixture = Fixture::new();
    let [a, b, _, _] = fixture.roots;
    let joined = fixture.merge(a, b);
    fixture
        .db
        .execute("DELETE FROM sessions WHERE id=?1", [b.to_string()])
        .expect("delete session fixture");
    let remaining = fixture.read(a);
    assert_eq!(remaining.sessions, [a]);
    assert!(remaining.revision > joined.revision);
    assert!(fixture.change(a, joined.expected(), Action::ResetLayout).is_err());
    assert_eq!(
        fixture.count("agent_collaboration_graph_history"),
        1,
        "previous membership snapshot remains"
    );
}
