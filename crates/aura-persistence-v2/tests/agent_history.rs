#[path = "support/agent.rs"]
#[allow(dead_code)]
mod support;
use aura_agent_domain::*;
use aura_domain_common::*;
use aura_model_domain::AssistantContentBlock;
use aura_persistence_core::StoreResult;
use aura_persistence_v2::{
    V2Store,
    agent::{desktop, desktop_messages, history, inbox, input_consumption, lifecycle, turns},
    context, model,
};

#[test]
fn recovery_entry_is_removed_and_a_new_turn_waits_for_the_intent_decision() {
    for mode in [AgentTaskMode::Normal, AgentTaskMode::Plan, AgentTaskMode::Goal] {
        let root = tempfile::tempdir().expect("directory");
        let mut store = V2Store::create(&root.path().join("v2")).expect("store");
        let f = support::Fixture::new(&mut store, mode);
        store
            .database
            .unit_of_work(|tx| {
                aura_persistence_v2::agent::recovery::interrupt(
                    tx,
                    f.turn.agent_run_id,
                    "agent.cancelled",
                    true,
                    f.at,
                )
            })
            .expect("explicit stop");
        let source =
            turns::load(store.database.connection(), f.turn.turn_task_id).expect("terminal");
        let request = AgentRelatedTurnRequest {
            session_id: source.session_id,
            source_turn_task_id: source.turn_task_id,
            expected_revision: source.revision,
            kind: AgentRelatedTurnKind::Recover,
            goal_budget: None,
        };
        assert!(
            aura_persistence_v2::agent::related::validate(store.database.connection(), &request)
                .is_err()
        );
        let next = store
            .database
            .unit_of_work(|tx| {
                let mut bundle = aura_context_fixtures::p06_prepared_context_fixture()
                    .expect("bundle")
                    .prompt_bundle_snapshot;
                bundle.id = PromptBundleSnapshotId::new();
                bundle.turn_task_id = TurnTaskId::new();
                context::insert_bundle(tx, source.session_id, &bundle)?;
                let user = OriginalUserMessage::new(
                    EventId::new(),
                    source.session_id,
                    "Continue from readable background".into(),
                    UserMessageOrigin::UserInput,
                    source.updated_at,
                )
                .expect("user");
                lifecycle::start(tx, &user, AgentTaskMode::Normal, None, bundle.id)
            })
            .expect("new turn after the interruption");
        // A new ordinary turn never silently restores a special mode: it enters as
        // Normal and waits for the AI intent decision on the interrupted episode.
        assert_eq!(next.mode, AgentTaskMode::Normal);
        let waiting =
            aura_persistence_v2::agent::mode::view(store.database.connection(), source.session_id)
                .expect("episode");
        assert_eq!(waiting.mode, mode);
        if mode == AgentTaskMode::Normal {
            assert!(!waiting.disposition_required, "Normal has no cycle to adjudicate");
        } else {
            assert!(
                waiting.disposition_required,
                "an interrupted {mode:?} waits for the intent decision"
            );
            // Skip ends the old cycle and answers this turn as Normal.
            let skipped = store
                .database
                .unit_of_work(|tx| {
                    aura_persistence_v2::agent::mode::decide(
                        tx,
                        next.turn_task_id,
                        &ModeDispositionInput {
                            decision: ModeDispositionDecision::Skip,
                            reason: "The new message is unrelated to the interrupted mode".into(),
                        },
                        f.at,
                    )
                })
                .expect("skip disposition");
            assert_eq!(skipped.mode, AgentTaskMode::Normal);
            assert_eq!(skipped.end_reason, Some(ModeEpisodeEndReason::Skipped));
            assert!(!skipped.disposition_required);
            assert_eq!(
                turns::load(store.database.connection(), next.turn_task_id)
                    .expect("turn retained")
                    .mode,
                AgentTaskMode::Normal
            );
            // The cycle is closed: a second decision cannot revive it.
            assert!(
                store
                    .database
                    .unit_of_work(|tx| {
                        aura_persistence_v2::agent::mode::decide(
                            tx,
                            next.turn_task_id,
                            &ModeDispositionInput {
                                decision: ModeDispositionDecision::Skip,
                                reason: "Repeated decision".into(),
                            },
                            f.at,
                        )
                    })
                    .is_err(),
                "a decided episode cannot be decided twice"
            );
        }
        assert_eq!(
            turns::load(store.database.connection(), source.turn_task_id).expect("source retained"),
            source
        );
    }
}

#[test]
fn long_turn_pages_preserve_all_sources_steps_and_exact_utf8_text() {
    let root = tempfile::tempdir().expect("directory");
    let mut store = V2Store::create(&root.path().join("v2")).expect("store");
    let f = support::Fixture::new(&mut store, AgentTaskMode::Plan);
    let foreign = SessionId::new();
    store
        .database
        .unit_of_work(|tx| {
            aura_persistence_v2::workspace::insert_session(
                tx,
                &aura_persistence_v2::workspace::SessionMetadata {
                    id: foreign,
                    workspace_id: f.turn.workspace_id,
                    title: "Foreign".into(),
                    created_at: f.at,
                    updated_at: f.at,
                },
            )
        })
        .expect("foreign session");
    let long = "正文🙂\n".repeat(10_000);
    let snapshot =
        model::step_snapshot(store.database.connection(), f.step.model_step_id).expect("snapshot");
    store
        .database
        .unit_of_work(|tx| -> StoreResult<()> {
            for n in 0..22 {
                inbox::submit(
                    tx,
                    CommandInvocationId::new(),
                    &AgentSubmitRequest {
                        session_id: f.turn.session_id,
                        text: format!("addition {n}: {long}"),
                        destination: AgentInputLane::Addition { turn_task_id: f.turn.turn_task_id },
                        skill_ids: Vec::new(),
                        reasoning_effort: None,
                        attachments: Vec::new(),
                    },
                    f.at,
                )?;
            }
            let old = turns::load_run(tx, f.turn.agent_run_id)?;
            let mut next = old.clone();
            next.revision = Revision::new(old.revision.value() + 1);
            next.state = AgentRunState::PreparingContext;
            turns::save_run(tx, &old, &next)?;
            input_consumption::additions(tx, f.turn.agent_run_id, f.at)?;
            let mut current = f.step.clone();
            for ordinal in 0..23 {
                if ordinal > 0 {
                    let old = turns::load_run(tx, f.turn.agent_run_id)?;
                    let mut next = old.clone();
                    next.revision = Revision::new(old.revision.value() + 1);
                    next.state = AgentRunState::PreparingContext;
                    next.next_model_ordinal = current.ordinal + 1;
                    turns::save_run(tx, &old, &next)?;
                    let prepared = lifecycle::prepare_step(
                        tx,
                        f.turn.agent_run_id,
                        AgentContinuation::CompletionReminder {
                            previous_model_step_id: current.model_step_id,
                        },
                        f.at,
                    )?;
                    let mut frozen = snapshot.clone();
                    frozen.id = ModelExecutionSnapshotId::new();
                    frozen.model_step_id = prepared.model_step_id;
                    model::insert_snapshot(tx, f.turn.session_id, &frozen)?;
                    current = prepared.clone();
                    current.state = ModelStepState::Running { execution_snapshot_id: frozen.id };
                    turns::save_step(tx, &prepared, &current)?;
                }
                let old = turns::load_run(tx, f.turn.agent_run_id)?;
                let mut next = old.clone();
                next.revision = Revision::new(old.revision.value() + 1);
                next.state = AgentRunState::ModelRunning { model_step_id: current.model_step_id };
                turns::save_run(tx, &old, &next)?;
                let mut output = f.template.clone();
                output.model_step_id = current.model_step_id;
                output.tool_calls.clear();
                output.refusal = None;
                output.assistant_blocks = vec![AssistantContentBlock {
                    block_id: ModelContentBlockId::new(),
                    text: long.clone(),
                    provider_reference: None,
                }];
                history::record_output(tx, &output, None, f.at)?;
            }
            Ok(())
        })
        .expect("durable long turn");
    let mut request = AgentHistoryRequest {
        session_id: f.turn.session_id,
        before_turn_task_id: None,
        within_turn: None,
        limit: 25,
    };
    let mut users = std::collections::BTreeSet::new();
    let mut steps = std::collections::BTreeSet::new();
    let runtime = RuntimeInstanceId::new();
    let mut first_cursor = None;
    loop {
        let page =
            desktop::projection(store.database.connection(), &request, runtime).expect("page");
        assert!(serde_json::to_vec(&page).expect("json").len() < 200_000);
        let turn = &page.turns[0];
        assert!(turn.user_messages.len() <= 10 && turn.assistant_messages.len() <= 10);
        for user in &turn.user_messages {
            assert!(users.insert(user.event_id), "no duplicated source");
        }
        for step in &turn.assistant_messages {
            assert!(steps.insert(step.model_step_id), "no duplicated step");
            assert!(step.text.len() <= 2048);
            assert_eq!(step.text_total_bytes, long.len() as u64);
        }
        if first_cursor.is_none() {
            first_cursor = turn.next_messages.clone();
        }
        match &turn.next_messages {
            Some(cursor) => request.within_turn = Some(cursor.clone()),
            None => break,
        }
    }
    assert_eq!(users.len(), 23);
    assert_eq!(steps.len(), 23);
    let source = AgentMessageTextSource::Assistant { model_step_id: f.step.model_step_id };
    let mut read =
        AgentMessageTextRequest { session_id: f.turn.session_id, source, offset: 0, limit: 4096 };
    let mut text = String::new();
    loop {
        let page = desktop_messages::text(store.database.connection(), &read).expect("text page");
        text.push_str(&page.text);
        if let Some(offset) = page.next_offset {
            assert!(offset > read.offset);
            read.offset = offset;
        } else {
            break;
        }
    }
    assert_eq!(text, long);
    read.offset = 1;
    let page = desktop_messages::text(store.database.connection(), &read).expect("snapped page");
    assert!(page.from_offset <= 1 && long[page.from_offset as usize..].starts_with(&page.text));
    read.offset = 0;
    read.session_id = foreign;
    assert!(
        desktop_messages::text(store.database.connection(), &read).is_err(),
        "reject foreign text"
    );
    request.session_id = foreign;
    request.within_turn = first_cursor;
    assert!(
        desktop::projection(store.database.connection(), &request, runtime).is_err(),
        "reject foreign turn cursor"
    );
}
