#![allow(clippy::unwrap_used)]
//! Long compaction chains keep a consumed addition covered. A broken link,
//! wrong artifact hash, foreign session, or cycle is not coverage.
#[path = "support/agent.rs"]
mod support;

use aura_agent_domain::*;
use aura_context_domain::*;
use aura_domain_common::*;
use aura_model_domain::{AssistantContentBlock, NormalizedModelOutput, NormalizedStopReason};
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_persistence_v2::{
    V2Store,
    agent::{completion, history, lineage, session, turns},
    context,
};
use rusqlite::Transaction;
use support::Fixture;

fn answering_output(f: &Fixture) -> NormalizedModelOutput {
    let mut output = f.template.clone();
    output.model_step_id = f.step.model_step_id;
    output.tool_calls = vec![];
    output.refusal = None;
    output.stop_reason = NormalizedStopReason::EndTurn;
    output.assistant_blocks = vec![AssistantContentBlock {
        block_id: ModelContentBlockId::new(),
        text: "你好，我可以帮你做什么？".into(),
        provider_reference: None,
    }];
    output
}

fn settle(
    tx: &Transaction<'_>,
    f: &Fixture,
    output: &NormalizedModelOutput,
) -> StoreResult<Option<NaturalCompletionRecord>> {
    history::record_output(tx, output, None, f.at)?;
    let old = turns::load_run(tx, f.turn.agent_run_id)?;
    let mut next = old.clone();
    next.revision = Revision::new(old.revision.value() + 1);
    next.next_model_ordinal = old.next_model_ordinal + 1;
    next.updated_at = f.at;
    next.state = AgentRunState::PreparingContext;
    turns::save_run(tx, &old, &next)?;
    completion::natural_from_response(tx, output, f.at)
}

fn attach_addition(store: &mut V2Store, f: &Fixture, text: &str) -> EventId {
    let addition = OriginalUserMessage::new(
        EventId::new(),
        f.turn.session_id,
        text.to_owned(),
        UserMessageOrigin::UserInput,
        f.at,
    )
    .unwrap();
    let event_id = addition.event_id;
    store
        .database
        .unit_of_work(|tx| -> StoreResult<()> {
            session::add_user_message(tx, &addition)?;
            let old = lineage::load(tx, f.turn.lineage_id)?;
            let mut next = old.clone();
            next.attach_source(&addition, f.turn.turn_task_id, vec![], old.revision, f.at)
                .map_err(|_| Error::Conflict)?;
            lineage::save(tx, &old, &next)
        })
        .unwrap();
    event_id
}

#[derive(Clone)]
struct BuiltHop {
    compaction: context::compaction::Compaction,
    result: ContextReadCompactionResult,
    artifact_id: ArtifactId,
}

fn event_hop(f: &Fixture, event_id: EventId, text: &str, session_id: SessionId) -> BuiltHop {
    let id = ContextProjectionSnapshotId::new();
    let artifact_id = ArtifactId::new();
    let anchors = CompactionAnchorManifest {
        manifest_hash: ContentHash::sha256(b"event-manifest"),
        anchors: vec![CompactionAnchor {
            anchor: "a000001".into(),
            kind: CompactionAnchorKind::Event,
            source_ref: ContextSourceRef::Event { event_id },
            source_hash: user_history_anchor_hash(event_id, text).unwrap(),
            preview: "stored addition".into(),
        }],
    };
    let span = CompactionSourceSpan {
        start_sequence: SessionSequence::new(1),
        end_sequence: SessionSequence::new(2),
        source_refs: vec![ContextSourceRef::Event { event_id }],
    };
    let result = ContextReadCompactionResult {
        source_context_snapshot_id: id,
        sequence: 1,
        historical_snapshot: "history".into(),
        active_turn_summary: "active".into(),
        structured_compaction: "structured".into(),
        source_span: span.clone(),
        anchor_manifest: anchors.clone(),
    };
    BuiltHop {
        compaction: record(f, session_id, vec![event_id], &result, artifact_id),
        result,
        artifact_id,
    }
}

fn artifact_hop(
    f: &Fixture,
    event_id: EventId,
    sequence: u32,
    previous: &BuiltHop,
    source_hash: ContentHash,
    artifact_id: ArtifactId,
) -> BuiltHop {
    let id = ContextProjectionSnapshotId::new();
    let linked = previous.artifact_id;
    let anchors = CompactionAnchorManifest {
        manifest_hash: ContentHash::sha256(sequence.to_string().as_bytes()),
        anchors: vec![CompactionAnchor {
            anchor: "a000001".into(),
            kind: CompactionAnchorKind::Artifact,
            source_ref: ContextSourceRef::Artifact { artifact_id: linked },
            source_hash,
            preview: "previous verified compaction".into(),
        }],
    };
    let span = CompactionSourceSpan {
        start_sequence: SessionSequence::new(sequence.into()),
        end_sequence: SessionSequence::new(sequence.into()),
        source_refs: vec![ContextSourceRef::Artifact { artifact_id: linked }],
    };
    let result = ContextReadCompactionResult {
        source_context_snapshot_id: id,
        sequence,
        historical_snapshot: format!("history-{sequence}"),
        active_turn_summary: "active".into(),
        structured_compaction: "structured".into(),
        source_span: span.clone(),
        anchor_manifest: anchors.clone(),
    };
    BuiltHop {
        compaction: record(f, f.turn.session_id, vec![event_id], &result, artifact_id),
        result,
        artifact_id,
    }
}

fn record(
    f: &Fixture,
    session_id: SessionId,
    excluded_event_ids: Vec<EventId>,
    result: &ContextReadCompactionResult,
    artifact_id: ArtifactId,
) -> context::compaction::Compaction {
    context::compaction::Compaction {
        id: result.source_context_snapshot_id,
        session_id,
        agent_run_id: f.turn.agent_run_id,
        source_step_id: f.step.model_step_id,
        sequence: result.sequence,
        revision: Revision::INITIAL,
        state: "completed".into(),
        source_span: result.source_span.clone(),
        anchors: result.anchor_manifest.clone(),
        source_artifact_id: ArtifactId::new(),
        source_hash: ContentHash::sha256(result.sequence.to_string().as_bytes()),
        excluded_event_ids,
        retained_recent_tokens: 0,
        retention_exceptions: vec![],
        attempts: vec![],
        result: Some(result.clone()),
        result_artifact_id: Some(artifact_id),
        created_at: f.at,
    }
}

fn insert_chain(store: &mut V2Store, f: &Fixture, hops: &[context::compaction::Compaction]) {
    let latest = hops.last().expect("chain").id;
    let execution = aura_persistence_v2::model::step_snapshot(
        store.database.connection(),
        f.step.model_step_id,
    )
    .unwrap();
    let p06 = aura_context_fixtures::p06_prepared_context_fixture().unwrap();
    let mut snapshot = p06.prepared.context.snapshot;
    snapshot.id = execution.context_snapshot_id;
    snapshot.session_id = f.turn.session_id;
    snapshot.turn_task_id = f.turn.turn_task_id;
    snapshot.agent_run_id = f.turn.agent_run_id;
    snapshot.model_step_id = f.step.model_step_id;
    let root = "Verify the requested work";
    snapshot.conversation_items = vec![aura_model_domain::LogicalConversationItem::User {
        source_event_id: f.turn.root_user_event_id,
        content: root.into(),
        content_hash: ContentHash::sha256(root.as_bytes()),
    }];
    let section = snapshot
        .sections
        .iter_mut()
        .find(|section| section.kind == ContextSectionKind::HistoricalSnapshotSummary)
        .unwrap();
    section.source_refs = vec![ContextSourceRef::Compaction { context_snapshot_id: latest }];
    let snapshot = snapshot.seal().unwrap();
    store
        .database
        .unit_of_work(|tx| -> StoreResult<()> {
            for hop in hops {
                let payload = aura_persistence_core::record::encode(hop)?;
                tx.execute(
                    "INSERT INTO context_compactions VALUES(?1,?2,'completed',1,?3)",
                    [hop.id.to_string(), f.turn.session_id.to_string(), payload],
                )?;
            }
            context::insert_snapshot(tx, &snapshot)
        })
        .unwrap();
}

fn completed(store: &mut V2Store, f: &Fixture) -> bool {
    let output = answering_output(f);
    store
        .database
        .unit_of_work(|tx| -> StoreResult<bool> { Ok(settle(tx, f, &output)?.is_some()) })
        .unwrap()
}

fn chain_hops(f: &Fixture, event_id: EventId, text: &str, count: usize) -> Vec<BuiltHop> {
    let mut previous = event_hop(f, event_id, text, f.turn.session_id);
    let mut hops = vec![previous.clone()];
    for sequence in 2..=u32::try_from(count).unwrap() {
        let hash = compaction_result_anchor_hash(&previous.result).unwrap();
        previous = artifact_hop(f, event_id, sequence, &previous, hash, ArtifactId::new());
        hops.push(previous.clone());
    }
    hops
}

fn valid_chain(
    f: &Fixture,
    event_id: EventId,
    text: &str,
    count: usize,
) -> Vec<context::compaction::Compaction> {
    chain_hops(f, event_id, text, count).into_iter().map(|hop| hop.compaction).collect()
}

fn direct_cover(f: &Fixture, event_id: EventId, text: &str, previous: &BuiltHop) -> BuiltHop {
    let id = ContextProjectionSnapshotId::new();
    let artifact_id = ArtifactId::new();
    let linked = previous.artifact_id;
    let anchors = CompactionAnchorManifest {
        manifest_hash: ContentHash::sha256(b"direct-cover"),
        anchors: vec![
            CompactionAnchor {
                anchor: "a000001".into(),
                kind: CompactionAnchorKind::Event,
                source_ref: ContextSourceRef::Event { event_id },
                source_hash: user_history_anchor_hash(event_id, text).unwrap(),
                preview: "stored addition".into(),
            },
            CompactionAnchor {
                anchor: "a000002".into(),
                kind: CompactionAnchorKind::Artifact,
                source_ref: ContextSourceRef::Artifact { artifact_id: linked },
                source_hash: compaction_result_anchor_hash(&previous.result).unwrap(),
                preview: "previous verified compaction".into(),
            },
        ],
    };
    let span = CompactionSourceSpan {
        start_sequence: SessionSequence::new(1),
        end_sequence: SessionSequence::new(2),
        source_refs: vec![
            ContextSourceRef::Event { event_id },
            ContextSourceRef::Artifact { artifact_id: linked },
        ],
    };
    let result = ContextReadCompactionResult {
        source_context_snapshot_id: id,
        sequence: previous.compaction.sequence + 1,
        historical_snapshot: "latest".into(),
        active_turn_summary: "active".into(),
        structured_compaction: "structured".into(),
        source_span: span.clone(),
        anchor_manifest: anchors.clone(),
    };
    BuiltHop {
        compaction: record(f, f.turn.session_id, vec![event_id], &result, artifact_id),
        result,
        artifact_id,
    }
}

fn completion_result(
    store: &mut V2Store,
    f: &Fixture,
) -> StoreResult<Option<NaturalCompletionRecord>> {
    let output = answering_output(f);
    store.database.unit_of_work(|tx| settle(tx, f, &output))
}

fn natural_after(count: usize) -> bool {
    let text = "Compacted consumed addition";
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    let event_id = attach_addition(&mut store, &f, text);
    insert_chain(&mut store, &f, &valid_chain(&f, event_id, text, count));
    completed(&mut store, &f)
}

#[test]
fn eight_nine_and_longer_compaction_chains_still_finish_naturally() {
    assert!(natural_after(8), "eight legal hops stay covered");
    assert!(natural_after(9), "a ninth legal hop is still covered");
    assert!(natural_after(16), "a longer legal chain stays covered");
}

#[test]
fn a_wrong_hash_missing_artifact_foreign_session_or_cycle_is_not_coverage() {
    let text = "Compacted consumed addition";
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    let event_id = attach_addition(&mut store, &f, text);
    let origin = event_hop(&f, event_id, text, f.turn.session_id);
    let wrong = artifact_hop(
        &f,
        event_id,
        2,
        &origin,
        ContentHash::sha256(b"not-the-result"),
        ArtifactId::new(),
    );
    insert_chain(&mut store, &f, &[origin.compaction.clone(), wrong.compaction]);
    assert!(!completed(&mut store, &f), "a wrong artifact hash is not coverage");

    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    let event_id = attach_addition(&mut store, &f, text);
    let origin = event_hop(&f, event_id, text, f.turn.session_id);
    let mut broken = artifact_hop(
        &f,
        event_id,
        2,
        &origin,
        compaction_result_anchor_hash(&origin.result).unwrap(),
        ArtifactId::new(),
    );
    let missing = ArtifactId::new();
    broken.compaction.anchors.anchors[0].source_ref =
        ContextSourceRef::Artifact { artifact_id: missing };
    broken.compaction.result.as_mut().unwrap().anchor_manifest.anchors[0].source_ref =
        ContextSourceRef::Artifact { artifact_id: missing };
    insert_chain(&mut store, &f, &[origin.compaction, broken.compaction]);
    assert!(!completed(&mut store, &f), "a missing previous artifact is not coverage");

    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    let event_id = attach_addition(&mut store, &f, text);
    let foreign = event_hop(&f, event_id, text, SessionId::new());
    insert_chain(&mut store, &f, &[foreign.compaction]);
    assert!(!completed(&mut store, &f), "another session's compaction is not coverage");

    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    let event_id = attach_addition(&mut store, &f, text);
    let origin = event_hop(&f, event_id, text, f.turn.session_id);
    let mut later = artifact_hop(
        &f,
        event_id,
        2,
        &origin,
        ContentHash::sha256(b"cycle-later"),
        ArtifactId::new(),
    );
    let mut earlier = origin.compaction;
    earlier.anchors.anchors[0] = CompactionAnchor {
        anchor: "a000001".into(),
        kind: CompactionAnchorKind::Artifact,
        source_ref: ContextSourceRef::Artifact { artifact_id: later.artifact_id },
        source_hash: ContentHash::sha256(b"cycle-earlier"),
        preview: "cycle".into(),
    };
    earlier.result.as_mut().unwrap().anchor_manifest = earlier.anchors.clone();
    earlier.result.as_mut().unwrap().source_span.source_refs =
        vec![ContextSourceRef::Artifact { artifact_id: later.artifact_id }];
    later.compaction.anchors.anchors[0].source_ref =
        ContextSourceRef::Artifact { artifact_id: earlier.result_artifact_id.unwrap() };
    later.compaction.result.as_mut().unwrap().anchor_manifest = later.compaction.anchors.clone();
    insert_chain(&mut store, &f, &[earlier, later.compaction]);
    assert!(!completed(&mut store, &f), "a cycle-shaped link is not coverage");
}

#[test]
fn a_direct_cover_ignores_unrelated_history_longer_than_the_budget() {
    let text = "Compacted consumed addition";
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    let event_id = attach_addition(&mut store, &f, text);
    let background = chain_hops(&f, EventId::new(), "older background", 256);
    let latest = direct_cover(&f, event_id, text, background.last().unwrap());
    let mut hops = background.into_iter().map(|hop| hop.compaction).collect::<Vec<_>>();
    hops.push(latest.compaction.clone());
    insert_chain(&mut store, &f, &hops);
    let loaded = aura_persistence_v2::agent::coverage_hops_for_events(
        store.database.connection(),
        &latest.compaction,
        &[event_id],
    )
    .unwrap();
    assert_eq!(loaded.len(), 1, "the covering summary is enough");
    assert!(
        aura_persistence_v2::agent::coverage_hops_for_events(
            store.database.connection(),
            &latest.compaction,
            &[],
        )
        .unwrap()
        .is_empty()
    );
    assert!(completion_result(&mut store, &f).unwrap().is_some());
}

#[test]
fn an_empty_demand_skips_a_long_chain_and_a_relevant_overrun_is_a_budget_error() {
    let text = "Compacted consumed addition";
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    let event_id = attach_addition(&mut store, &f, text);
    let hops = valid_chain(&f, event_id, text, 257);
    let latest = hops.last().unwrap().clone();
    insert_chain(&mut store, &f, &hops);
    assert!(
        aura_persistence_v2::agent::coverage_hops_for_events(
            store.database.connection(),
            &latest,
            &[],
        )
        .unwrap()
        .is_empty(),
        "no addition to prove does not read the ancestor chain"
    );
    let error = aura_persistence_v2::agent::coverage_hops_for_events(
        store.database.connection(),
        &latest,
        &[event_id],
    )
    .unwrap_err();
    assert!(matches!(error, Error::Invalid(COVERAGE_BUDGET_ERROR)));
    let error = completion_result(&mut store, &f).unwrap_err();
    assert!(matches!(error, Error::Invalid(COVERAGE_BUDGET_ERROR)));

    assert!(natural_after(COVERAGE_WALK_BUDGET), "a chain of the budget length stays covered");
}
