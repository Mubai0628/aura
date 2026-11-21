use super::*;

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
        background_sources: vec![],
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
        background_sources: vec![],
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
