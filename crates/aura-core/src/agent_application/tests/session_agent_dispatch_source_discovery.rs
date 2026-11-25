use super::*;
use aura_agent_collaboration::{
    PeerDeliveryKind, PeerDispatchInput, PeerOrigin, PeerSender, PeerSource, dispatch, graph,
};
use aura_domain_common::{TaskLineageId, ToolCallId};
use aura_persistence_core::PersistenceCoreError as Error;

#[tokio::test]
async fn session_agent_dispatch_discovers_the_seventeenth_source_without_an_event_id() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::PeerDispatch { disconnect: false }).await;
    let (runtime, a, model) = Box::pin(prepare(&host, &provider)).await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("ready") };
    let workspace = services
        .persistence
        .workspace_session_projection()
        .expect("projection")
        .sessions
        .into_iter()
        .find(|session| session.session_id == a)
        .expect("A")
        .workspace_id;
    let b = Box::pin(create_session(&runtime, workspace, PermissionMode::ReadOnly)).await;
    Box::pin(connect(&runtime, a, b)).await;
    let source = Box::pin(begin(&runtime, a, model, "B4 user authorizes a summary for B")).await;
    Box::pin(wait(&runtime, &source, |projection| {
        matches!(projection.turn.state, TurnTaskState::Completed { .. })
    }))
    .await;
    let acceptance = format!("验收串-{}", EventId::new());
    let app = services.agent_required();
    let session_id = a;
    let target = b;
    let old_run = source.agent_run_id;
    let seed_acceptance = acceptance.clone();
    let (delivery, source_ids) = app
        .store
        .write(StoreStep::operation("test.peer.source.seed"), move |journal| {
            journal.database.unit_of_work(|tx| {
                let old_run = aura_persistence_v2::agent::turns::load_run(tx, old_run)?;
                let old_turn = aura_persistence_v2::agent::turns::load(tx, old_run.turn_task_id)?;
                let old = aura_persistence_v2::agent::lineage::load(tx, old_turn.lineage_id)?;
                let now = UtcTimestamp::now();
                let mut sources = Vec::new();
                let mut ids = Vec::new();
                for index in 0..17 {
                    let event = EventId::new();
                    let text = if index == 16 {
                        format!("{} {seed_acceptance}", "末页长文 ".repeat(300))
                    } else {
                        format!("第 {} 条来源", index + 1)
                    };
                    let message = aura_agent_domain::OriginalUserMessage::new(
                        event,
                        session_id,
                        text.clone(),
                        aura_agent_domain::UserMessageOrigin::UserInput,
                        now,
                    )
                    .map_err(|_| Error::Invalid("seed user message"))?;
                    aura_persistence_v2::agent::session::add_user_message(tx, &message)?;
                    sources.push(aura_agent_domain::TaskLineageSource {
                        user_event_id: event,
                        source_sequence: index + 1,
                        content_hash: message.content_hash.clone(),
                        immutable_preview: aura_agent_domain::source_preview(&text),
                        related_ranges: Vec::new(),
                        membership: aura_agent_domain::SourceMembership::Active,
                        attached_by_turn_task_id: old.last_active_turn_task_id,
                        attached_at: now,
                    });
                    ids.push(event);
                }
                let mut fresh = old.clone();
                fresh.lineage_id = TaskLineageId::new();
                fresh.root_user_event_id = ids[0];
                fresh.state = aura_agent_domain::TaskLineageState::Active;
                fresh.revision = Revision::INITIAL;
                fresh.created_by_turn_task_id = old.last_active_turn_task_id;
                fresh.last_active_turn_task_id = old.last_active_turn_task_id;
                fresh.sources = vec![sources[0].clone()];
                fresh.relations.clear();
                fresh.source_recheck = Default::default();
                fresh.created_at = now;
                fresh.updated_at = now;
                fresh.completed_at = None;
                fresh.derived_from_lineage_id = Some(old.lineage_id);
                aura_persistence_v2::agent::lineage::insert(tx, &fresh)
                    .map_err(|_| Error::Invalid("fresh lineage insert"))?;
                let mut next = fresh.clone();
                next.revision = Revision::new(2);
                next.sources = sources.clone();
                next.updated_at = now;
                aura_persistence_v2::agent::lineage::save(tx, &fresh, &next)
                    .map_err(|_| Error::Invalid("fresh lineage save"))?;
                let preview = sources
                    .iter()
                    .take(16)
                    .map(|source| PeerSource {
                        event_id: source.user_event_id,
                        content_hash: source.content_hash.clone(),
                        excerpt: source.immutable_preview.clone(),
                    })
                    .collect();
                let origin = PeerOrigin {
                    workspace_id: workspace,
                    session_id,
                    lineage_id: fresh.lineage_id,
                    root_event_id: ids[0],
                    lineage_revision: next.revision,
                    sources: preview,
                };
                let graph = graph::load(tx, session_id, workspace)
                    .map_err(|_| Error::Invalid("graph load"))?;
                let input = PeerDispatchInput {
                    graph_id: graph.graph_id.ok_or(Error::Invalid("graph id"))?,
                    graph_revision: graph.revision,
                    targets: vec![target],
                    task: "B5 source discovery".into(),
                };
                let sender = PeerSender {
                    session_id,
                    turn_task_id: old.last_active_turn_task_id,
                    tool_call_id: ToolCallId::new(),
                };
                let deliveries =
                    dispatch::accept(tx, sender, &input, PeerDeliveryKind::Dispatch, &origin, now)
                        .map_err(|_| Error::Invalid("dispatch accept"))?;
                Ok((deliveries[0], ids))
            })
        })
        .await
        .expect("seed sources and delivery");
    let delivery_origin = services
        .agent_required()
        .store
        .read(StoreStep::operation("test.peer.source.origin"), move |db| {
            Ok(aura_agent_collaboration::dispatch::load(db, delivery)?.expect("delivery").origin)
        })
        .await
        .expect("origin");
    assert_eq!(delivery_origin.sources.len(), 16, "the delivery preview stays bounded");
    assert!(!delivery_origin.sources.iter().any(|source| source.event_id == source_ids[16]));
    let binding = services.persistence.session_model_binding_state(b).expect("binding");
    call(
        &runtime,
        "models.session.set_binding",
        json!({"session_id":b,"model_profile_id":model,"expected_revision":binding.revision}),
    )
    .await;
    Box::pin(turn(&runtime, b, model, &format!("B5 discover {delivery}"))).await;
    let outcomes = tool_outcomes(&provider);
    let status = outcomes
        .iter()
        .map(|value| value["outcome"]["payload"]["value"].clone())
        .find(|value| value["deliveries"][0]["sources"].is_array())
        .expect("status source catalog");
    assert_eq!(status["deliveries"][0]["sources"].as_array().expect("source catalog").len(), 17);
    let read = outcomes
        .iter()
        .map(|value| value["outcome"]["payload"]["value"].clone())
        .rfind(|value| {
            value["delivery_id"] == json!(delivery) && value["event_id"] == json!(source_ids[16])
        })
        .expect("B discovered and read the seventeenth source");
    assert!(read["text"].as_str().is_some_and(|text| text.contains(&acceptance)));
}
