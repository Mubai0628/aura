use super::*;

fn old_store(path: &Path) {
    let modules =
        MODULES.iter().copied().filter(|m| m.owner != "collaboration").collect::<Vec<_>>();
    let mut database = DatabaseConnection::create_new(path).expect("database");
    schema::initialize(&mut database, &modules).expect("old module inventory");
}

#[test]
fn session_agent_adds_only_known_missing_module_and_preserves_old_tables() {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("aura-v2.db");
    old_store(&path);
    let before = live_structure(&aura_persistence_core::open_reader(&path).expect("reader"))
        .expect("before");
    assert!(migration_pending(&path).expect("pending"));
    migrate(&path).expect("migrate");
    assert!(!migration_pending(&path).expect("current"));
    let db = aura_persistence_core::open_reader(&path).expect("reader");
    let after = live_structure(&db).expect("after");
    assert!(before.iter().all(|row| after.contains(row)));
    schema::verify(&db, MODULES).expect("known complete structure");
}

#[test]
fn session_agent_missing_module_ddl_failure_leaves_inventory_and_tables_unchanged() {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("aura-v2.db");
    old_store(&path);
    let before = live_structure(&aura_persistence_core::open_reader(&path).expect("reader"))
        .expect("before");
    let planned = plan_migration(&path, Check::Whole).expect("plan").expect("new module");
    let mut writer = rusqlite::Connection::open(&path).expect("writer");
    writer
        .authorizer(Some(|context: AuthContext<'_>| match context.action {
            AuthAction::CreateTable { table_name: "agent_collaboration_generations" } => {
                Authorization::Deny
            }
            _ => Authorization::Allow,
        }))
        .expect("fault injection");
    let tx = writer.transaction().expect("transaction");
    assert!(apply_plan(&tx, &planned).is_err());
    assert!(
        live_structure(&tx)
            .expect("during")
            .iter()
            .any(|(name, _)| name == "agent_collaboration_profiles")
    );
    drop(tx);
    drop(writer);
    let reader = aura_persistence_core::open_reader(&path).expect("reader");
    assert_eq!(live_structure(&reader).expect("unchanged"), before);
    let owners: i64 = reader
        .query_row("SELECT count(*) FROM v2_schema_metadata WHERE owner='collaboration'", [], |r| {
            r.get(0)
        })
        .expect("inventory");
    assert_eq!(owners, 0);
    drop(reader);
    migrate(&path).expect("retry");
}

#[test]
fn session_agent_does_not_adopt_unknown_tables_or_missing_legacy_owners() {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("aura-v2.db");
    old_store(&path);
    let writer = rusqlite::Connection::open(&path).expect("writer");
    writer.execute_batch("CREATE TABLE unexpected(value TEXT);").expect("foreign change");
    assert!(migrate(&path).is_err());
    writer
        .execute_batch("DROP TABLE unexpected; DELETE FROM v2_schema_metadata WHERE owner='tool';")
        .expect("missing owner fixture");
    assert!(migrate(&path).is_err());
    let count: i64 = writer
        .query_row(
            "SELECT count(*) FROM sqlite_master WHERE name='agent_collaboration_profiles'",
            [],
            |r| r.get(0),
        )
        .expect("not written");
    assert_eq!(count, 0);
}

fn preceding_collaboration(
    path: &Path,
    marker: &str,
) -> (
    aura_domain_common::SessionId,
    aura_domain_common::WorkspaceId,
    aura_agent_collaboration::AgentProfile,
) {
    let sql = aura_agent_collaboration::SCHEMA;
    let prefix = &sql[..sql.find(marker).expect("frozen predecessor") + 1];
    seed_collaboration_predecessor(path, prefix)
}

fn seed_collaboration_predecessor(
    path: &Path,
    prefix: &'static str,
) -> (
    aura_domain_common::SessionId,
    aura_domain_common::WorkspaceId,
    aura_agent_collaboration::AgentProfile,
) {
    use aura_domain_common::{SessionId, UtcTimestamp, WorkspaceId};
    let modules = MODULES
        .iter()
        .map(|module| SchemaModule {
            owner: module.owner,
            sql: if module.owner == "collaboration" { prefix } else { module.sql },
        })
        .collect::<Vec<_>>();
    let mut database = DatabaseConnection::create_new(path).expect("database");
    schema::initialize(&mut database, &modules).expect("preceding inventory");
    drop(database);
    let mut writer = rusqlite::Connection::open(path).expect("writer");
    writer.execute_batch("PRAGMA foreign_keys=ON;").expect("keys");
    let tx = writer.transaction().expect("transaction");
    let (session, workspace, at) = (SessionId::new(), WorkspaceId::new(), UtcTimestamp::now());
    tx.execute(
        "INSERT INTO workspaces VALUES(?1,'旧工作区','D:\\Fixture','online',1,'{}')",
        [workspace.to_string()],
    )
    .expect("workspace");
    tx.execute(
        "INSERT INTO sessions VALUES(?1,?2,'原会话标题','active',?3,?3)",
        rusqlite::params![session.to_string(), workspace.to_string(), at.to_string()],
    )
    .expect("session");
    let profile = aura_agent_collaboration::profile::set_user(
        &tx,
        workspace,
        "原会话标题",
        &aura_agent_collaboration::AgentProfileSetRequest {
            session_id: session,
            expected_revision: 0,
            description: "必须保留的人工职责".into(),
        },
        at,
    )
    .expect("old profile");
    if prefix.contains("CREATE TABLE agent_collaboration_session_policy") {
        aura_agent_collaboration::policy::set(
            &tx,
            &aura_agent_collaboration::AgentSessionPolicySetRequest {
                session_id: session,
                expected_revision: 0,
                allow_proactive_temporary_agents: true,
            },
            at,
        )
        .expect("old policy");
    }
    tx.commit().expect("seed commit");
    (session, workspace, profile)
}

#[test]
fn session_agent_existing_collaboration_owners_append_graphs_without_rewriting_profiles() {
    for marker in [
        "\nCREATE TABLE agent_collaboration_session_policy",
        "\nCREATE TABLE agent_collaboration_graphs",
        "\nCREATE TABLE agent_collaboration_runs",
        "\nCREATE TABLE agent_collaboration_questions",
    ] {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("aura-v2.db");
        let (session, workspace, profile) = preceding_collaboration(&path, marker);
        assert!(migration_pending(&path).expect("known predecessor must be recognized"));
        migrate(&path).expect("append missing collaboration tables");
        assert!(!migration_pending(&path).expect("current"));
        let reader = aura_persistence_core::open_reader(&path).expect("reader");
        assert_eq!(
            aura_agent_collaboration::profile::load(&reader, session, workspace, "原会话标题")
                .expect("original profile"),
            profile
        );
        let title: String = reader
            .query_row("SELECT title FROM sessions WHERE id=?1", [session.to_string()], |row| {
                row.get(0)
            })
            .expect("original title");
        assert_eq!(title, "原会话标题");
        let graph = aura_agent_collaboration::graph::load(&reader, session, workspace)
            .expect("independent canvas");
        assert!(graph.graph_id.is_none());
        let policy = aura_agent_collaboration::policy::load(&reader, session).expect("policy");
        assert_eq!(policy.allow_proactive_temporary_agents, !marker.contains("session_policy"));
        schema::verify(&reader, MODULES).expect("current structure");
    }
}

#[test]
fn session_agent_migrates_legacy_review_preferences_once() {
    use aura_agent_collaboration::dispatch::review;

    for old_choice in [Some(true), Some(false), None] {
        let temp = tempfile::tempdir().expect("temp");
        let path = temp.path().join("aura-v2.db");
        let (session, _, _) =
            preceding_collaboration(&path, "\nCREATE TABLE agent_collaboration_review_settings");
        if let Some(required) = old_choice {
            let writer = rusqlite::Connection::open(&path).expect("legacy writer");
            writer
                .execute(
                    "INSERT INTO agent_collaboration_review_policy VALUES(?1,4,?2)",
                    rusqlite::params![session.to_string(), required],
                )
                .expect("legacy policy");
        }

        migrate(&path).expect("known predecessor");
        let mut writer = rusqlite::Connection::open(&path).expect("new writer");
        let policy = review::policy(&writer).expect("global policy");
        assert_eq!(policy.required, old_choice.unwrap_or(false));
        assert_eq!(policy.revision, u64::from(policy.required));
        let tx = writer.transaction().expect("settings transaction");
        review::set_policy(&tx, policy.revision, !policy.required).expect("user update");
        tx.commit().expect("update committed");
        drop(writer);

        assert!(!migration_pending(&path).expect("migration applied exactly once"));
        let reader = aura_persistence_core::open_reader(&path).expect("reopened");
        assert_eq!(
            review::policy(&reader).expect("reopened global policy").required,
            !policy.required
        );
    }
}

#[test]
fn session_agent_recognizes_the_development_review_predecessor_without_a_final_newline() {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("aura-v2.db");
    let (sql, _) = aura_agent_collaboration::SCHEMA
        .split_once(GLOBAL_REVIEW_SETTINGS_START)
        .expect("predecessor");
    assert_eq!(
        blake3::hash(sql.as_bytes()).to_hex().as_str(),
        "eebecedbf2129ebb01d8f4390dcc8ba11572f144829c8575c5e90fe38537b31e"
    );
    let (session, workspace, profile) = seed_collaboration_predecessor(&path, sql);
    let writer = rusqlite::Connection::open(&path).expect("legacy writer");
    writer
        .execute(
            "INSERT INTO agent_collaboration_review_policy VALUES(?1,4,1)",
            [session.to_string()],
        )
        .expect("legacy policy");
    drop(writer);

    assert!(migration_pending(&path).expect("recognize the stored development digest"));
    migrate(&path).expect("open the preceding development store");
    assert!(!migration_pending(&path).expect("reopen after one migration"));
    let reader = aura_persistence_core::open_reader(&path).expect("reader");
    schema::verify(&reader, MODULES).expect("canonical structure and digests");
    assert!(
        aura_agent_collaboration::dispatch::review::policy(&reader)
            .expect("global policy")
            .required
    );
    assert_eq!(
        aura_agent_collaboration::profile::load(&reader, session, workspace, "原会话标题")
            .expect("original profile"),
        profile
    );
}

#[test]
fn session_agent_existing_collaboration_graph_ddl_failure_rolls_back_owner_and_tables() {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("aura-v2.db");
    let (session, workspace, profile) =
        preceding_collaboration(&path, "\nCREATE TABLE agent_collaboration_graphs");
    let reader = aura_persistence_core::open_reader(&path).expect("reader");
    let before = live_structure(&reader).expect("before");
    let digest: String = reader
        .query_row("SELECT digest FROM v2_schema_metadata WHERE owner='collaboration'", [], |row| {
            row.get(0)
        })
        .expect("old digest");
    drop(reader);
    let planned = plan_migration(&path, Check::Whole).expect("known plan").expect("upgrade");
    let mut writer = rusqlite::Connection::open(&path).expect("writer");
    writer
        .authorizer(Some(|context: AuthContext<'_>| match context.action {
            AuthAction::CreateTable { table_name: "agent_collaboration_graph_history" } => {
                Authorization::Deny
            }
            _ => Authorization::Allow,
        }))
        .expect("fault injection");
    let tx = writer.transaction().expect("transaction");
    assert!(apply_plan(&tx, &planned).is_err());
    drop(tx);
    drop(writer);
    let reader = aura_persistence_core::open_reader(&path).expect("reader");
    assert_eq!(live_structure(&reader).expect("rolled back"), before);
    let after_digest: String = reader
        .query_row("SELECT digest FROM v2_schema_metadata WHERE owner='collaboration'", [], |row| {
            row.get(0)
        })
        .expect("digest");
    assert_eq!(after_digest, digest);
    assert_eq!(
        aura_agent_collaboration::profile::load(&reader, session, workspace, "原会话标题")
            .expect("profile"),
        profile
    );
    drop(reader);
    migrate(&path).expect("explicit retry after known rollback");
}

#[test]
fn session_agent_dispatch_upgrade_preserves_the_b3_graph_and_profiles() {
    let temp = tempfile::tempdir().expect("temp");
    let path = temp.path().join("b3.db");
    let (a, work, profile) =
        preceding_collaboration(&path, "\nCREATE TABLE agent_collaboration_runs");
    let mut writer = rusqlite::Connection::open(&path).expect("old writer");
    let b = aura_domain_common::SessionId::new();
    let at = aura_domain_common::UtcTimestamp::now();
    let id = aura_domain_common::AgentCollaborationGraphId::new();
    let edge =
        aura_agent_collaboration::AgentGraphEdge { source_session_id: a, target_session_id: b };
    let raw = serde_json::json!({"edges":[edge],"positions":[]}).to_string();
    let tx = writer.transaction().expect("old graph");
    tx.execute(
        "INSERT INTO sessions VALUES(?1,?2,'B','active',?3,?3)",
        rusqlite::params![b.to_string(), work.to_string(), at.to_string()],
    )
    .expect("B");
    tx.execute(
        "INSERT INTO agent_collaboration_graphs VALUES(?1,?2,7,?3,?4,?4)",
        rusqlite::params![id.to_string(), work.to_string(), raw, at.to_string()],
    )
    .expect("old graph row");
    for member in [a, b] {
        tx.execute(
            "INSERT INTO agent_collaboration_graph_members VALUES(?1,?2)",
            [member.to_string(), id.to_string()],
        )
        .expect("old member");
    }
    tx.commit().expect("old commit");
    drop(writer);
    assert!(migration_pending(&path).expect("recognized exact B3"));
    migrate(&path).expect("append B4 only");
    assert!(!migration_pending(&path).expect("aligned"));
    let db = aura_persistence_core::open_reader(&path).expect("reader");
    let saved = aura_agent_collaboration::graph::load(&db, a, work).expect("graph retained");
    assert_eq!(saved.graph_id, Some(id));
    assert_eq!(saved.revision, 7);
    assert_eq!(saved.edges, vec![edge]);
    assert_eq!(
        aura_agent_collaboration::profile::load(&db, a, work, "原会话标题")
            .expect("profile retained"),
        profile
    );
    assert!(
        aura_agent_collaboration::dispatch::receivers(&db)
            .expect("new outbox is present")
            .is_empty()
    );
}
