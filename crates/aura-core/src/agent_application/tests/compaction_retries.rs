//! Rejected and interrupted summaries preserve the previous background without another generation.
use super::l10_15_compaction::{compactions, start};
use super::l10_19_compaction::{asks_compaction, before_the_third};
use super::l12_4_compaction::completed;
use super::*;

pub(super) async fn wait_for_first_attempt(
    host: &crate::r03_tests::Fixture,
    provider: &Provider,
) -> (crate::RuntimeState, SessionId) {
    let (runtime, session, model) = before_the_third(host, provider, 64_000).await;
    let handle = start(&runtime, session, model, "AURA-L1019-THIRD: answer at length").await;
    waiting::wait(&runtime, &handle, |v| {
        matches!(v.run.state, AgentRunState::AwaitingContextRemediation { .. })
            || v.turn.state.terminal()
    })
    .await;
    (runtime, session)
}

#[tokio::test]
async fn a_broken_summary_stream_is_not_sent_again() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::CompactInterruptedOnPrefix).await;
    let (runtime, session) = wait_for_first_attempt(&host, &provider).await;
    assert_eq!((compactions(&runtime, session), completed(&runtime, session)), (1, 0));
    assert_eq!(
        provider.requests.lock().expect("requests").iter().filter(|r| asks_compaction(r)).count(),
        1
    );
}
