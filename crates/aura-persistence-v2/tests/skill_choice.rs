//! Skills chosen with a message. A queued message keeps them; only a message that starts
//! a new turn may carry them, so neither an addition nor moving a queued message into the
//! running task can. The choice is kept per message (history) and per run (task state).
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_core::PersistenceCoreError as Error;
use aura_persistence_v2::{
    V2Store,
    agent::{inbox, skills},
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
        session_id: f.turn.session_id,
        text: "with a Skill".into(),
        destination,
        skill_ids,
        reasoning_effort: None,
        attachments: Vec::new(),
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

    // The choice is kept per message for its history and per run for the task state.
    store
        .database
        .unit_of_work(|tx| {
            skills::set_selected(tx, message.message_id, std::slice::from_ref(&notes), f.at)?;
            skills::set_chosen(tx, f.turn.agent_run_id, &[notes.skill_id], f.at)
        })
        .expect("recorded");
    let db = store.database.connection();
    assert_eq!(
        skills::selected(db, message.message_id).expect("selected"),
        std::slice::from_ref(&notes)
    );
    assert_eq!(skills::chosen(db, f.turn.agent_run_id).expect("chosen"), [notes.skill_id]);
    assert!(skills::selected(db, EventId::new()).expect("none").is_empty());
}
