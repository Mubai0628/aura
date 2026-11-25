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
            &HistoryReadInput {
                entries: vec!["T1.3".into()],
                offset: None,
                limit: None,
                conversation: None,
            },
        )
        .expect("an entry of the first round")
    });
    assert_eq!(read.entries[0].text, "AURA-ROUND-B: go on", "the second turn's message");
}

#[tokio::test]
async fn history_read_pressure_keeps_the_adopted_prefix() {
    use aura_model_domain::LogicalConversationItem;
    use aura_persistence_v2::agent::model_context::history_for_context;

    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, session, model) = waiting::prepare(&host, &provider).await;
    let first = "FIRST-ADOPTED-SOURCE ".to_owned() + &"a".repeat(3_200);
    let second = "SECOND-ADOPTED-SOURCE ".to_owned() + &"b".repeat(3_200);
    for text in [&first, &second] {
        let request = start(&runtime, session, model, text).await;
        waiting::wait(&runtime, &request, completed).await;
    }
    let third = start(&runtime, session, model, "A short new event").await;
    waiting::wait(&runtime, &third, completed).await;
    let history = with_store(&runtime, |db| {
        history_for_context(db, third.agent_run_id, false, 1_500, None).expect("bounded history")
    });
    let user_messages = history.items.iter().filter_map(|item| match item {
        LogicalConversationItem::User { content, .. } => Some(content.as_str()),
        _ => None,
    });
    let prefixes = user_messages.map(|text| text.split_whitespace().next().unwrap_or_default());
    assert_eq!(
        prefixes.collect::<Vec<_>>(),
        ["FIRST-ADOPTED-SOURCE", "SECOND-ADOPTED-SOURCE", "A"]
    );
}

#[tokio::test]
async fn history_rounds_cold_or_missing_projection_keeps_a_contiguous_recent_range() {
    use aura_model_domain::LogicalConversationItem;
    use aura_persistence_v2::agent::model_context::history_for_context;

    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, session, model) = waiting::prepare(&host, &provider).await;
    let large = "RECENT-LARGE ".to_owned() + &"z".repeat(4_500);
    for text in ["OLD-SMALL: answer", &large] {
        let run = start(&runtime, session, model, text).await;
        waiting::wait(&runtime, &run, completed).await;
    }
    let current = start(&runtime, session, model, "REQUIRED: answer").await;
    waiting::wait(&runtime, &current, completed).await;
    with_store(&runtime, |db| {
        // Simulate a legacy store whose earlier projection bodies cannot be recovered.
        db.execute("DELETE FROM context_snapshots WHERE session_id=?1", [session.to_string()])
            .expect("fixture legacy projections");
        let history = history_for_context(db, current.agent_run_id, false, 800, None)
            .expect("bounded legacy history");
        let users = history
            .items
            .iter()
            .filter_map(|item| match item {
                LogicalConversationItem::User { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(users, ["REQUIRED: answer"]);
    });
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

#[tokio::test]
async fn a_revoked_checkpoint_does_not_hide_events_the_fallback_did_not_cover() {
    use aura_persistence_v2::{agent, context::compaction};

    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::CompactOnPrefix).await;
    let (runtime, session, model) = before_the_third(&host, &provider, 64_000).await;
    let third = start(&runtime, session, model, "AURA-ROUNDS-THIRD: answer at length").await;
    waiting::wait(&runtime, &third, completed).await;
    super::l10_19_compaction::resize(&runtime, model, 1_000_000, 64_000).await;
    let material = "SOURCE-ONLY-COVERED-BY-NEWER-CHECKPOINT";
    let intermediate = start(&runtime, session, model, material).await;
    waiting::wait(&runtime, &intermediate, completed).await;

    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    {
        let mut store = services.broker.lock().expect("event journal");
        store.database.unit_of_work(|tx| -> aura_persistence_core::StoreResult<()> {
            let mut newer = compaction::latest(tx, session)?.expect("original checkpoint");
            let run = agent::turns::load_run(tx, intermediate.agent_run_id)?;
            let turn = agent::turns::load(tx, run.turn_task_id)?;
            newer.id = aura_domain_common::ContextProjectionSnapshotId::new();
            newer.background_sources = vec![compaction::BackgroundSource::Memory {
                global: true, path: "fixture.md".parse().expect("fixture memory path"), version: "fixture".into(),
            }];
            newer.excluded_event_ids.push(turn.root_user_event_id);
            let result = newer.result.as_mut().expect("valid summary");
            result.historical_snapshot = "REVOKED-CHECKPOINT-PAYLOAD".to_owned();
            let artifact = aura_domain_common::ArtifactId::new();
            aura_persistence_core::artifact::put(
                tx, artifact, &newer.id.to_string(), "application/json",
                &serde_json::to_vec(result)?, aura_domain_common::UtcTimestamp::now(),
            )?;
            newer.result_artifact_id = Some(artifact);
            tx.execute(
                "INSERT INTO context_compactions(id,session_id,state,revision,payload) VALUES(?1,?2,'completed',1,?3)",
                [newer.id.to_string(), session.to_string(), serde_json::to_string(&newer)?],
            )?;
            Ok(())
        }).expect("write fixture checkpoint with unavailable authority");
    }
    let current = start(&runtime, session, model, "Continue the local task.").await;
    waiting::wait(&runtime, &current, completed).await;
    let requests = provider.requests.lock().expect("provider request log");
    let wire = requests.last().expect("provider received the continuation");
    assert!(wire.to_string().contains("The prior response is summarized here"));
    assert!(!wire.to_string().contains("REVOKED-CHECKPOINT-PAYLOAD"));
    assert!(
        wire["messages"].as_array().expect("Chat messages").iter().any(|item| {
            item["role"] == "user"
                && item["content"].as_str().is_some_and(|text| text.contains(material))
        }),
        "the older summary left this message uncovered"
    );
}

#[tokio::test]
async fn history_rounds_exhausted_headroom_needs_a_committed_checkpoint_before_normal_send() {
    for summary in [true, false] {
        let host = crate::r03_tests::Fixture::new();
        let log = host.root.path().join("history-read-pressure.jsonl");
        let _log_route = crate::CoreLogRoute::to(log.clone());
        let provider = Provider::scenario(Scenario::HistoryReadPressure { summary }).await;
        let (runtime, session, model) = waiting::prepare(&host, &provider).await;
        super::l10_19_compaction::resize(&runtime, model, 64_000, 4_096).await;
        let old_marker = "EARLIER-ADOPTED-HISTORY";
        let old = format!("{old_marker}\n{}\nEnd of preserved background.", " ".repeat(96_000));
        let initial = start(&runtime, session, model, &old).await;
        waiting::wait(&runtime, &initial, completed).await;
        let file = host.root.path().join("waiting-workspace").join("burst.txt");
        std::fs::write(
            file,
            format!("BURST-RESULT\n{}\nEnd of current result.", " ".repeat(405_000)),
        )
        .expect("write large result fixture");
        let next = start(&runtime, session, model, "AURA-HISTORY-READ burst.txt").await;
        let outcome = waiting::wait(&runtime, &next, |view| {
            view.turn.state.terminal()
                || matches!(view.run.state, AgentRunState::AwaitingContextRemediation { .. })
        })
        .await;

        let requests = provider.requests.lock().expect("provider request log");
        assert!(
            requests[1].to_string().contains(old_marker),
            "the pre-call request adopted the old history"
        );
        let attempted = requests.iter().position(asks_compaction).unwrap_or_else(|| {
            let rows = std::fs::read_to_string(log).unwrap_or_default();
            let context = rows.lines().filter(|r| r.contains("context.")).collect::<Vec<_>>();
            let count = requests.len();
            panic!(
                "no compaction from the old frozen prefix, {count} requests\n{}",
                context.join("\n")
            );
        });
        assert_eq!(attempted, 2, "no normal request was sent with the gap");
        assert!(requests[attempted].to_string().contains(old_marker));
        assert!(!requests[attempted].to_string().contains("End of current result."));
        let sent = requests[attempted + 1..].iter().find(|request| !asks_compaction(request));
        if summary {
            let sent = sent
                .expect("the successful checkpoint allows an ordinary continuation")
                .to_string();
            assert!(sent.contains("The prior response is summarized here"));
            assert!(sent.contains("BURST-RESULT"));
            assert_eq!(super::l12_4_compaction::completed(&runtime, session), 1);
        } else {
            assert_eq!(super::l12_4_compaction::completed(&runtime, session), 0);
            assert!(sent.is_none(), "a rejected summary leaves the normal send blocked");
            assert!(matches!(outcome.run.state, AgentRunState::AwaitingContextRemediation { .. }));
        }
    }
}
