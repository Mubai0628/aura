use crate::graph_fixture::Fixture;
use aura_agent_collaboration::{
    AgentGraphAction, PeerDispatchInput, PeerOrigin, PeerSender, PeerSource, dispatch,
};
use aura_domain_common::{
    AgentRunId, ContentHash, EventId, Revision, SessionId, TaskLineageId, ToolCallId, TurnTaskId,
    UtcTimestamp,
};
use aura_persistence_core::StoreResult;
use rusqlite::Connection;

fn prepare(f: &mut Fixture) -> (PeerSender, PeerOrigin, PeerDispatchInput) {
    let [a, b, c, _] = f.roots;
    f.merge(a, b);
    f.merge(a, c);
    for target in [b, c] {
        f.change(
            a,
            f.read(a).expected(),
            AgentGraphAction::Connect { source_session_id: a, target_session_id: target },
        )
        .expect("edge");
    }
    let graph = f.read(a);
    let root_event_id = EventId::new();
    (
        PeerSender {
            session_id: a,
            turn_task_id: TurnTaskId::new(),
            tool_call_id: ToolCallId::new(),
        },
        PeerOrigin {
            workspace_id: f.access.workspace_id,
            session_id: a,
            lineage_id: TaskLineageId::new(),
            root_event_id,
            lineage_revision: Revision::INITIAL,
            sources: vec![PeerSource {
                event_id: root_event_id,
                content_hash: ContentHash::blake3(b"user input"),
                excerpt: "user input".into(),
            }],
        },
        PeerDispatchInput {
            graph_id: graph.graph_id.expect("graph"),
            graph_revision: graph.revision,
            targets: vec![b],
            task: "summarize".into(),
        },
    )
}

fn accept(
    db: &mut Connection,
    sender: PeerSender,
    input: &PeerDispatchInput,
    source: &PeerOrigin,
) -> StoreResult<Vec<EventId>> {
    let tx = db.transaction()?;
    let ids = dispatch::accept(
        &tx,
        sender,
        input,
        aura_agent_collaboration::PeerDeliveryKind::Dispatch,
        source,
        UtcTimestamp::now(),
    )?;
    tx.commit()?;
    Ok(ids)
}

#[test]
fn duplicate_call_reconciles_once_after_reopen_even_if_the_edge_was_removed() {
    let tmp = tempfile::tempdir().expect("dir");
    let path = tmp.path().join("peers.db");
    let mut f = Fixture::using(Connection::open(&path).expect("create"));
    let (sender, source, input) = prepare(&mut f);
    let first = accept(&mut f.db, sender, &input, &source).expect("first acceptance");
    let target = input.targets[0];
    let bound = AgentRunId::new();
    let d = dispatch::load(&f.db, first[0]).expect("query before claim").expect("delivery");
    let claim = f.db.transaction().expect("transaction");
    dispatch::claim(&claim, &d, TurnTaskId::new(), bound).expect("claim once");
    claim.commit().expect("claim committed");
    f.change(
        sender.session_id,
        f.read(sender.session_id).expected(),
        AgentGraphAction::Disconnect {
            source_session_id: sender.session_id,
            target_session_id: target,
        },
    )
    .expect("remove edge");
    drop(f);
    let mut db = Connection::open(&path).expect("reopen");
    // Simulate the caller not receiving the first reply. Consult the original call, not the body.
    assert_eq!(
        dispatch::by_call(&db, sender.session_id, sender.tool_call_id).expect("reconcile"),
        first
    );
    assert!(
        dispatch::by_call(&db, SessionId::new(), sender.tool_call_id)
            .expect("different session")
            .is_empty()
    );
    assert_eq!(accept(&mut db, sender, &input, &source).expect("duplicate"), first);
    assert_eq!(dispatch::budget(&db, &source).expect("budget"), (1, 32));
    assert_eq!(dispatch::pending(&db, target).expect("outbox"), None);
    assert_eq!(
        dispatch::load(&db, first[0]).expect("delivery query").expect("delivery").target_run,
        Some(bound)
    );
    let mut alternate = input.clone();
    alternate.task.push_str(" forge different task");
    assert!(accept(&mut db, sender, &alternate, &source).is_err());
    let distinct = PeerSender { tool_call_id: ToolCallId::new(), ..sender };
    assert!(
        accept(&mut db, distinct, &input, &source)
            .expect_err("stale revision")
            .to_string()
            .contains("peers.graph_changed")
    );
}

#[test]
fn manual_peer_review_survives_policy_change_and_preserves_the_original() {
    let mut f = Fixture::new();
    let (sender, origin, input) = prepare(&mut f);
    let target = input.targets[0];
    let tx = f.db.transaction().expect("isolated fixture state");
    dispatch::review::set_policy(&tx, 0, true).expect("isolated fixture state");
    tx.commit().expect("isolated fixture state");
    let id = accept(&mut f.db, sender, &input, &origin).expect("isolated fixture state")[0];
    let tx = f.db.transaction().expect("isolated fixture state");
    let item =
        dispatch::load(&tx, id).expect("isolated fixture state").expect("isolated fixture state");
    assert!(dispatch::review::waiting(&tx, id).expect("isolated fixture state"));
    assert_eq!(dispatch::pending(&tx, target).expect("isolated fixture state"), None);
    dispatch::review::set_policy(&tx, 1, false).expect("isolated fixture state");
    assert!(
        dispatch::review::release(&tx, &item, sender.session_id, None, UtcTimestamp::now())
            .is_err()
    );
    assert_eq!(dispatch::pending(&tx, target).expect("isolated fixture state"), None);
    dispatch::review::release(&tx, &item, target, Some("approved revision"), UtcTimestamp::now())
        .expect("isolated fixture state");
    assert_eq!(
        dispatch::review::revised_text(&tx, id).expect("isolated fixture state").as_deref(),
        Some("approved revision")
    );
    assert_eq!(dispatch::pending(&tx, target).expect("isolated fixture state"), Some(id));
    assert_eq!(
        dispatch::load(&tx, id)
            .expect("isolated fixture state")
            .expect("isolated fixture state")
            .input,
        input
    );
    tx.commit().expect("isolated fixture state");
    assert_eq!(dispatch::budget(&f.db, &origin).expect("isolated fixture state"), (1, 32));
}

#[test]
fn global_review_holds_every_new_target_and_keeps_existing_deliveries_on_their_original_policy() {
    let mut f = Fixture::new();
    let (sender, origin, mut input) = prepare(&mut f);
    input.targets.push(f.roots[2]);
    let tx = f.db.transaction().expect("settings transaction");
    dispatch::review::set_policy(&tx, 0, true).expect("enable review");
    tx.commit().expect("enable committed");
    let held = accept(&mut f.db, sender, &input, &origin).expect("two targets");
    assert_eq!(held.len(), 2);
    for id in held.iter().copied() {
        assert!(dispatch::review::waiting(&f.db, id).expect("held by the global policy"));
    }

    let tx = f.db.transaction().expect("settings transaction");
    dispatch::review::set_policy(&tx, 1, false).expect("disable review");
    tx.commit().expect("disable committed");
    let distinct = PeerSender { tool_call_id: ToolCallId::new(), ..sender };
    let automatic = accept(&mut f.db, distinct, &input, &origin).expect("new automatic deliveries");
    for id in automatic {
        assert!(!dispatch::review::waiting(&f.db, id).expect("new delivery needs no review"));
    }
    for id in held {
        assert!(dispatch::review::waiting(&f.db, id).expect("original review remains"));
    }
    assert_eq!(dispatch::budget(&f.db, &origin).expect("same run"), (4, 32));
}

#[test]
fn failed_atomic_fanout_rolls_back_then_the_thirty_third_target_is_refused() {
    let mut f = Fixture::new();
    let (sender, source, input) = prepare(&mut f);
    let mut batch = input.clone();
    batch.targets.push(f.roots[2]);
    // Reject the second outbox insert, after the earlier receipt and quota writes.
    f.db.execute_batch("CREATE TRIGGER fixture_reject BEFORE INSERT ON agent_collaboration_outbox WHEN (SELECT count(*) FROM agent_collaboration_outbox)=1 BEGIN SELECT RAISE(ABORT,'fixture'); END;").expect("fault");
    assert!(accept(&mut f.db, sender, &batch, &source).is_err());
    assert_eq!(dispatch::budget(&f.db, &source).expect("rollback"), (0, 32));
    assert!(dispatch::receivers(&f.db).expect("empty outbox").is_empty());
    assert!(
        dispatch::by_call(&f.db, sender.session_id, sender.tool_call_id)
            .expect("empty receipts")
            .is_empty()
    );
    f.db.execute_batch("DROP TRIGGER fixture_reject").expect("remove fault");
    for _ in 0..31 {
        let distinct = PeerSender { tool_call_id: ToolCallId::new(), ..sender };
        accept(&mut f.db, distinct, &input, &source)
            .expect("same text may be intentionally sent again");
    }
    let rejected =
        accept(&mut f.db, sender, &batch, &source).expect_err("batch not partly accepted");
    assert!(rejected.to_string().contains("peers.budget_exhausted"));
    assert_eq!(dispatch::budget(&f.db, &source).expect("no partial charge"), (31, 32));
    assert!(dispatch::pending(&f.db, f.roots[2]).expect("no partial delivery").is_none());
    accept(&mut f.db, sender, &input, &source).expect("32nd");
    assert!(
        accept(
            &mut f.db,
            PeerSender { tool_call_id: ToolCallId::new(), ..sender },
            &input,
            &source
        )
        .expect_err("33rd")
        .to_string()
        .contains("peers.budget_exhausted")
    );
    assert_eq!(dispatch::budget(&f.db, &source).expect("charged precisely"), (32, 32));
    let tx = f.db.transaction().expect("isolated fixture state");
    dispatch::controls::increase(&tx, &source, 0, 34).expect("isolated fixture state");
    assert!(dispatch::controls::increase(&tx, &source, 0, 40).is_err(), "stale revision");
    dispatch::controls::set_default(&tx, 0, 50).expect("isolated fixture state");
    tx.commit().expect("isolated fixture state");
    assert_eq!(dispatch::budget(&f.db, &source).expect("isolated fixture state"), (32, 34));
    let id = accept(
        &mut f.db,
        PeerSender { tool_call_id: ToolCallId::new(), ..sender },
        &input,
        &source,
    )
    .expect("isolated fixture state")[0];
    let tx = f.db.transaction().expect("isolated fixture state");
    let stopped = dispatch::controls::stop(&tx, &source, 1).expect("isolated fixture state");
    assert_eq!(stopped.len(), 33);
    for d in stopped {
        assert!(dispatch::controls::cancel_queued(&tx, &d).expect("isolated fixture state"));
    }
    tx.commit().expect("isolated fixture state");
    assert!(
        dispatch::load(&f.db, id)
            .expect("isolated fixture state")
            .expect("isolated fixture state")
            .cancelled
    );
    assert!(dispatch::receivers(&f.db).expect("isolated fixture state").is_empty());
    assert!(
        accept(
            &mut f.db,
            PeerSender { tool_call_id: ToolCallId::new(), ..sender },
            &input,
            &source
        )
        .expect_err("fixture operation must be rejected")
        .to_string()
        .contains("peers.run_stopped")
    );
    assert_eq!(dispatch::budget(&f.db, &source).expect("isolated fixture state"), (33, 34));
}
