//! What a run's first step decided about Skills: the pinned Skills that go in whole and the list
//! of the other names. The run's later steps read it back unchanged; a run that has not decided
//! has nothing yet.
#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_persistence_v2::{V2Store, agent::skills};

#[test]
fn a_runs_skills_are_read_back_as_its_first_step_wrote_them() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Normal);
    let run = f.turn.agent_run_id;
    assert_eq!(skills::turn_skills(store.database.connection(), run).expect("read"), None);
    let decided = skills::TurnSkills {
        pinned: vec![SkillInstanceId::new(), SkillInstanceId::new()],
        list: Some("Skills you can load:\n- notes\n".into()),
    };
    store
        .database
        .unit_of_work(|tx| skills::set_turn_skills(tx, run, &decided, f.at))
        .expect("written");
    let db = store.database.connection();
    assert_eq!(skills::turn_skills(db, run).expect("read"), Some(decided));
    assert_eq!(skills::turn_skills(db, AgentRunId::new()).expect("another run"), None);
}
