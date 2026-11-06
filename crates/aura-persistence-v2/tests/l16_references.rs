//! What a message refers to with "@". Core resolves it when the message is sent and it is kept
//! with the message: a queued message carries what it referred to then, and when it becomes part
//! of the conversation the references follow what was typed and its attachments, ahead of the
//! Skills chosen with it. The conversation shows what was typed with what it referred to, and
//! removing the session removes them.
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::PersistenceCoreError as Error;
use aura_persistence_v2::{
    V2Store,
    agent::references,
    agent::{attachments, desktop_messages, inbox, input_consumption, lifecycle, recovery},
    session_purge,
};

fn at(value: &str) -> WorkspacePath {
    WorkspacePath::parse(value).expect("path")
}

fn resolved() -> Vec<ResolvedMessageReference> {
    vec![
        ResolvedMessageReference::WorkspaceFile { path: at("src/lib.rs") },
        ResolvedMessageReference::WorkspaceFolder { path: at("docs") },
    ]
}

fn request(session: SessionId, text: &str) -> AgentSubmitRequest {
    let mut request = AgentSubmitRequest::new(session, text, AgentInputLane::NextTurn);
    request.references = vec![
        AgentMessageReference::WorkspacePath { path: at("src/lib.rs") },
        AgentMessageReference::WorkspacePath { path: at("docs") },
    ];
    request
}

#[test]
fn a_queued_messages_references_follow_what_was_typed_ahead_of_its_chosen_skills() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let session = f.turn.session_id;
    let notes = LoadedSkillEntry {
        skill_id: SkillInstanceId::new(),
        name: "notes".into(),
        origin: SkillOrigin::AuraStore,
        fingerprint: "a".repeat(64),
    };
    let whole = "Chosen Skill \"notes\" [aura_store]. Follow it.\n\nWrite short notes.".to_owned();
    let command = CommandInvocationId::new();
    let submit = |store: &mut V2Store, command, text: &str, chosen: bool| {
        store.database.unit_of_work(|tx| {
            let mut request = request(session, text);
            let (skills, texts) = if chosen {
                request.skill_ids = vec![notes.skill_id];
                (std::slice::from_ref(&notes), std::slice::from_ref(&whole))
            } else {
                (&[][..], &[][..])
            };
            inbox::submit_prepared(
                tx,
                command,
                &request,
                &inbox::PreparedMessage {
                    skills,
                    skill_texts: texts,
                    references: &resolved(),
                    ..Default::default()
                },
                f.at,
            )
        })
    };
    submit(&mut store, command, "Look at these", true).expect("queued");
    // The same command again is the same message: its references are kept once.
    let queued = submit(&mut store, command, "Look at these", true).expect("queued");
    assert_eq!(queued.messages.len(), 1);
    let kept: i64 = store
        .database
        .connection()
        .query_row("SELECT count(*) FROM events WHERE kind='agent.message_references'", [], |row| {
            row.get(0)
        })
        .expect("count");
    assert_eq!(kept, 1);
    // A message of references alone has no words of its own; one with nothing is refused.
    submit(&mut store, CommandInvocationId::new(), "", false).expect("references alone");
    let empty = store.database.unit_of_work(|tx| {
        inbox::submit_prepared(
            tx,
            CommandInvocationId::new(),
            &AgentSubmitRequest::new(session, " ", AgentInputLane::NextTurn),
            &inbox::PreparedMessage::default(),
            f.at,
        )
    });
    assert!(matches!(empty, Err(Error::Invalid("agent.input_invalid"))), "{empty:?}");

    let message = queued.messages[0].clone();
    let views: Vec<_> = resolved().iter().map(ResolvedMessageReference::view).collect();
    assert_eq!(message.references, views, "the queue shows what the message refers to");
    let db = store.database.connection();
    assert_eq!(references::of_message(db, message.message_id).expect("kept"), resolved());
    assert!(references::of_message(db, EventId::new()).expect("none").is_empty());

    // Taken into the conversation: what was typed, then the references, then the Skill whole,
    // the same bytes each time.
    let user = attachments::freeze(db, &message, session, f.at).expect("frozen");
    assert_eq!(
        user.text,
        message_with_skills(
            &message_with_references("Look at these", &resolved()),
            std::slice::from_ref(&whole)
        )
    );
    assert!(user.text.starts_with(
        "Look at these\n\nReferred to in this message:\n- workspace file `src/lib.rs`, not \
         included"
    ));
    assert_eq!(attachments::freeze(db, &message, session, f.at).expect("again").text, user.text);

    // Its words can be edited to nothing but blank: the references keep it a message.
    let paused = inbox::load(db, session).expect("inbox");
    let alone = paused.messages[1].clone();
    store
        .database
        .unit_of_work(|tx| {
            inbox::mutate(
                tx,
                CommandInvocationId::new(),
                &AgentInboxMutation {
                    session_id: session,
                    expected_revision: paused.revision,
                    action: AgentInboxAction::Edit {
                        message_id: alone.message_id,
                        text: " ".into(),
                    },
                },
                f.at,
            )
        })
        .expect("edited");

    // The turn they start: the conversation shows what was typed, with the references; blank
    // words are not part of the text sent and are not shown.
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
    let prior = aura_persistence_v2::agent::turns::load_run(
        store.database.connection(),
        f.turn.agent_run_id,
    )
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
            attachments::record(
                tx,
                user.event_id,
                &first.text,
                &user.text,
                &first.attachments,
                f.at,
            )?;
            input_consumption::start_batch(tx, &pending, &turn, f.at)?;
            Ok::<_, Error>(turn)
        })
        .expect("turn");
    let db = store.database.connection();
    let page = desktop_messages::page(db, &turn, None).expect("page");
    let shown = page.users.iter().find(|u| u.event_id == user.event_id).expect("root shown");
    assert_eq!((shown.text.as_str(), shown.total_bytes), ("Look at these", 13));
    assert_eq!(shown.references, views);
    let alone = page.users.iter().find(|u| u.event_id == alone.message_id).expect("alone shown");
    assert_eq!((alone.text.as_str(), alone.total_bytes), ("", 0));
    assert_eq!(alone.references, views);

    // Removing the session removes what its messages referred to.
    session_purge::purge_now(&mut store.database, |db| session_purge::plan_session(db, session))
        .expect("purged");
    let db = store.database.connection();
    assert!(references::of_message(db, message.message_id).expect("read").is_empty());
    assert!(references::of_message(db, alone.event_id).expect("read").is_empty());
}
