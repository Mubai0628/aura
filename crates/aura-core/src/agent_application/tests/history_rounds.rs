//! The conversation by round. A round is one task, from the user's request until it was
//! completed; the history tools list, search and read it by round and entry, and a compaction
//! leaves its one paragraph and the latest rounds of its time in the context.
use super::l10_15_compaction::start;
use super::l10_19_compaction::{asks_compaction, before_the_third};
use super::*;
use aura_context_domain::{HistoryListTurnsInput, HistoryReadInput};

const LOOK_BACK: Scenario = Scenario::Sequence {
    calls: &[
        ("history.list_turns", "{}"),
        ("history.search", r#"{"query":"ANSWER finish"}"#),
        ("history.read", r#"{"entries":["T1.1"],"offset":10,"limit":6}"#),
    ],
};

fn completed(view: &AgentRunProjection) -> bool {
    matches!(view.turn.state, TurnTaskState::Completed { .. })
}

/// The tool results the model got, in order, as the last request carries them.
fn returned(provider: &Provider) -> Vec<Value> {
    let requests = provider.requests.lock().expect("requests").clone();
    requests
        .last()
        .and_then(|request| request["messages"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| {
            serde_json::from_str(message["content"].as_str().expect("text")).expect("a result")
        })
        .collect()
}

/// Reads or writes Core's store directly.
fn with_store<T>(
    runtime: &crate::RuntimeState,
    work: impl FnOnce(&rusqlite::Connection) -> T,
) -> T {
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let journal = services.broker.lock().expect("V2 store");
    work(journal.database.connection())
}

#[tokio::test]
async fn the_model_lists_searches_and_reads_its_conversation_by_round_and_entry() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(LOOK_BACK).await;
    let (runtime, run) = waiting::begin(&host, &provider, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: run.turn.session_id, agent_run_id: run.run.agent_run_id };
    waiting::wait(&runtime, &request, completed).await;
    let results = returned(&provider);
    let listed = results[0].to_string();
    assert!(
        listed.contains(r#""id":"T1""#)
            && listed.contains("Check the answer and finish")
            && listed.contains(r#""total":1"#),
        "{listed}"
    );
    let found = results[1].to_string();
    assert!(found.contains(r#""entry":"T1.1""#) && found.contains("user_message"), "{found}");
    assert!(found.contains("answer") && found.contains(r#""complete":true"#), "{found}");
    let read = results[2].to_string();
    assert!(
        read.contains(r#""entry":"T1.1""#)
            && read.contains(r#""text":"answer""#)
            && read.contains(r#""chars":27"#)
            && read.contains(r#""next_offset":16"#),
        "a page of the message's text: {read}"
    );
}

/// A turn stopped before it completed leaves its round open: the message after it belongs to the
/// same round, and a message after a completed turn begins the next one.
#[tokio::test]
async fn rounds_follow_how_the_conversations_turns_ended() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, session, model) = waiting::prepare(&host, &provider).await;
    for text in ["AURA-ROUND-A: build it", "AURA-ROUND-B: go on", "AURA-ROUND-C: something new"] {
        let run = start(&runtime, session, model, text).await;
        waiting::wait(&runtime, &run, completed).await;
    }
    let list = |db: &rusqlite::Connection| {
        aura_persistence_v2::agent::rounds::list(db, session, &HistoryListTurnsInput::default())
            .expect("rounds")
    };
    assert_eq!(with_store(&runtime, list).total, 3, "three completed tasks");
    // The first turn was stopped, not completed: the second one goes on with its task.
    with_store(&runtime, |db| {
        let first = db
            .query_row(
                "SELECT id FROM turn_tasks WHERE session_id=?1                  ORDER BY json_extract(payload,'$.created_at') LIMIT 1",
                [session.to_string()],
                |row| row.get::<_, String>(0),
            )
            .expect("the first turn");
        db.execute(
            "UPDATE turn_tasks SET payload=json_set(payload,'$.state',json('{\"state\":\"cancelled\"}')) WHERE id=?1",
            [first],
        )
        .expect("stopped");
    });
    let rounds = with_store(&runtime, list);
    let firsts = rounds.turns.iter().map(|round| round.first_message.as_str()).collect::<Vec<_>>();
    assert_eq!(firsts, ["AURA-ROUND-A: build it", "AURA-ROUND-C: something new"]);
    let ids = rounds.turns.iter().map(|round| round.id.as_str()).collect::<Vec<_>>();
    assert_eq!(ids, ["T1", "T2"]);
    let read = with_store(&runtime, |db| {
        aura_persistence_v2::agent::rounds::read(
            db,
            session,
            &HistoryReadInput { entries: vec!["T1.3".into()], offset: None, limit: None },
        )
        .expect("an entry of the first round")
    });
    assert_eq!(read.entries[0].text, "AURA-ROUND-B: go on", "the second turn's message");
}

#[tokio::test]
async fn a_compaction_leaves_one_paragraph_and_the_latest_rounds_of_its_time() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::CompactOnPrefix).await;
    let (runtime, session, model) = before_the_third(&host, &provider, 64_000).await;
    let third = start(&runtime, session, model, "AURA-ROUNDS-THIRD: answer at length").await;
    waiting::wait(&runtime, &third, completed).await;
    let requests = provider.requests.lock().expect("provider requests").clone();
    assert_eq!(requests.iter().filter(|request| asks_compaction(request)).count(), 1);
    let after = requests
        .iter()
        .rfind(|request| !asks_compaction(request))
        .expect("the step's request after the compaction")
        .to_string();
    assert!(after.contains("Earlier in this conversation, summarized"), "{after}");
    assert!(after.contains("The prior response is summarized here"), "{after}");
    assert!(after.contains("T1 · ") && after.contains("AURA-L1019-FIRST: answer briefly"));
    assert!(after.contains("T2 · ") && after.contains(" characters)"), "the long message's length");
    assert!(after.contains("T3 · ") && after.contains("AURA-ROUNDS-THIRD"), "{after}");
    assert!(!after.contains("Truth-Index") && !after.contains("anchor_manifest"));
}
