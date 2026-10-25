//! Publication re-checks the live approval even when no managed stop reached the transfer.
use super::*;
use std::time::Duration;

struct Release(aura_persistence::barrier::Barrier);
impl Drop for Release {
    fn drop(&mut self) {
        self.0.release_all();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn s07_n11_a_withdrawn_network_approval_is_rechecked_before_publication() {
    let server = Server::start().await;
    let f = fixture(PermissionMode::ReadOnly, &["http.download"]).await;
    let batch = f.admit(&[(
        "http.download",
        json!({"url":format!("{}/binary/10",server.url),"path":"late.bin"}),
    )]);
    let id = batch.batch.calls[0].tool_call_id;
    approve(&f, &batch.calls[&id], ApprovalDecisionKind::AllowSession);
    let engine = f.service().agent_execution_required();
    let hold = Release(engine.store.arm_hold("agent.http.download_publish", 20_000));
    let execution = Arc::clone(&engine);
    let commands = f.service().commands_required().clone();
    let (_parent, cancel) = tokio::sync::watch::channel(false);
    let task = tokio::spawn(async move { execution.execute_batch(batch, commands, cancel).await });
    let observed = hold.0.clone();
    assert!(
        tokio::task::spawn_blocking(move || observed.wait_for_arrivals(1, Duration::from_secs(10)))
            .await
            .expect("observe the held publication section")
    );
    // Withdraw only the network answer, directly in the Store: no managed stop runs, so the
    // publication's own live check is the one that must notice it.
    {
        let mut store = f.service().broker.lock().expect("approval store");
        let network = aura_persistence_v2::network_consents::list(
            store.database.connection(),
            f.turn.session_id,
        )
        .expect("network consent records");
        assert_eq!(network.len(), 1);
        store
            .database
            .unit_of_work(|tx| {
                aura_persistence_v2::network_consents::revoke(
                    tx,
                    f.turn.session_id,
                    network[0].grant_id,
                    network[0].revision,
                    UtcTimestamp::now(),
                )
            })
            .expect("withdraw the network approval");
    }
    hold.0.release_all();
    tokio::time::timeout(Duration::from_secs(15), task)
        .await
        .expect("the download finishes after release")
        .expect("the tool batch does not panic")
        .expect("the batch has a paired terminal response");
    assert_eq!(server.count(), 1, "the transfer itself had been authorized");
    assert!(!f.root.join("late.bin").exists(), "a withdrawn approval never publishes the file");
    let store = f.service().broker.lock().expect("download result");
    let call = aura_persistence_v2::tool::execution::call(store.database.connection(), id)
        .expect("download call");
    assert!(!matches!(call.result.expect("paired result").outcome, ToolResultOutcomeV1::Succeeded));
}
