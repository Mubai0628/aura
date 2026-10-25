use super::*;

#[tokio::test]
async fn the_real_core_port_hands_observation_to_the_worker_without_a_global_ticket_table() {
    let case = Case::new();
    let index = case.port.activity_index.clone();
    let lease = Arc::new(crate::agent_activity::AgentActivityLease::new(index.clone()));
    let published_index = index.clone();
    let callback: aura_fs_tools::FileExecutionObserver = Arc::new(move |_| {
        let record = crate::agent_activity::tests_support::synthetic_record();
        let published = crate::agent_activity::PublishedActivity {
            id: record.id,
            generation: record.generation,
        };
        assert!(published_index.publish(record).was_recorded());
        lease.started(published);
        Box::new(crate::agent_activity::execution::EndOwner(Arc::clone(&lease)))
    });
    let mut register = || Ok(());
    case.port.claim(&case.request, &mut register).expect("actual bound attempt claim");
    let worker = crate::agent_activity::execution::scope_for_test(callback, async {
        case.port.execution_observer(&case.request).expect("the actual Core port captured the sink")
    })
    .await;
    assert_eq!(index.len(), 0, "claim and dispatch return are not a real process start");
    worker.started();
    assert_eq!(index.len(), 1, "the sink survives returning from dispatch's task-local scope");
    worker.foreground_finished();
    assert_eq!(index.len(), 0, "actual foreground completion owns the end");
    worker.worker_finished();
}
