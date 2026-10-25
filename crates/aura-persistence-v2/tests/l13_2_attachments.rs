//! A queued message keeps its
//! pictures and files with the conversation; when it becomes part of the conversation the model
//! reads what the user typed with its attachments, while the conversation shows what the user
//! typed with a card for each. A repeated command does not keep them twice, and a removed message
//! lets them go.
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::PersistenceCoreError as Error;
use aura_persistence_v2::{
    V2Store,
    agent::{
        attachments::{self, PreparedAttachment},
        desktop_messages, inbox, input_consumption, lifecycle, recovery, turns,
    },
};

const PNG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
const PNG_BASE64: &str = "iVBORw0KGgoAAAAN";
const NOTES: &str = "first line\nsecond ``` line";

fn drafts() -> Vec<AgentAttachmentDraft> {
    vec![
        AgentAttachmentDraft::Image {
            name: "shot.png".into(),
            media_type: "image/png".into(),
            data_base64: PNG_BASE64.into(),
        },
        AgentAttachmentDraft::Text { name: "notes.md".into(), text: NOTES.into() },
        AgentAttachmentDraft::File { path: "C:/synthetic/docs/spec.pdf".into() },
    ]
}

fn prepared() -> Vec<PreparedAttachment> {
    vec![
        PreparedAttachment::Image {
            name: "shot.png".into(),
            media_type: "image/png".into(),
            bytes: PNG.to_vec(),
        },
        PreparedAttachment::Text { name: "notes.md".into(), text: NOTES.into() },
        PreparedAttachment::File {
            name: "spec.pdf".into(),
            path: "docs/spec.pdf".into(),
            byte_length: 2_411_724,
            listing: None,
        },
    ]
}

fn request(session: SessionId, text: &str, attached: bool) -> AgentSubmitRequest {
    AgentSubmitRequest {
        session_id: session,
        text: text.into(),
        destination: AgentInputLane::NextTurn,
        skill_ids: Vec::new(),
        reasoning_effort: None,
        attachments: if attached { drafts() } else { Vec::new() },
    }
}

#[test]
fn a_message_keeps_its_attachments_and_the_model_reads_them_with_its_text() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let session = f.turn.session_id;
    let command = CommandInvocationId::new();
    let submit = |store: &mut V2Store, command, text: &str, attached: bool| {
        store
            .database
            .unit_of_work(|tx| {
                let prepared = if attached { prepared() } else { Vec::new() };
                inbox::submit_message(
                    tx,
                    command,
                    &request(session, text, attached),
                    &[],
                    &prepared,
                    f.at,
                )
            })
            .expect("queued")
    };
    submit(&mut store, command, "看看这些", true);
    // The same command again is the same message: nothing is queued or kept twice.
    let queued = submit(&mut store, command, "看看这些", true);
    assert_eq!(queued.messages.len(), 1);
    let receipts: i64 = store
        .database
        .connection()
        .query_row(
            "SELECT count(*) FROM events WHERE kind='agent.inbox_command' AND instr(payload, ?1) > 0",
            [PNG_BASE64],
            |row| row.get(0),
        )
        .expect("receipts");
    assert_eq!(receipts, 0, "the command's receipt holds digests, not the picture");
    // A message of attachments alone has no words of its own.
    submit(&mut store, CommandInvocationId::new(), "", true);
    let kept = queued.messages[0].attachments.clone();
    let [
        AgentMessageAttachment::Image { artifact_id: image, byte_length: 12, .. },
        AgentMessageAttachment::Text { artifact_id: notes, .. },
        AgentMessageAttachment::File { path, byte_length: 2_411_724, .. },
    ] = kept.as_slice()
    else {
        panic!("kept: {kept:?}");
    };
    assert_eq!(path, "docs/spec.pdf");
    assert_eq!(
        attachments::sent_by_user(store.database.connection(), session, *image).expect("read"),
        Some((ContentHash::sha256(PNG), 12))
    );
    assert_eq!(
        attachments::sent_by_user(store.database.connection(), session, ArtifactId::new())
            .expect("read"),
        None
    );

    // The turn they start: the model reads the typed text with the attachments.
    store
        .database
        .unit_of_work(|tx| recovery::interrupt(tx, f.turn.agent_run_id, "stopped", true, f.at))
        .expect("stop");
    let paused = inbox::load(store.database.connection(), session).expect("inbox");
    let pending = store
        .database
        .unit_of_work(|tx| {
            inbox::mutate(
                tx,
                CommandInvocationId::new(),
                &AgentInboxMutation {
                    session_id: session,
                    expected_revision: paused.revision,
                    action: AgentInboxAction::Resume,
                },
                f.at,
            )
        })
        .expect("resume");
    let first = &pending.messages[0];
    let user =
        attachments::freeze(store.database.connection(), first, session, f.at).expect("root");
    assert!(user.text.starts_with("看看这些\n\nAttached to this message:\n- picture `shot.png`"));
    assert!(user.text.contains("- workspace file `docs/spec.pdf` (2.3 MB)"), "{}", user.text);
    assert!(user.text.ends_with("`notes.md`:\n````\nfirst line\nsecond ``` line\n````"));
    let prior = turns::load_run(store.database.connection(), f.turn.agent_run_id)
        .expect("run")
        .prompt_bundle_snapshot_id;
    let mut bundle = aura_persistence_v2::agent::model_context::bundle(
        store.database.connection(),
        prior,
        session,
    )
    .expect("bundle");
    bundle.id = PromptBundleSnapshotId::new();
    bundle.turn_task_id = TurnTaskId::new();
    let turn = store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::context::insert_bundle(tx, session, &bundle)?;
            let turn = lifecycle::start(tx, &user, AgentTaskMode::Normal, None, bundle.id)?;
            attachments::record(tx, user.event_id, &first.text, &first.attachments, f.at)?;
            input_consumption::start_batch(tx, &pending, &turn, f.at)?;
            Ok::<_, Error>(turn)
        })
        .expect("turn");

    // The conversation shows what the user typed, with the attachments beside it.
    let db = store.database.connection();
    let page = desktop_messages::page(db, &turn, None).expect("page");
    let shown = page.users.iter().find(|u| u.event_id == user.event_id).expect("root shown");
    assert_eq!((shown.text.as_str(), shown.total_bytes), ("看看这些", "看看这些".len() as u64));
    assert_eq!(shown.attachments, kept);
    let alone = page.users.iter().find(|u| u.event_id != user.event_id).expect("second shown");
    assert_eq!((alone.text.as_str(), alone.total_bytes), ("", 0));
    assert_eq!(alone.attachments.len(), 3);
    let read = desktop_messages::text(
        db,
        &AgentMessageTextRequest {
            session_id: session,
            source: AgentMessageTextSource::User { event_id: user.event_id },
            offset: 0,
            limit: 65_536,
        },
    )
    .expect("text");
    assert_eq!((read.text.as_str(), read.total_bytes, read.next_offset), ("看看这些", 12, None));
    // The text file is read back as it was sent.
    assert_eq!(
        aura_persistence_core::artifact::read(db, *notes, &session.to_string()).expect("notes"),
        NOTES.as_bytes()
    );
}

#[test]
fn a_removed_message_lets_its_attachments_go() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let session = f.turn.session_id;
    let queued = store
        .database
        .unit_of_work(|tx| {
            inbox::submit_message(
                tx,
                CommandInvocationId::new(),
                &request(session, "later", true),
                &[],
                &prepared(),
                f.at,
            )
        })
        .expect("queued");
    let message = &queued.messages[0];
    let AgentMessageAttachment::Image { artifact_id, .. } = message.attachments[0] else {
        panic!("an image first");
    };
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
                f.at,
            )
        })
        .expect("removed");
    let db = store.database.connection();
    assert!(aura_persistence_core::artifact::read(db, artifact_id, &session.to_string()).is_err());
    let left: i64 = db
        .query_row("SELECT count(*) FROM artifacts WHERE id=?1", [artifact_id.to_string()], |r| {
            r.get(0)
        })
        .expect("count");
    assert_eq!(left, 0, "nothing else holds the picture, so it is gone");
}
