//! The MCP secret index and OAuth sign-ins. The frozen `tool.sql` before them upgrades by
//! adding only these two tables; a secret belongs to one server and advances its revision
//! before every write; a sign-in keeps its encrypted tokens and is replaced or forgotten whole.
use aura_domain_common::{CredentialId, UtcTimestamp};
use aura_persistence_core::{DatabaseConnection, StoreResult, schema};
use aura_persistence_v2::mcp::{
    OAuthSession, SecretState, forget_oauth, forget_secret, oauth, oauth_servers, register_secret,
    save_oauth, secret, secret_stored, secrets,
};
use aura_persistence_v2::{MODULES, V2Store};
use rusqlite::{Connection, OpenFlags};
use serde_json::json;

const BEFORE: &str = include_str!("fixtures/tool-before-l08b5.sql");

fn tables(connection: &Connection) -> usize {
    ["mcp_secrets", "mcp_oauth_sessions"]
        .iter()
        .filter(|table| {
            connection
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [**table],
                    |row| row.get::<_, i64>(0),
                )
                .expect("catalog")
                == 1
        })
        .count()
}

#[test]
fn the_l08_schema_upgrades_once_adding_only_the_credential_tables() {
    let temp = tempfile::tempdir().expect("temporary namespace");
    let root = temp.path().join("v2");
    std::fs::create_dir(&root).expect("root");
    let mut database = DatabaseConnection::create_new(&root.join("aura-v2.db")).expect("db");
    let modules = MODULES
        .iter()
        .map(|module| schema::SchemaModule {
            owner: module.owner,
            sql: if module.owner == "tool" { BEFORE } else { module.sql },
        })
        .collect::<Vec<_>>();
    schema::initialize(&mut database, &modules).expect("frozen L08 schema");
    database
        .connection()
        .execute(
            "INSERT INTO mcp_catalogs(server_id,config_fingerprint,protocol_version,server_info,catalog,discovered_at) VALUES('files','f','2025-11-25','{}','{}','2026-10-17T00:00:00Z')",
            [],
        )
        .expect("an L08 catalog row");
    drop(database);
    std::fs::write(
        root.join("v2-development.json"),
        b"{\"schema_version\":1,\"namespace\":\"aura-v2-development\"}\n",
    )
    .expect("namespace");
    let reader =
        Connection::open_with_flags(root.join("aura-v2.db"), OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("read fixture");
    assert_eq!(tables(&reader), 0);
    drop(reader);

    let store = V2Store::open(&root).expect("supported L08 upgrade");
    assert_eq!(tables(store.database.connection()), 2);
    let kept: String = store
        .database
        .connection()
        .query_row("SELECT server_id FROM mcp_catalogs", [], |row| row.get(0))
        .expect("kept catalog");
    assert_eq!(kept, "files");
    drop(store);
    V2Store::open(&root).expect("reopen after the upgrade");
}

#[test]
fn a_secret_belongs_to_one_server_and_advances_before_each_write() {
    let temp = tempfile::tempdir().expect("temp");
    let mut store = V2Store::create(&temp.path().join("v2")).expect("create");
    let id = CredentialId::new();
    let at = UtcTimestamp::now();
    let first =
        store.database.unit_of_work(|tx| register_secret(tx, id, "web", at)).expect("registered");
    assert_eq!((first.revision, first.state), (1, SecretState::Pending));
    store.database.unit_of_work(|tx| secret_stored(tx, id, 1, at)).expect("stored");
    let second = store
        .database
        .unit_of_work(|tx| register_secret(tx, id, "web", at))
        .expect("the next value");
    assert_eq!((second.revision, second.state), (2, SecretState::Pending));
    // A late confirmation of the earlier value does not mark the new one stored.
    assert!(store.database.unit_of_work(|tx| secret_stored(tx, id, 1, at)).is_err());
    assert!(
        store.database.unit_of_work(|tx| register_secret(tx, id, "other", at)).is_err(),
        "another server never takes this secret"
    );
    store.database.unit_of_work(|tx| secret_stored(tx, id, 2, at)).expect("stored");
    let db = store.database.connection();
    let known = secret(db, id).expect("read").expect("known");
    assert_eq!((known.server_id.as_str(), known.revision), ("web", 2));
    assert_eq!(known.state, SecretState::Stored);
    assert_eq!(secrets(db).expect("all").len(), 1);
    store.database.unit_of_work(|tx| forget_secret(tx, id)).expect("forgotten");
    assert_eq!(secret(store.database.connection(), id).expect("read"), None);
}

#[test]
fn a_sign_in_is_kept_replaced_and_forgotten_whole() {
    let temp = tempfile::tempdir().expect("temp");
    let mut store = V2Store::create(&temp.path().join("v2")).expect("create");
    let at = UtcTimestamp::now();
    let session = OAuthSession {
        server_id: "web".into(),
        config_fingerprint: "fingerprint".into(),
        credential_id: CredentialId::new(),
        revision: 1,
        authorization: json!({"issuer": "https://auth.example.com", "client_id": "aura"}),
        protected_tokens: vec![1, 2, 3, 0, 255],
        expires_at: Some(at),
        updated_at: at,
    };
    let save = |store: &mut V2Store, session: &OAuthSession| -> StoreResult<()> {
        store.database.unit_of_work(|tx| save_oauth(tx, session))
    };
    save(&mut store, &session).expect("signed in");
    assert_eq!(oauth(store.database.connection(), "web").expect("read"), Some(session.clone()));
    let refreshed = OAuthSession {
        revision: 2,
        protected_tokens: vec![9; 4000],
        expires_at: None,
        ..session.clone()
    };
    save(&mut store, &refreshed).expect("refreshed");
    assert_eq!(oauth(store.database.connection(), "web").expect("read"), Some(refreshed));
    assert_eq!(oauth_servers(store.database.connection()).expect("servers"), ["web"]);
    store.database.unit_of_work(|tx| forget_oauth(tx, "web")).expect("signed out");
    assert_eq!(oauth(store.database.connection(), "web").expect("read"), None);
}
