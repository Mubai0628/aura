//! Skills chosen with a message. A queued message keeps them; only a message that starts
//! a new turn may carry them, so neither an addition nor moving a queued message into the
//! running task can. Each chosen Skill is written out when the message is sent and follows what
//! the user typed when the message becomes part of the conversation; its history shows what was
//! typed, with the choice.
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::PersistenceCoreError as Error;
use aura_persistence_v2::{
    V2Store,
    agent::{attachments, inbox, skills},
    session_purge,
};

fn chosen(name: &str) -> LoadedSkillEntry {
    LoadedSkillEntry {
        skill_id: SkillInstanceId::new(),
        name: name.into(),
        origin: SkillOrigin::AuraStore,
        fingerprint: "a".repeat(64),
    }
}

#[test]
fn a_queued_message_keeps_its_skills_and_only_a_new_turn_may_carry_them() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let notes = chosen("notes");
    let request = |destination, skill_ids: Vec<SkillInstanceId>| AgentSubmitRequest {
        computer_mode: None,
        session_id: f.turn.session_id,
        text: "with a Skill".into(),
        destination,
        skill_ids,
        reasoning_effort: None,
        speed_mode: None,
        attachments: Vec::new(),
        references: Vec::new(),
    };
    let queued = store
        .database
        .unit_of_work(|tx| {
            inbox::submit_with_skills(
                tx,
                CommandInvocationId::new(),
                &request(AgentInputLane::NextTurn, vec![notes.skill_id]),
                std::slice::from_ref(&notes),
                f.at,
            )
        })
        .expect("queued");
    let message = queued.messages[0].clone();
    assert_eq!(message.skills, std::slice::from_ref(&notes));

    // An addition to the running task cannot carry a Skill.
    let addition = store.database.unit_of_work(|tx| {
        inbox::submit_with_skills(
            tx,
            CommandInvocationId::new(),
            &request(
                AgentInputLane::Addition { turn_task_id: f.turn.turn_task_id },
                vec![notes.skill_id],
            ),
            std::slice::from_ref(&notes),
            f.at,
        )
    });
    assert!(matches!(addition, Err(Error::Invalid("agent.skills_need_new_turn"))), "{addition:?}");

    // Neither can the queued message be moved into it.
    let moved = store.database.unit_of_work(|tx| {
        inbox::mutate(
            tx,
            CommandInvocationId::new(),
            &AgentInboxMutation {
                session_id: f.turn.session_id,
                expected_revision: queued.revision,
                action: AgentInboxAction::Append {
                    message_id: message.message_id,
                    turn_task_id: f.turn.turn_task_id,
                },
            },
            f.at,
        )
    });
    assert!(matches!(moved, Err(Error::Invalid("agent.skills_need_new_turn"))), "{moved:?}");
    let still = inbox::load(store.database.connection(), f.turn.session_id).expect("inbox");
    assert_eq!(still.messages, std::slice::from_ref(&message), "the queue is unchanged");

    // The choice is kept per message for its history.
    store
        .database
        .unit_of_work(|tx| {
            skills::set_selected(tx, message.message_id, std::slice::from_ref(&notes), f.at)
        })
        .expect("recorded");
    let db = store.database.connection();
    assert_eq!(
        skills::selected(db, message.message_id).expect("selected"),
        std::slice::from_ref(&notes)
    );
    assert!(skills::selected(db, EventId::new()).expect("none").is_empty());
}

#[test]
fn a_queued_messages_chosen_skills_are_written_out_when_sent_and_follow_what_was_typed() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let session = f.turn.session_id;
    let notes = chosen("notes");
    let whole = "Chosen Skill \"notes\" [aura_store] (folder C:/notes; files: none besides SKILL.md). Follow it.\n\nWrite short notes.".to_owned();
    let request = AgentSubmitRequest {
        computer_mode: None,
        session_id: session,
        text: "Use it".into(),
        destination: AgentInputLane::NextTurn,
        skill_ids: vec![notes.skill_id],
        reasoning_effort: None,
        speed_mode: None,
        attachments: Vec::new(),
        references: Vec::new(),
    };
    let queued = store
        .database
        .unit_of_work(|tx| {
            inbox::submit_chosen(
                tx,
                CommandInvocationId::new(),
                &request,
                std::slice::from_ref(&notes),
                std::slice::from_ref(&whole),
                &[],
                f.at,
            )
        })
        .expect("queued");
    let message = queued.messages[0].clone();
    let db = store.database.connection();
    assert_eq!(
        skills::chosen_texts(db, message.message_id).expect("written"),
        std::slice::from_ref(&whole)
    );
    assert!(skills::chosen_texts(db, EventId::new()).expect("none").is_empty());

    // Taken into the conversation, the message is what was typed, then the Skill whole.
    let user = attachments::freeze(db, &message, session, f.at).expect("frozen");
    assert_eq!(user.event_id, message.message_id);
    assert_eq!(user.text, format!("Use it\n\n{whole}"));
    assert_eq!(user.text, message_with_skills("Use it", std::slice::from_ref(&whole)));

    // Its history shows what was typed; a message that carries only that records nothing.
    let plain = EventId::new();
    store
        .database
        .unit_of_work(|tx| {
            attachments::record(tx, user.event_id, &message.text, &user.text, &[], f.at)?;
            attachments::record(tx, plain, "Plain", "Plain", &[], f.at)
        })
        .expect("recorded");
    let db = store.database.connection();
    assert_eq!(
        attachments::of_message(db, user.event_id).expect("read"),
        Some(("Use it".len() as u64, Vec::new()))
    );
    assert_eq!(attachments::of_message(db, plain).expect("read"), None);

    // Removing the session removes what was written out for its queued message.
    session_purge::purge_now(&mut store.database, |db| session_purge::plan_session(db, session))
        .expect("purged");
    let db = store.database.connection();
    assert!(skills::chosen_texts(db, message.message_id).expect("read").is_empty());
}
