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
async fn revoking_a_file_version_removes_its_compacted_background_and_restores_original_history() {
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
    for valid in [true, false] {
        if !valid {
            std::fs::write(&file, b"replaced").expect("valid compaction fixture and receipt");
        }
        let r = start(&runtime, session, model, "Report the current stage briefly.").await;
        waiting::wait(&runtime, &r, done).await;
        let sent = provider
            .requests
            .lock()
            .expect("valid compaction fixture and receipt")
            .last()
            .expect("valid compaction fixture and receipt")
            .to_string();
        assert_eq!(sent.contains("The prior response is summarized here"), valid);
        if !valid {
            assert!(sent.contains("AURA-L1019-LONG"));
        }
    }
}
