use super::l10_15_compaction::{compactions, start};
use super::l10_19_compaction::{asks_compaction, resize, used};
use super::l12_4_compaction::completed;
use super::*;

/// Quotes in each of the two long messages. The step's request carries them as they are; the
/// isolated request carries the history as JSON, where each takes two bytes, so the two messages
/// together are more than it can hold, and each alone fits.
const QUOTES: usize = 120_000;

#[tokio::test]
async fn two_long_messages_are_covered_by_one_request_with_the_original_tools() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::CompactOnPrefix).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    // Room for both long messages until the window is set for the third turn.
    resize(&runtime, model, 1_000_000, 64_000).await;
    let done = |v: &AgentRunProjection| matches!(v.turn.state, TurnTaskState::Completed { .. });
    for label in ["AURA-PARTS-FIRST", "AURA-PARTS-SECOND"] {
        let long = format!("{label}: keep this in mind. {}", "\"".repeat(QUOTES));
        let turn = start(&runtime, session, model, &long).await;
        waiting::wait(&runtime, &turn, done).await;
    }
    // As in `before_the_third`: the third turn's third step misses the room it must leave for the
    // output by about 6,000 tokens, while the step's request with the ask leaves the summary room.
    let continuing = used(&runtime, session, 1).await + 400 + 20_034;
    let usable = continuing + 26_000;
    let window = (usable * 100).div_ceil(95).max(usable + 4_096);
    resize(&runtime, model, window, 64_000).await;
    let third = start(&runtime, session, model, "AURA-PARTS-THIRD: answer at length").await;
    waiting::wait(&runtime, &third, done).await;

    let requests = provider.requests.lock().expect("provider requests").clone();
    let tools = |request: &Value| request["tools"].as_array().map_or(0, Vec::len);
    let asked = requests.iter().filter(|request| asks_compaction(request)).collect::<Vec<_>>();
    assert_eq!(asked.len(), 1, "one summary for the whole frozen history");
    assert!(tools(asked[0]) > 0);
    let body = asked[0].to_string();
    assert!(body.contains("AURA-PARTS-FIRST") && body.contains("AURA-PARTS-SECOND"));
    assert_eq!(compactions(&runtime, session), 1);
    assert_eq!(completed(&runtime, session), 1);
}

#[tokio::test]
async fn context_continuity_preserves_observed_history_after_a_file_changes() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::CompactOnPrefix).await;
    let (runtime, session, model) =
        super::l10_19_compaction::before_the_third(&host, &provider, 64_000).await;
    let third = start(&runtime, session, model, "AURA-L1019-THIRD: answer at length").await;
    let done = |v: &AgentRunProjection| matches!(v.turn.state, TurnTaskState::Completed { .. });
    waiting::wait(&runtime, &third, done).await;
    let file = host.root.path().join("waiting-workspace").join("version-evidence.txt");
    std::fs::write(&file, b"verified file").expect("valid compaction fixture and receipt");
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    // Install a version-bound historical receipt in the isolated store, then exercise the actual
    // request assembler and file identity check for both the matching and changed file.
    {
        let mut store = services.broker.lock().expect("valid compaction fixture and receipt");
        store
            .database
            .unit_of_work(|tx| -> aura_persistence_core::StoreResult<()> {
                use aura_persistence_v2::context::compaction;
                let mut record =
                    compaction::latest(tx, session)?.expect("valid compaction fixture and receipt");
                record.background_sources = vec![compaction::BackgroundSource::File {
                    path: aura_domain_common::WorkspacePath::parse("version-evidence.txt")
                        .expect("valid compaction fixture and receipt"),
                    version: aura_domain_common::ContentHash::sha256(b"verified file"),
                }];
                tx.execute(
                    "UPDATE context_compactions SET payload=?1 WHERE id=?2",
                    [serde_json::to_string(&record)?, record.id.to_string()],
                )?;
                Ok(())
            })
            .expect("valid compaction fixture and receipt");
    }
    resize(&runtime, model, 1_000_000, 64_000).await;
    let moved = file.with_file_name("relocated-evidence.txt");
    for change in ["unchanged", "renamed", "new version", "deleted"] {
        match change {
            "renamed" => std::fs::rename(&file, &moved).expect("rename fixture file"),
            "new version" => {
                std::fs::write(&file, b"replacement contents").expect("replace observed file")
            }
            "deleted" => std::fs::remove_file(&file).expect("remove fixture file"),
            _ => {}
        }
        let r = start(&runtime, session, model, "Report the current stage briefly.").await;
        waiting::wait(&runtime, &r, done).await;
        let requests = provider.requests.lock().expect("provider request log");
        let request = requests.last().expect("provider received a continuation");
        let messages = request["messages"].as_array().expect("Chat messages");
        assert!(
            messages.iter().any(|message| {
                message["role"] == "assistant"
                    && message["content"]
                        .as_str()
                        .is_some_and(|text| text.contains("The prior response is summarized here"))
            }),
            "a fresh continuation carries the saved summary as conversation history"
        );
        assert!(
            messages.iter().filter(|message| message["role"] == "system").all(|message| {
                !message["content"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("The prior response is summarized here")
            }),
            "a saved summary must not become a system instruction in a fresh request"
        );
        assert!(
            !request.to_string().contains(&"a detail kept ".repeat(100)),
            "covered raw history was materialized in the request"
        );
    }
    assert_eq!(compactions(&runtime, session), 1);
    {
        let mut db = services.broker.lock().expect("event journal");
        db.database.unit_of_work(|tx| -> aura_persistence_core::StoreResult<()> {
            tx.execute("INSERT INTO context_compactions(id,session_id,state,revision,payload) VALUES(?1,?2,'completed',1,'{}')",
                [aura_domain_common::ContextProjectionSnapshotId::new().to_string(),session.to_string()])?;
            Ok(())
        }).expect("insert malformed checkpoint fixture");
    }
    let r = start(&runtime, session, model, "Continue after reopening the history.").await;
    waiting::wait(&runtime, &r, done).await;
    let sent = provider
        .requests
        .lock()
        .expect("provider request log")
        .last()
        .expect("provider received the resumed request")
        .to_string();
    assert!(sent.contains("The prior response is summarized here"));
    assert!(sent.contains("An earlier history checkpoint is unavailable"));
    assert!(!sent.contains(&"a detail kept ".repeat(100)));
}

#[tokio::test]
async fn context_continuity_a_smaller_window_continues_from_bounded_history() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::CompactOnPrefix).await;
    let (runtime, session, model) =
        super::l10_19_compaction::before_the_third(&host, &provider, 64_000).await;
    let fixed = used(&runtime, session, 0).await;
    let history = used(&runtime, session, 1).await;
    assert!(history > fixed);
    resize(&runtime, model, fixed + (history - fixed) / 2 + 4_096, 1_024).await;
    let turn = start(&runtime, session, model, "Continue with the current history.").await;
    waiting::wait(&runtime, &turn, |view| {
        view.turn.state.terminal()
            || matches!(view.run.state, AgentRunState::AwaitingContextRemediation { .. })
    })
    .await;
    let requests = provider.requests.lock().expect("provider request log");
    let wire = requests.last().expect("provider received the bounded request").to_string();
    assert!(wire.contains("Continue with the current history."));
    assert!(wire.contains("history.read"), "the omitted material remains reachable");
    assert!(!wire.contains("AURA-L1019-LONG"));
}

#[tokio::test]
async fn context_continuity_recovers_a_large_paste_from_the_authenticated_original() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Plain).await;
    let (runtime, session, model) =
        waiting::prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    resize(&runtime, model, 64_000, 4096).await;
    let turn = start(
        &runtime,
        session,
        model,
        &format!("Opening instruction.\n{}\nFinal corrected title.", "长资料".repeat(27_000)),
    )
    .await;
    waiting::wait(&runtime, &turn, |view| {
        view.turn.state.terminal()
            || matches!(view.run.state, AgentRunState::AwaitingContextRemediation { .. })
    })
    .await;
    let requests = provider.requests.lock().expect("provider request log");
    let wire =
        requests.last().expect("the real transport received the verified excerpt").to_string();
    assert!(
        wire.contains("Opening instruction.")
            && wire.contains("Final corrected title.")
            && wire.contains("bytes omitted"),
        "both original edges must reach the provider"
    );
    assert!(!wire.contains(&"长资料".repeat(10_000)));
    drop(requests);
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let store = services.broker.lock().expect("event journal");
    let db = store.database.connection();
    let (step, embedded_flag, separate_flag): (String, Option<String>, bool) = db
        .query_row(
            "SELECT s.id,json_type(e.payload,'$.reduction.history_on_demand'),EXISTS(
               SELECT 1 FROM events h WHERE h.owner=s.id
               AND h.kind='agent.prompt_inspector.history_on_demand' AND h.payload='true')
             FROM model_steps s
             JOIN events e ON e.owner=s.id AND e.kind='agent.prompt_inspector'
             WHERE s.agent_run_id=?1 ORDER BY s.ordinal LIMIT 1",
            [turn.agent_run_id.to_string()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("frozen inspector and version-one extension event");
    assert_eq!(embedded_flag, None, "strict older readers require the old projection shape");
    assert!(separate_flag, "the on-demand marker must also survive");
    let inspection = aura_persistence_v2::agent::inspector::read(
        db,
        &AgentInspectorRequest {
            session_id: session,
            model_step_id: step.parse().expect("step ID"),
        },
    )
    .expect("read the frozen inspection");
    assert!(inspection.projection.reduction.expect("bounded step").history_on_demand);
}
