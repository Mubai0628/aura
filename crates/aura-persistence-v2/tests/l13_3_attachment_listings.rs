//! ZIP listings are user-submitted content owned by the conversation, never a shared file reader.
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;

use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::{PersistenceCoreError as Error, artifact};
use aura_persistence_v2::{
    V2Store,
    agent::{
        attachments::{self, PreparedAttachment},
        inbox,
    },
};

const WORKSPACE_LIST: &str = "docs/notes.md\t12 B\nphotos/\t0 B";
const EXTERNAL_LIST: &str = "源文件/main.rs\t120 B\nprivate.dat\t64 B\n另有 3 项";

fn queue(
    store: &mut V2Store,
    fixture: &support::Fixture,
) -> (AgentInboxProjection, ExternalGrantId) {
    let grant = ExternalGrantId::new();
    let request = AgentSubmitRequest {
        session_id: fixture.turn.session_id,
        text: "比较两个 ZIP".to_owned(),
        destination: AgentInputLane::NextTurn,
        skill_ids: Vec::new(),
        reasoning_effort: None,
        attachments: vec![
            AgentAttachmentDraft::File { path: "C:/synthetic/local.zip".to_owned() },
            AgentAttachmentDraft::File { path: "D:/Attachments/external.zip".to_owned() },
        ],
    };
    let prepared = vec![
        PreparedAttachment::File {
            name: "local.zip".to_owned(),
            path: "local.zip".to_owned(),
            byte_length: 1024,
            listing: Some(WORKSPACE_LIST.to_owned()),
        },
        PreparedAttachment::ExternalFile {
            name: "external.zip".to_owned(),
            path: "D:/Attachments/external.zip".to_owned(),
            byte_length: 2048,
            external_grant_id: grant,
            listing: Some(EXTERNAL_LIST.to_owned()),
        },
    ];
    let queued = store
        .database
        .unit_of_work(|tx| {
            inbox::submit_message(
                tx,
                CommandInvocationId::new(),
                &request,
                &[],
                &prepared,
                fixture.at,
            )
        })
        .expect("queue ZIP attachments");
    (queued, grant)
}

fn listings(attachments: &[AgentMessageAttachment]) -> [ArtifactId; 2] {
    let [
        AgentMessageAttachment::File { listing_artifact_id: Some(local), .. },
        AgentMessageAttachment::ExternalFile { listing_artifact_id: Some(external), .. },
    ] = attachments
    else {
        panic!("both ZIP listings must be persisted: {attachments:?}");
    };
    [*local, *external]
}

#[test]
fn zip_listings_survive_reload_and_freeze_as_user_content_with_session_scoped_access() {
    let root = tempfile::tempdir().expect("fixture root");
    let mut store = V2Store::create(&root.path().join("v2")).expect("fixture store");
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let (queued, grant) = queue(&mut store, &fixture);
    let session = fixture.turn.session_id;
    let other = support::Fixture::new_in_workspace(&mut store, AgentTaskMode::Normal, "C:/other");
    let db = store.database.connection();
    let reloaded = inbox::load(db, session).expect("reload saved attachments");
    assert_eq!(reloaded.messages[0].attachments, queued.messages[0].attachments);
    let message = &reloaded.messages[0];
    assert!(
        matches!(&message.attachments[1], AgentMessageAttachment::ExternalFile { external_grant_id, .. } if *external_grant_id == grant)
    );
    let ids = listings(&message.attachments);
    let text = attachments::message_text(db, session, &message.text, &message.attachments)
        .expect("read message listings");
    assert!(text.contains(WORKSPACE_LIST), "{text}");
    assert!(text.contains(EXTERNAL_LIST), "{text}");
    assert!(text.contains(&grant.to_string()), "{text}");
    assert!(text.contains("D:/Attachments/external.zip"), "{text}");
    assert!(text.contains("ZIP 清单（用户附件内容）"), "{text}");
    let frozen =
        attachments::freeze(db, message, session, fixture.at).expect("freeze request content");
    assert_eq!(frozen.text, text);
    for (id, expected) in ids.into_iter().zip([WORKSPACE_LIST, EXTERNAL_LIST]) {
        assert_eq!(
            artifact::read(db, id, &session.to_string()).expect("user listing bytes"),
            expected.as_bytes()
        );
        assert_eq!(
            attachments::sent_by_user(db, session, id).expect("user origin"),
            Some((ContentHash::sha256(expected.as_bytes()), expected.len() as u64))
        );
        assert!(
            matches!(
                artifact::read(db, id, &other.turn.session_id.to_string()),
                Err(Error::Invalid("artifact access"))
            ),
            "a different real session must be denied access"
        );
        assert_eq!(
            attachments::sent_by_user(db, other.turn.session_id, id).expect("other origin"),
            None
        );
    }
    assert!(matches!(
        attachments::message_text(db, other.turn.session_id, &message.text, &message.attachments),
        Err(Error::Invalid("artifact access"))
    ));
}

#[test]
fn removing_the_message_releases_both_listing_artifacts_owners_and_access_links() {
    let root = tempfile::tempdir().expect("fixture root");
    let mut store = V2Store::create(&root.path().join("v2")).expect("fixture store");
    let fixture = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let (queued, _) = queue(&mut store, &fixture);
    let session = fixture.turn.session_id;
    let message = &queued.messages[0];
    let ids = listings(&message.attachments);
    store
        .database
        .unit_of_work(|tx| {
            inbox::mutate(
                tx,
                CommandInvocationId::new(),
                &AgentInboxMutation {
                    session_id: session,
                    expected_revision: queued.revision,
                    action: AgentInboxAction::Remove { message_id: message.message_id },
                },
                fixture.at,
            )
        })
        .expect("remove queued ZIP message");
    let db = store.database.connection();
    for id in ids {
        assert!(matches!(
            artifact::read(db, id, &session.to_string()),
            Err(Error::Invalid("artifact access"))
        ));
        assert_eq!(attachments::sent_by_user(db, session, id).expect("released origin"), None);
        for table in ["artifacts", "artifact_owners", "artifact_access_links"] {
            let column = if table == "artifacts" { "id" } else { "artifact_id" };
            let count: i64 = db
                .query_row(
                    &format!("SELECT count(*) FROM {table} WHERE {column}=?1"),
                    [id.to_string()],
                    |row| row.get(0),
                )
                .expect("released listing count");
            assert_eq!(count, 0, "{table} still holds removed listing {id}");
        }
    }
}
