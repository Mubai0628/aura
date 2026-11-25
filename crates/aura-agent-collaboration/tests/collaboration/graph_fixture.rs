use aura_agent_collaboration::{
    AgentGraph, AgentGraphAction, AgentGraphMutationRequest, AgentGraphRevision, SCHEMA, graph,
};
use aura_domain_common::{SessionId, UtcTimestamp, WorkspaceId};
use aura_persistence_core::StoreResult;
use rusqlite::Connection;

pub struct Fixture {
    pub db: Connection,
    pub roots: [SessionId; 4],
    pub children: [SessionId; 2],
    pub access: graph::GraphAccess,
}
impl Fixture {
    pub fn new() -> Self {
        Self::using(Connection::open_in_memory().expect("database"))
    }
    pub fn using(db: Connection) -> Self {
        db.execute_batch("PRAGMA foreign_keys=ON; CREATE TABLE sessions(id TEXT PRIMARY KEY);")
            .expect("base");
        db.execute_batch(SCHEMA).expect("schema");
        let roots = std::array::from_fn(|_| SessionId::new());
        let children = std::array::from_fn(|_| SessionId::new());
        for id in roots.iter().chain(&children) {
            db.execute("INSERT INTO sessions(id) VALUES(?1)", [id.to_string()]).expect("session");
        }
        let access = graph::GraphAccess {
            workspace_id: WorkspaceId::new(),
            ordinary_sessions: roots.into_iter().collect(),
            temporary_owners: [(children[0], roots[0]), (children[1], roots[1])]
                .into_iter()
                .collect(),
        };
        Self { db, roots, children, access }
    }
    pub fn read(&self, root: SessionId) -> AgentGraph {
        graph::load(&self.db, root, self.access.workspace_id).expect("graph")
    }
    pub fn change(
        &mut self,
        root: SessionId,
        expected_graph: AgentGraphRevision,
        action: AgentGraphAction,
    ) -> StoreResult<AgentGraph> {
        let tx = self.db.transaction()?;
        let result = graph::apply(
            &tx,
            &AgentGraphMutationRequest { session_id: root, expected_graph, action },
            &self.access,
            UtcTimestamp::now(),
        )?;
        tx.commit()?;
        Ok(result)
    }
    pub fn merge(&mut self, root: SessionId, other: SessionId) -> AgentGraph {
        let expected_other = self.read(other).expected();
        self.change(
            root,
            self.read(root).expected(),
            AgentGraphAction::Merge {
                other_session_id: other,
                expected_other,
                confirmed: true,
                positions: vec![],
            },
        )
        .expect("merge")
    }
    pub fn count(&self, table: &str) -> i64 {
        assert!(
            [
                "agent_collaboration_graphs",
                "agent_collaboration_graph_members",
                "agent_collaboration_graph_history"
            ]
            .contains(&table)
        );
        self.db
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0))
            .expect("count")
    }
}
