#![allow(clippy::unwrap_used)]
#[path = "support/agent.rs"]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::NormalizedModelOutput;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult};
use aura_persistence_v2::{
    V2Store,
    agent::{completion, history, lineage, session, turns, work},
    context,
    tool::execution as tools,
};
use rusqlite::Transaction;
use support::Fixture;

fn inputs(f: &Fixture) -> (TaskLineageReviewCompletionInput, NormalCompleteInput) {
    (
        TaskLineageReviewCompletionInput {
            lineage_id: f.turn.lineage_id,
            expected_lineage_revision: Revision::INITIAL,
            source_read_receipt_id: None,
            assessment: CompletionAssessment::FullySatisfied,
            reviewed_user_intents: vec![ReviewedUserIntent {
                description: "Requested work verified".into(),
                source_event_ids: vec![f.turn.root_user_event_id],
                result: ReviewedIntentResult::Satisfied,
                evidence_refs: vec![],
            }],
            evidence_refs: vec![],
        },
        NormalCompleteInput {
            lineage_id: f.turn.lineage_id,
            expected_lineage_revision: Revision::INITIAL,

            final_report: "Verified".into(),
            disclosed_limitations: vec![],
        },
    )
}
fn finish(
    tx: &Transaction<'_>,
    f: &Fixture,
    batch: &tools::BatchExecution,
    review: &TaskLineageReviewCompletionInput,
    input: &NormalCompleteInput,
) -> StoreResult<()> {
    let previous = lineage::load(tx, f.turn.lineage_id)?;
    let run = turns::load_run(tx, f.turn.agent_run_id)?;
    let mut next = previous.clone();
    let review = next
        .review_completion(
            review.clone(),
            None,
            CompletionReviewId::new(),
            &CompletionContext {
                turn: &f.turn,
                run: &run,
                model_step_id: f.step.model_step_id,
                at: f.at,
            },
        )
        .map_err(|_| Error::Conflict)?;
    lineage::save(tx, &previous, &next)?;
    lineage::save_review(tx, &review)?;
    f.success(tx, batch, 0)?;
    f.success(tx, batch, 1)?;
    history::complete_batch(tx, batch.batch.tool_batch_id, f.at)?;
    completion::normal(tx, f.step.model_step_id, batch.batch.tool_batch_id, input, true, f.at)?;
    Ok(())
}
#[test]
fn normal_can_finish_without_review_despite_unfinished_todo_and_strict_advisory_policy() {
    let temp = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&temp.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    let (_, input) = inputs(&f);
    store
        .database
        .unit_of_work(|tx| -> StoreResult<()> {
            let old = lineage::load(tx, f.turn.lineage_id)?;
            let mut next = old.clone();
            next.completion_policy = CompletionPolicy::StrictCompletionRequired {
                source_event_ids: vec![f.turn.root_user_event_id],
            };
            next.revision = Revision::new(old.revision.value() + 1);
            lineage::save(tx, &old, &next)?;
            let old_todo = work::todo(tx, f.turn.turn_task_id)?;
            let mut todo = old_todo.clone();
            todo.revision = Revision::new(todo.revision.value() + 1);
            todo.items.push(TodoItem {
                id: TodoItemId::new(),
                title: "Optional remaining check".into(),
                status: TodoStatus::Pending,
            });
            work::save_todo(tx, &old_todo, &todo, f.at)?;
            Ok(())
        })
        .unwrap();
    let mut input = input;
    input.expected_lineage_revision = Revision::new(2);
    let batch =
        f.admit(&mut store, vec![("normal.complete", serde_json::to_value(&input).unwrap())]);
    store
        .database
        .unit_of_work(|tx| -> StoreResult<()> {
            f.success(tx, &batch, 0)?;
            history::complete_batch(tx, batch.batch.tool_batch_id, f.at)?;
            let record = completion::normal(
                tx,
                f.step.model_step_id,
                batch.batch.tool_batch_id,
                &input,
                false,
                f.at,
            )?;
            assert_eq!(record.outcome, NormalCompletionOutcome::ModelDecision);
            assert!(record.review_id.is_none());
            Ok(())
        })
        .unwrap();
    assert!(
        turns::load(store.database.connection(), f.turn.turn_task_id).unwrap().state.terminal()
    );
    let view = aura_persistence_v2::agent::desktop::projection(
        store.database.connection(),
        &AgentHistoryRequest {
            session_id: f.turn.session_id,
            before_turn_task_id: None,
            within_turn: None,
            limit: 5,
        },
        RuntimeInstanceId::new(),
    )
    .unwrap();
    let page = aura_persistence_v2::agent::desktop_messages::text(
        store.database.connection(),
        &AgentMessageTextRequest {
            session_id: f.turn.session_id,
            source: AgentMessageTextSource::Completion { turn_task_id: f.turn.turn_task_id },
            offset: 0,
            limit: 64,
        },
    )
    .unwrap();
    let expected = ContentHash::blake3(input.final_report.as_bytes()).to_string();
    assert_eq!(page.sealed_revision.as_deref(), Some(expected.as_str()));
    assert_eq!(view.turns[0].completion.as_ref().unwrap().digest, page.sealed_revision);
}

#[test]
fn pending_addition_vetoes_completion_without_losing_the_input() {
    use aura_persistence_v2::agent::inbox;
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    let (_, input) = inputs(&f);
    let batch =
        f.admit(&mut store, vec![("normal.complete", serde_json::to_value(&input).unwrap())]);
    let pending = store
        .database
        .unit_of_work(|tx| {
            inbox::submit(
                tx,
                CommandInvocationId::new(),
                &AgentSubmitRequest::new(
                    f.turn.session_id,
                    "One additional requirement",
                    AgentInputLane::Addition { turn_task_id: f.turn.turn_task_id },
                ),
                f.at,
            )
        })
        .unwrap();
    assert!(
        store
            .database
            .unit_of_work(|tx| {
                f.success(tx, &batch, 0)?;
                history::complete_batch(tx, batch.batch.tool_batch_id, f.at)?;
                completion::normal(
                    tx,
                    f.step.model_step_id,
                    batch.batch.tool_batch_id,
                    &input,
                    false,
                    f.at,
                )
            })
            .is_err()
    );
    assert_eq!(inbox::load(store.database.connection(), f.turn.session_id).unwrap(), pending);
    assert!(
        !turns::load(store.database.connection(), f.turn.turn_task_id).unwrap().state.terminal()
    );
    assert!(
        tools::call(store.database.connection(), batch.batch.calls[0].tool_call_id)
            .unwrap()
            .result
            .is_none(),
        "all effects roll back with completion"
    );
}

fn answering_output(f: &Fixture) -> NormalizedModelOutput {
    use aura_model_domain::{AssistantContentBlock, NormalizedStopReason};
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

fn insert_covering_snapshot(store: &mut V2Store, f: &Fixture, include_root: bool) {
    use aura_model_domain::LogicalConversationItem;
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
    let text = "Verify the requested work";
    snapshot.conversation_items = if include_root {
        vec![LogicalConversationItem::User {
            source_event_id: f.turn.root_user_event_id,
            content: text.to_owned(),
            content_hash: ContentHash::sha256(text.as_bytes()),
        }]
    } else {
        vec![]
    };
    let snapshot = snapshot.seal().unwrap();
    store.database.unit_of_work(|tx| context::insert_snapshot(tx, &snapshot)).unwrap();
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

#[test]
fn a_covered_normal_answer_completes_naturally_once() {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    insert_covering_snapshot(&mut store, &f, true);
    let output = answering_output(&f);
    let (first, second) = store
        .database
        .unit_of_work(|tx| -> StoreResult<_> {
            let first = settle(tx, &f, &output)?;
            let second = completion::natural_from_response(tx, &output, f.at)?;
            Ok((first, second))
        })
        .unwrap();
    assert!(first.is_some(), "a covered answer completes");
    assert!(second.is_none(), "a second decision does not open another terminal");
    assert!(
        turns::load(store.database.connection(), f.turn.turn_task_id).unwrap().state.terminal()
    );
    let count: i64 = store
        .database
        .connection()
        .query_row("SELECT COUNT(*) FROM events WHERE kind='agent.natural_completed'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn an_unconsumed_accepted_addition_vetoes_a_natural_completion() {
    use aura_persistence_v2::agent::inbox;
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    insert_covering_snapshot(&mut store, &f, true);
    let output = answering_output(&f);
    let pending = store
        .database
        .unit_of_work(|tx| {
            inbox::submit(
                tx,
                CommandInvocationId::new(),
                &AgentSubmitRequest::new(
                    f.turn.session_id,
                    "还有一件事",
                    AgentInputLane::Addition { turn_task_id: f.turn.turn_task_id },
                ),
                f.at,
            )
        })
        .unwrap();
    let declined = store
        .database
        .unit_of_work(|tx| -> StoreResult<bool> { Ok(settle(tx, &f, &output)?.is_none()) })
        .unwrap();
    assert!(declined, "an unconsumed addition must veto the completion");
    let db = store.database.connection();
    assert!(!turns::load(db, f.turn.turn_task_id).unwrap().state.terminal());
    assert_eq!(inbox::load(db, f.turn.session_id).unwrap(), pending);
}

#[test]
fn revoking_an_addition_before_consumption_allows_natural_completion() {
    use aura_persistence_v2::agent::inbox;
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    insert_covering_snapshot(&mut store, &f, true);
    let output = answering_output(&f);
    let pending = store
        .database
        .unit_of_work(|tx| {
            inbox::submit(
                tx,
                CommandInvocationId::new(),
                &AgentSubmitRequest::new(
                    f.turn.session_id,
                    "先撤回",
                    AgentInputLane::Addition { turn_task_id: f.turn.turn_task_id },
                ),
                f.at,
            )
        })
        .unwrap();
    let message_id = pending.messages[0].message_id;
    store
        .database
        .unit_of_work(|tx| {
            inbox::mutate(
                tx,
                CommandInvocationId::new(),
                &AgentInboxMutation {
                    session_id: f.turn.session_id,
                    expected_revision: pending.revision,
                    action: AgentInboxAction::Revoke { message_id },
                },
                f.at,
            )
        })
        .unwrap();
    let completed = store
        .database
        .unit_of_work(|tx| -> StoreResult<bool> { Ok(settle(tx, &f, &output)?.is_some()) })
        .unwrap();
    assert!(completed, "a revoked addition is no longer a pending source");
}

#[test]
fn cancellation_and_a_formal_wait_veto_natural_completion() {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    insert_covering_snapshot(&mut store, &f, true);
    let output = answering_output(&f);
    let cancelled = store
        .database
        .unit_of_work(|tx| -> StoreResult<bool> {
            aura_persistence_v2::agent::application::request_cancel(tx, f.turn.agent_run_id, f.at)?;
            Ok(settle(tx, &f, &output)?.is_none())
        })
        .unwrap();
    assert!(cancelled);
    assert!(
        !turns::load(store.database.connection(), f.turn.turn_task_id).unwrap().state.terminal()
    );

    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    insert_covering_snapshot(&mut store, &f, true);
    let output = answering_output(&f);
    let waiting = store
        .database
        .unit_of_work(|tx| -> StoreResult<bool> {
            history::record_output(tx, &output, None, f.at)?;
            let old = turns::load_run(tx, f.turn.agent_run_id)?;
            let mut next = old.clone();
            next.revision = Revision::new(old.revision.value() + 1);
            next.next_model_ordinal = old.next_model_ordinal + 1;
            next.updated_at = f.at;
            next.state = AgentRunState::PreparingContext;
            turns::save_run(tx, &old, &next)?;
            let turn = turns::load(tx, f.turn.turn_task_id)?;
            let mut waiting = turn.clone();
            waiting
                .transition(
                    turn.revision,
                    TurnTaskState::AwaitingQuery { query_id: UserQueryId::new() },
                    f.at,
                )
                .map_err(|_| Error::Conflict)?;
            turns::save(tx, &turn, &waiting)?;
            Ok(completion::natural_from_response(tx, &output, f.at)?.is_none())
        })
        .unwrap();
    assert!(waiting, "a formal query wait cannot complete naturally");
    assert!(
        !turns::load(store.database.connection(), f.turn.turn_task_id).unwrap().state.terminal()
    );
}

#[test]
fn natural_completion_requires_the_snapshot_or_its_own_compaction_anchor() {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    insert_covering_snapshot(&mut store, &f, false);
    let output = answering_output(&f);
    let missing = store
        .database
        .unit_of_work(|tx| -> StoreResult<bool> { Ok(settle(tx, &f, &output)?.is_none()) })
        .unwrap();
    assert!(missing, "a summary-free snapshot that omits the source cannot complete");
    assert!(
        !turns::load(store.database.connection(), f.turn.turn_task_id).unwrap().state.terminal()
    );

    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    insert_anchored_snapshot(&mut store, &f, true);
    let output = answering_output(&f);
    let covered = store
        .database
        .unit_of_work(|tx| -> StoreResult<bool> { Ok(settle(tx, &f, &output)?.is_some()) })
        .unwrap();
    assert!(covered, "the snapshot's own compaction anchor covers the source");

    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    insert_anchored_snapshot(&mut store, &f, false);
    let output = answering_output(&f);
    let unrelated = store
        .database
        .unit_of_work(|tx| -> StoreResult<bool> { Ok(settle(tx, &f, &output)?.is_none()) })
        .unwrap();
    assert!(unrelated, "an anchor for a different event is not coverage");
}

fn insert_anchored_snapshot(store: &mut V2Store, f: &Fixture, matching: bool) {
    use aura_context_domain::*;
    use aura_model_domain::LogicalConversationItem;
    let execution = aura_persistence_v2::model::step_snapshot(
        store.database.connection(),
        f.step.model_step_id,
    )
    .unwrap();
    let anchored_event = if matching { f.turn.root_user_event_id } else { EventId::new() };
    let compaction_id = ContextProjectionSnapshotId::new();
    let anchor = CompactionAnchor {
        anchor: "root".into(),
        kind: CompactionAnchorKind::Event,
        source_ref: ContextSourceRef::Event { event_id: anchored_event },
        source_hash: aura_context_domain::user_history_anchor_hash(
            anchored_event,
            "Verify the requested work",
        )
        .unwrap(),
        preview: "Verify the requested work".into(),
    };
    let anchors = CompactionAnchorManifest {
        manifest_hash: ContentHash::sha256(b"anchor-manifest"),
        anchors: vec![anchor],
    };
    let span = CompactionSourceSpan {
        start_sequence: SessionSequence::new(1),
        end_sequence: SessionSequence::new(2),
        source_refs: vec![ContextSourceRef::Event { event_id: anchored_event }],
    };
    let compaction = context::compaction::Compaction {
        id: compaction_id,
        session_id: f.turn.session_id,
        agent_run_id: f.turn.agent_run_id,
        source_step_id: f.step.model_step_id,
        sequence: 1,
        revision: Revision::INITIAL,
        state: "completed".into(),
        source_span: span.clone(),
        anchors: anchors.clone(),
        source_artifact_id: ArtifactId::new(),
        source_hash: ContentHash::sha256(b"sources"),
        excluded_event_ids: vec![anchored_event],
        retained_recent_tokens: 0,
        retention_exceptions: vec![],
        attempts: vec![],
        result: Some(ContextReadCompactionResult {
            source_context_snapshot_id: compaction_id,
            sequence: 1,
            historical_snapshot: "history".into(),
            active_turn_summary: "active".into(),
            structured_compaction: "structured".into(),
            source_span: span,
            anchor_manifest: anchors,
        }),
        result_artifact_id: None,
        created_at: f.at,
    };
    let p06 = aura_context_fixtures::p06_prepared_context_fixture().unwrap();
    let mut snapshot = p06.prepared.context.snapshot;
    snapshot.id = execution.context_snapshot_id;
    snapshot.session_id = f.turn.session_id;
    snapshot.turn_task_id = f.turn.turn_task_id;
    snapshot.agent_run_id = f.turn.agent_run_id;
    snapshot.model_step_id = f.step.model_step_id;
    snapshot.conversation_items = vec![LogicalConversationItem::User {
        source_event_id: EventId::new(),
        content: "unrelated".into(),
        content_hash: ContentHash::sha256(b"unrelated"),
    }];
    let section = snapshot
        .sections
        .iter_mut()
        .find(|section| section.kind == ContextSectionKind::HistoricalSnapshotSummary)
        .unwrap();
    section.source_refs = vec![ContextSourceRef::Compaction { context_snapshot_id: compaction_id }];
    let snapshot = snapshot.seal().unwrap();
    let payload = aura_persistence_core::record::encode(&compaction).unwrap();
    store
        .database
        .unit_of_work(|tx| -> StoreResult<()> {
            tx.execute(
                "INSERT INTO context_compactions VALUES(?1,?2,'completed',1,?3)",
                [compaction_id.to_string(), f.turn.session_id.to_string(), payload.clone()],
            )?;
            context::insert_snapshot(tx, &snapshot)
        })
        .unwrap();
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

fn root_history_item(event_id: EventId) -> aura_model_domain::LogicalConversationItem {
    let text = "Verify the requested work";
    aura_model_domain::LogicalConversationItem::User {
        source_event_id: event_id,
        content: text.to_owned(),
        content_hash: ContentHash::sha256(text.as_bytes()),
    }
}

/// Inserts one completed compaction and a snapshot whose summary points at it.
/// `show_root` puts the stored root text in the frozen history as the production item.
fn insert_event_anchor(
    store: &mut V2Store,
    f: &Fixture,
    event_id: EventId,
    source_hash: ContentHash,
    show_root: bool,
) {
    use aura_context_domain::*;
    use aura_model_domain::LogicalConversationItem;
    let execution = aura_persistence_v2::model::step_snapshot(
        store.database.connection(),
        f.step.model_step_id,
    )
    .unwrap();
    let compaction_id = ContextProjectionSnapshotId::new();
    let anchor = CompactionAnchor {
        anchor: "event".into(),
        kind: CompactionAnchorKind::Event,
        source_ref: ContextSourceRef::Event { event_id },
        source_hash,
        preview: "stored source".into(),
    };
    let anchors = CompactionAnchorManifest {
        manifest_hash: ContentHash::sha256(b"anchor-manifest"),
        anchors: vec![anchor],
    };
    let span = CompactionSourceSpan {
        start_sequence: SessionSequence::new(1),
        end_sequence: SessionSequence::new(2),
        source_refs: vec![ContextSourceRef::Event { event_id }],
    };
    let compaction = context::compaction::Compaction {
        id: compaction_id,
        session_id: f.turn.session_id,
        agent_run_id: f.turn.agent_run_id,
        source_step_id: f.step.model_step_id,
        sequence: 1,
        revision: Revision::INITIAL,
        state: "completed".into(),
        source_span: span.clone(),
        anchors: anchors.clone(),
        source_artifact_id: ArtifactId::new(),
        source_hash: ContentHash::sha256(b"sources"),
        excluded_event_ids: vec![event_id],
        retained_recent_tokens: 0,
        retention_exceptions: vec![],
        attempts: vec![],
        result: Some(ContextReadCompactionResult {
            source_context_snapshot_id: compaction_id,
            sequence: 1,
            historical_snapshot: "history".into(),
            active_turn_summary: "active".into(),
            structured_compaction: "structured".into(),
            source_span: span,
            anchor_manifest: anchors,
        }),
        result_artifact_id: None,
        created_at: f.at,
    };
    let p06 = aura_context_fixtures::p06_prepared_context_fixture().unwrap();
    let mut snapshot = p06.prepared.context.snapshot;
    snapshot.id = execution.context_snapshot_id;
    snapshot.session_id = f.turn.session_id;
    snapshot.turn_task_id = f.turn.turn_task_id;
    snapshot.agent_run_id = f.turn.agent_run_id;
    snapshot.model_step_id = f.step.model_step_id;
    snapshot.conversation_items = if show_root {
        vec![root_history_item(f.turn.root_user_event_id)]
    } else {
        vec![LogicalConversationItem::User {
            source_event_id: EventId::new(),
            content: "unrelated".into(),
            content_hash: ContentHash::sha256(b"unrelated"),
        }]
    };
    let section = snapshot
        .sections
        .iter_mut()
        .find(|section| section.kind == ContextSectionKind::HistoricalSnapshotSummary)
        .unwrap();
    section.source_refs = vec![ContextSourceRef::Compaction { context_snapshot_id: compaction_id }];
    let snapshot = snapshot.seal().unwrap();
    let payload = aura_persistence_core::record::encode(&compaction).unwrap();
    store
        .database
        .unit_of_work(|tx| -> StoreResult<()> {
            tx.execute(
                "INSERT INTO context_compactions VALUES(?1,?2,'completed',1,?3)",
                [compaction_id.to_string(), f.turn.session_id.to_string(), payload.clone()],
            )?;
            context::insert_snapshot(tx, &snapshot)
        })
        .unwrap();
}

#[test]
fn a_production_selector_anchor_covers_a_compacted_addition() {
    let text = "Compacted consumed addition";
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    let event_id = attach_addition(&mut store, &f, text);
    let hash = aura_context_domain::user_history_anchor_hash(event_id, text).unwrap();
    insert_event_anchor(&mut store, &f, event_id, hash, true);
    let output = answering_output(&f);
    let covered = store
        .database
        .unit_of_work(|tx| -> StoreResult<bool> { Ok(settle(tx, &f, &output)?.is_some()) })
        .unwrap();
    assert!(covered, "the selector hash of the stored addition is coverage");
}

#[test]
fn a_source_body_hash_is_not_a_compaction_anchor() {
    let text = "Compacted consumed addition";
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    let event_id = attach_addition(&mut store, &f, text);
    insert_event_anchor(&mut store, &f, event_id, ContentHash::blake3(text.as_bytes()), true);
    let output = answering_output(&f);
    let uncovered = store
        .database
        .unit_of_work(|tx| -> StoreResult<bool> { Ok(settle(tx, &f, &output)?.is_none()) })
        .unwrap();
    assert!(uncovered, "BLAKE3 of the user text is not the selector anchor");

    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    let event_id = attach_addition(&mut store, &f, text);
    insert_event_anchor(&mut store, &f, event_id, ContentHash::sha256(text.as_bytes()), true);
    let output = answering_output(&f);
    let uncovered = store
        .database
        .unit_of_work(|tx| -> StoreResult<bool> { Ok(settle(tx, &f, &output)?.is_none()) })
        .unwrap();
    assert!(uncovered, "SHA-256 of the user text is not the selector anchor");
}

#[test]
fn an_addition_without_an_anchor_blocks_natural_completion() {
    let root = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    insert_covering_snapshot(&mut store, &f, true);
    attach_addition(&mut store, &f, "Still required");
    let output = answering_output(&f);
    let blocked = store
        .database
        .unit_of_work(|tx| -> StoreResult<bool> { Ok(settle(tx, &f, &output)?.is_none()) })
        .unwrap();
    assert!(blocked);
    assert!(
        !turns::load(store.database.connection(), f.turn.turn_task_id).unwrap().state.terminal()
    );
}

#[test]
fn a_later_compaction_covers_an_event_through_the_previous_anchor() {
    use aura_context_domain::*;
    let text = "Compacted consumed addition";
    let root_dir = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root_dir.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    let event_id = attach_addition(&mut store, &f, text);
    let execution = aura_persistence_v2::model::step_snapshot(
        store.database.connection(),
        f.step.model_step_id,
    )
    .unwrap();
    let first_id = ContextProjectionSnapshotId::new();
    let result_artifact_id = ArtifactId::new();
    let event_anchor = CompactionAnchor {
        anchor: "addition".into(),
        kind: CompactionAnchorKind::Event,
        source_ref: ContextSourceRef::Event { event_id },
        source_hash: user_history_anchor_hash(event_id, text).unwrap(),
        preview: text.into(),
    };
    let first_anchors = CompactionAnchorManifest {
        manifest_hash: ContentHash::sha256(b"first-manifest"),
        anchors: vec![event_anchor],
    };
    let first_span = CompactionSourceSpan {
        start_sequence: SessionSequence::new(1),
        end_sequence: SessionSequence::new(2),
        source_refs: vec![ContextSourceRef::Event { event_id }],
    };
    let first_result = ContextReadCompactionResult {
        source_context_snapshot_id: first_id,
        sequence: 1,
        historical_snapshot: "history".into(),
        active_turn_summary: "active".into(),
        structured_compaction: "structured".into(),
        source_span: first_span.clone(),
        anchor_manifest: first_anchors.clone(),
    };
    let first = context::compaction::Compaction {
        id: first_id,
        session_id: f.turn.session_id,
        agent_run_id: f.turn.agent_run_id,
        source_step_id: f.step.model_step_id,
        sequence: 1,
        revision: Revision::INITIAL,
        state: "completed".into(),
        source_span: first_span,
        anchors: first_anchors,
        source_artifact_id: ArtifactId::new(),
        source_hash: ContentHash::sha256(b"first-sources"),
        excluded_event_ids: vec![event_id],
        retained_recent_tokens: 0,
        retention_exceptions: vec![],
        attempts: vec![],
        result: Some(first_result.clone()),
        result_artifact_id: Some(result_artifact_id),
        created_at: f.at,
    };
    let second_id = ContextProjectionSnapshotId::new();
    let previous_anchor = CompactionAnchor {
        anchor: "previous".into(),
        kind: CompactionAnchorKind::Artifact,
        source_ref: ContextSourceRef::Artifact { artifact_id: result_artifact_id },
        source_hash: compaction_result_anchor_hash(&first_result).unwrap(),
        preview: "previous".into(),
    };
    let second_anchors = CompactionAnchorManifest {
        manifest_hash: ContentHash::sha256(b"second-manifest"),
        anchors: vec![previous_anchor],
    };
    let second_span = CompactionSourceSpan {
        start_sequence: SessionSequence::new(3),
        end_sequence: SessionSequence::new(4),
        source_refs: vec![ContextSourceRef::Artifact { artifact_id: result_artifact_id }],
    };
    let second = context::compaction::Compaction {
        id: second_id,
        session_id: f.turn.session_id,
        agent_run_id: f.turn.agent_run_id,
        source_step_id: f.step.model_step_id,
        sequence: 2,
        revision: Revision::INITIAL,
        state: "completed".into(),
        source_span: second_span.clone(),
        anchors: second_anchors.clone(),
        source_artifact_id: ArtifactId::new(),
        source_hash: ContentHash::sha256(b"second-sources"),
        excluded_event_ids: vec![event_id],
        retained_recent_tokens: 0,
        retention_exceptions: vec![],
        attempts: vec![],
        result: Some(ContextReadCompactionResult {
            source_context_snapshot_id: second_id,
            sequence: 2,
            historical_snapshot: "later".into(),
            active_turn_summary: "later".into(),
            structured_compaction: "later".into(),
            source_span: second_span,
            anchor_manifest: second_anchors,
        }),
        result_artifact_id: Some(ArtifactId::new()),
        created_at: f.at,
    };
    let p06 = aura_context_fixtures::p06_prepared_context_fixture().unwrap();
    let mut snapshot = p06.prepared.context.snapshot;
    snapshot.id = execution.context_snapshot_id;
    snapshot.session_id = f.turn.session_id;
    snapshot.turn_task_id = f.turn.turn_task_id;
    snapshot.agent_run_id = f.turn.agent_run_id;
    snapshot.model_step_id = f.step.model_step_id;
    snapshot.conversation_items = vec![root_history_item(f.turn.root_user_event_id)];
    let section = snapshot
        .sections
        .iter_mut()
        .find(|section| section.kind == ContextSectionKind::HistoricalSnapshotSummary)
        .unwrap();
    section.source_refs = vec![ContextSourceRef::Compaction { context_snapshot_id: second_id }];
    let snapshot = snapshot.seal().unwrap();
    let first_payload = aura_persistence_core::record::encode(&first).unwrap();
    let second_payload = aura_persistence_core::record::encode(&second).unwrap();
    store
        .database
        .unit_of_work(|tx| -> StoreResult<()> {
            tx.execute(
                "INSERT INTO context_compactions VALUES(?1,?2,'completed',1,?3)",
                [first_id.to_string(), f.turn.session_id.to_string(), first_payload.clone()],
            )?;
            tx.execute(
                "INSERT INTO context_compactions VALUES(?1,?2,'completed',1,?3)",
                [second_id.to_string(), f.turn.session_id.to_string(), second_payload.clone()],
            )?;
            context::insert_snapshot(tx, &snapshot)
        })
        .unwrap();
    let output = answering_output(&f);
    let covered = store
        .database
        .unit_of_work(|tx| -> StoreResult<bool> { Ok(settle(tx, &f, &output)?.is_some()) })
        .unwrap();
    assert!(covered, "the latest summary follows the previous artifact anchor");
}

#[test]
fn a_joined_user_item_needs_the_frozen_manifest_not_a_substring() {
    use aura_context_domain::*;
    use aura_model_domain::LogicalConversationItem;
    use aura_security_contract::NetworkDataClass;
    let text = "Verify the requested work";
    let wrapped = format!("<Current-Turn-Working-Set>\n{text}\n</Current-Turn-Working-Set>");
    let root_dir = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root_dir.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
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
    snapshot.conversation_items = vec![LogicalConversationItem::User {
        source_event_id: f.turn.root_user_event_id,
        content: wrapped.clone(),
        content_hash: ContentHash::sha256(wrapped.as_bytes()),
    }];
    for section in &mut snapshot.sections {
        if section.kind == ContextSectionKind::HistoricalSnapshotSummary {
            section.source_refs.clear();
        }
    }
    let bare = snapshot.clone().seal().unwrap();
    store.database.unit_of_work(|tx| context::insert_snapshot(tx, &bare)).unwrap();
    let output = answering_output(&f);
    let substring_is_not_identity = store
        .database
        .unit_of_work(|tx| -> StoreResult<bool> { Ok(settle(tx, &f, &output)?.is_none()) })
        .unwrap();
    assert!(substring_is_not_identity);

    let root_dir = tempfile::tempdir().unwrap();
    let mut store = V2Store::create(&root_dir.path().join("v2")).unwrap();
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
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
    snapshot.conversation_items = vec![LogicalConversationItem::User {
        source_event_id: f.turn.root_user_event_id,
        content: wrapped.clone(),
        content_hash: ContentHash::sha256(wrapped.as_bytes()),
    }];
    for section in &mut snapshot.sections {
        if section.kind == ContextSectionKind::HistoricalSnapshotSummary {
            section.source_refs.clear();
        }
    }
    let manifest = ModelEgressManifestV1::new(
        snapshot.id,
        vec![ModelEgressItem {
            ordinal: 1,
            source_ref: ContextSourceRef::Event { event_id: f.turn.root_user_event_id },
            source_kind: ModelEgressSourceKind::UserEvent,
            data_class: NetworkDataClass::UserFileContent,
            content_hash: ContentHash::sha256(text.as_bytes()),
            byte_length: text.len() as u64,
            authorization_basis: ModelEgressAuthorizationBasis::UserSubmission,
        }],
    )
    .unwrap();
    let artifact_id = ArtifactId::new();
    snapshot.model_egress_manifest_artifact_id = artifact_id;
    snapshot.model_egress_manifest_hash = manifest.manifest_hash.clone();
    let snapshot = snapshot.seal().unwrap();
    let bytes = aura_domain_common::canonical_json_v1(&manifest).unwrap();
    store
        .database
        .unit_of_work(|tx| -> StoreResult<()> {
            aura_persistence_core::artifact::put(
                tx,
                artifact_id,
                &f.step.model_step_id.to_string(),
                "application/vnd.aura.model-egress-manifest+json",
                &bytes,
                f.at,
            )?;
            context::insert_snapshot(tx, &snapshot)
        })
        .unwrap();
    let output = answering_output(&f);
    let bound = store
        .database
        .unit_of_work(|tx| -> StoreResult<bool> { Ok(settle(tx, &f, &output)?.is_some()) })
        .unwrap();
    assert!(bound, "the frozen manifest hash of the stored text is coverage");
}

#[test]
fn completion_results_history_lineage_and_projection_rollback_and_restart_together() {
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().join("v2");
    let mut store = V2Store::create(&root).expect("store");
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    let (review, input) = inputs(&f);
    let batch = f.admit(
        &mut store,
        vec![
            ("task_lineage.review_completion", serde_json::to_value(&review).expect("json")),
            ("normal.complete", serde_json::to_value(&input).expect("json")),
        ],
    );
    let failed: StoreResult<()> = store.database.unit_of_work(|tx| {
        finish(tx, &f, &batch, &review, &input)?;
        Err(Error::Invalid("power loss before commit"))
    });
    assert!(failed.is_err());
    drop(store);
    let mut store = V2Store::open(&root).expect("restart");
    let db = store.database.connection();
    assert_eq!(turns::load(db, f.turn.turn_task_id).expect("turn"), f.turn);
    assert_eq!(
        lineage::load(db, f.turn.lineage_id).expect("lineage").completion_gate,
        CompletionGate::Unreviewed
    );
    assert!(context::history(db, f.turn.session_id).expect("history").is_empty());
    assert!(tools::batch(db, batch.batch.tool_batch_id).expect("batch").result.is_none());
    assert!(tools::call(db, batch.batch.calls[0].tool_call_id).expect("call").attempt_id.is_none());
    store
        .database
        .unit_of_work(|tx| finish(tx, &f, &batch, &review, &input))
        .expect("atomic complete");
    drop(store);
    let store = V2Store::open(&root).expect("second restart");
    let db = store.database.connection();
    assert_eq!(
        turns::load(db, f.turn.turn_task_id).expect("turn").state,
        TurnTaskState::Completed { final_report: input.final_report }
    );
    assert_eq!(
        lineage::load(db, f.turn.lineage_id).expect("lineage").state,
        TaskLineageState::Completed
    );
    assert_eq!(
        turns::load_run(db, f.turn.agent_run_id).expect("run").state,
        AgentRunState::Completed
    );
    assert_eq!(session::load(db, f.turn.session_id).expect("session").active_turn, None);
    assert_eq!(context::history(db, f.turn.session_id).expect("history").len(), 1);
    assert_eq!(
        tools::batch(db, batch.batch.tool_batch_id)
            .expect("batch")
            .result
            .expect("terminal")
            .results
            .len(),
        2
    );
}
#[test]
fn completion_preserves_queued_user_input_without_using_it_as_a_quality_veto() {
    let dir = tempfile::tempdir().expect("temp");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = Fixture::new(&mut store, AgentTaskMode::Normal);
    let (review, input) = inputs(&f);
    let batch = f.admit(
        &mut store,
        vec![
            ("task_lineage.review_completion", serde_json::to_value(&review).expect("json")),
            ("normal.complete", serde_json::to_value(&input).expect("json")),
        ],
    );
    let mut forged = input.clone();
    forged.final_report = "Not the model's actual call".into();
    assert!(store.database.unit_of_work(|tx| finish(tx, &f, &batch, &review, &forged)).is_err());
    store
        .database
        .unit_of_work(|tx| {
            let user = OriginalUserMessage::new(
                EventId::new(),
                f.turn.session_id,
                "Also verify cancellation".into(),
                UserMessageOrigin::Steer,
                f.at,
            )
            .expect("steer");
            session::add_user_message(tx, &user)?;
            let old = session::load(tx, f.turn.session_id)?;
            let mut next = old.clone();
            next.pending_user_event_ids.push(user.event_id);
            next.revision = Revision::new(old.revision.value() + 1);
            session::save(tx, &old, &next, f.at)
        })
        .expect("queued real user input");
    store
        .database
        .unit_of_work(|tx| finish(tx, &f, &batch, &review, &input))
        .expect("explicit model completion");
    let db = store.database.connection();
    assert!(turns::load(db, f.turn.turn_task_id).expect("turn").state.terminal());
    assert_eq!(
        session::load(db, f.turn.session_id).expect("session").pending_user_event_ids.len(),
        1
    );
    assert_eq!(context::history(db, f.turn.session_id).expect("history").len(), 1);
}
#[test]
fn generic_state_updates_cannot_bypass_completion_and_goal_budget_is_not_completion() {
    let dir = tempfile::tempdir().expect("temp");
    let mut store = V2Store::create(&dir.path().join("v2")).expect("store");
    let f = Fixture::new(&mut store, AgentTaskMode::Goal);
    assert!(
        store
            .database
            .unit_of_work(|tx| {
                let mut next = f.turn.clone();
                next.transition(
                    f.turn.revision,
                    TurnTaskState::Completed { final_report: "Bypass".into() },
                    f.at,
                )
                .expect("shape");
                turns::save(tx, &f.turn, &next)
            })
            .is_err()
    );
    assert!(
        store
            .database
            .unit_of_work(|tx| {
                let old = work::goal(tx, f.turn.turn_task_id)?;
                let mut next = old.clone();
                next.completed = true;
                next.revision = Revision::new(2);
                work::save_goal(tx, &old, &next, f.at)
            })
            .is_err()
    );
    assert!(!work::goal(store.database.connection(), f.turn.turn_task_id).expect("goal").completed);
}
