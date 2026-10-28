//! A compacted addition stays covered after its text leaves the request: the compaction the
//! request shows is followed, as the store recorded it, to the source it replaced.
use super::*;

#[test]
fn tool_continuation_accepts_a_compacted_addition_only_with_its_anchor() {
    let mut input = tool_continuation_input(false);
    let event_id = authorize_addition(&mut input, "Compacted addition.", false);
    let hash = aura_context_domain::user_history_anchor_hash(event_id, "Compacted addition.")
        .expect("production history-item anchor");
    input.compacted_source_proofs.push(
        aura_context_domain::prove_user_source(event_id, "Compacted addition.", &hash)
            .expect("proof bound from the stored text"),
    );
    let result = aura_context_domain::ContextReadCompactionResult {
        source_context_snapshot_id: ContextProjectionSnapshotId::new(),
        sequence: 1,
        historical_snapshot: "history".to_owned(),
        active_turn_summary: "active".to_owned(),
        structured_compaction: "structured".to_owned(),
        source_span: aura_context_domain::CompactionSourceSpan {
            start_sequence: SessionSequence::new(1),
            end_sequence: SessionSequence::new(2),
            source_refs: vec![aura_context_domain::ContextSourceRef::Event { event_id }],
        },
        anchor_manifest: aura_context_domain::CompactionAnchorManifest {
            anchors: vec![aura_context_domain::CompactionAnchor {
                anchor: "addition".to_owned(),
                kind: aura_context_domain::CompactionAnchorKind::Event,
                source_ref: aura_context_domain::ContextSourceRef::Event { event_id },
                source_hash: hash,
                preview: "Compacted addition.".to_owned(),
            }],
            manifest_hash: ContentHash::sha256(b"manifest"),
        },
    };
    input
        .section_inputs
        .iter_mut()
        .find(|section| section.kind == ContextSectionKind::RecentHistory)
        .expect("history section")
        .fragments
        .push(ContextFragment {
            source_ref: aura_context_domain::ContextSourceRef::Compaction {
                context_snapshot_id: result.source_context_snapshot_id,
            },
            source_kind: ModelEgressSourceKind::Compaction,
            data_class: NetworkDataClass::WorkspaceSource,
            authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content: serde_json::to_string(&result).expect("compaction result"),
        });
    let session_id = input.request.session_id;
    input.coverage_hops.push(stored_hop(&result, session_id, event_id));
    let prepared = compile_prepared(input);
    assert!(
        !prepared
            .context
            .conversation_items
            .iter()
            .any(|item| { conversation_source_event_id(item) == event_id })
    );

    let mut missing = tool_continuation_input(false);
    authorize_addition(&mut missing, "Compacted addition.", false);
    let estimators = ContextEstimatorRegistry::default();
    let compiler = ContextPreparationCompiler::new(&estimators);
    assert!(matches!(
        compiler.compile(missing),
        Err(ContextPreparationError::ToolContinuationInvalid)
    ));
}

#[test]
fn tool_continuation_keeps_a_compacted_addition_after_its_text_is_removed() {
    let mut input = tool_continuation_input(false);
    let first_text = "First consumed addition.";
    let second_text = "Second consumed addition.";
    let first = authorize_addition(&mut input, first_text, true);
    authorize_addition(&mut input, second_text, true);
    compile_prepared(input.clone());
    input.recent_history_items.retain(|item| conversation_source_event_id(item) != first);
    for section in &mut input.section_inputs {
        section.fragments.retain(|fragment| {
            fragment.source_ref != aura_context_domain::ContextSourceRef::Event { event_id: first }
        });
    }
    let hash = aura_context_domain::user_history_anchor_hash(first, first_text).expect("anchor");
    input
        .compacted_source_proofs
        .push(aura_context_domain::prove_user_source(first, first_text, &hash).expect("proof"));
    let result = aura_context_domain::ContextReadCompactionResult {
        source_context_snapshot_id: ContextProjectionSnapshotId::new(),
        sequence: 1,
        historical_snapshot: "Earlier input".to_owned(),
        active_turn_summary: "Both additions apply".to_owned(),
        structured_compaction: "summary".to_owned(),
        source_span: aura_context_domain::CompactionSourceSpan {
            start_sequence: SessionSequence::new(5),
            end_sequence: SessionSequence::new(6),
            source_refs: vec![aura_context_domain::ContextSourceRef::Event { event_id: first }],
        },
        anchor_manifest: aura_context_domain::CompactionAnchorManifest {
            manifest_hash: ContentHash::sha256(b"manifest"),
            anchors: vec![aura_context_domain::CompactionAnchor {
                anchor: "a000001".to_owned(),
                kind: aura_context_domain::CompactionAnchorKind::Event,
                source_ref: aura_context_domain::ContextSourceRef::Event { event_id: first },
                source_hash: hash,
                preview: first_text.to_owned(),
            }],
        },
    };
    input.section_inputs.push(ContextSectionInput {
        kind: ContextSectionKind::HistoricalSnapshotSummary,
        compressibility: ContextCompressibility::CompressibleHistory,
        privacy_class: ContextPrivacyClass::ModelVisible,
        fragments: vec![ContextFragment {
            source_ref: aura_context_domain::ContextSourceRef::Compaction {
                context_snapshot_id: result.source_context_snapshot_id,
            },
            source_kind: ModelEgressSourceKind::Compaction,
            data_class: NetworkDataClass::WorkspaceSource,
            authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content: serde_json::to_string(&result).expect("result"),
        }],
    });
    let session_id = input.request.session_id;
    input.coverage_hops.push(stored_hop(&result, session_id, first));
    let prepared = compile_prepared(input);
    let texts = prepared
        .context
        .conversation_items
        .iter()
        .filter_map(|item| match item {
            LogicalConversationItem::User { content, .. } => Some(content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(texts.contains(&second_text));
    assert!(!texts.iter().any(|text| text.contains(first_text)));
}

#[test]
fn tool_continuation_follows_a_previous_summary_to_a_compacted_addition() {
    let mut input = tool_continuation_input(false);
    let text = "Compacted addition.";
    let event_id = authorize_addition(&mut input, text, false);
    let anchor_hash =
        aura_context_domain::user_history_anchor_hash(event_id, text).expect("anchor");
    input
        .compacted_source_proofs
        .push(aura_context_domain::prove_user_source(event_id, text, &anchor_hash).expect("proof"));
    let origin_artifact = ArtifactId::new();
    let origin_id = ContextProjectionSnapshotId::new();
    let origin_anchors = aura_context_domain::CompactionAnchorManifest {
        manifest_hash: ContentHash::sha256(b"origin"),
        anchors: vec![aura_context_domain::CompactionAnchor {
            anchor: "event".to_owned(),
            kind: aura_context_domain::CompactionAnchorKind::Event,
            source_ref: aura_context_domain::ContextSourceRef::Event { event_id },
            source_hash: anchor_hash,
            preview: "preview".to_owned(),
        }],
    };
    let origin = aura_context_domain::ContextReadCompactionResult {
        source_context_snapshot_id: origin_id,
        sequence: 1,
        historical_snapshot: "history".to_owned(),
        active_turn_summary: "active".to_owned(),
        structured_compaction: "structured".to_owned(),
        source_span: aura_context_domain::CompactionSourceSpan {
            start_sequence: SessionSequence::new(1),
            end_sequence: SessionSequence::new(1),
            source_refs: vec![aura_context_domain::ContextSourceRef::Event { event_id }],
        },
        anchor_manifest: origin_anchors.clone(),
    };
    let origin_hop = aura_context_domain::CoverageHop::from_record(
        origin_id,
        input.request.session_id,
        true,
        &origin_anchors,
        vec![event_id],
        Some(origin_artifact),
        Some(origin),
    );
    let latest_id = ContextProjectionSnapshotId::new();
    let latest_hash = aura_context_domain::compaction_result_anchor_hash(
        origin_hop.result.as_ref().expect("origin"),
    )
    .expect("previous hash");
    let latest_anchors = aura_context_domain::CompactionAnchorManifest {
        manifest_hash: ContentHash::sha256(b"latest"),
        anchors: vec![aura_context_domain::CompactionAnchor {
            anchor: "previous".to_owned(),
            kind: aura_context_domain::CompactionAnchorKind::Artifact,
            source_ref: aura_context_domain::ContextSourceRef::Artifact {
                artifact_id: origin_artifact,
            },
            source_hash: latest_hash,
            preview: "previous verified compaction".to_owned(),
        }],
    };
    let latest = aura_context_domain::ContextReadCompactionResult {
        source_context_snapshot_id: latest_id,
        sequence: 2,
        historical_snapshot: "later".to_owned(),
        active_turn_summary: "later".to_owned(),
        structured_compaction: "later".to_owned(),
        source_span: aura_context_domain::CompactionSourceSpan {
            start_sequence: SessionSequence::new(2),
            end_sequence: SessionSequence::new(2),
            source_refs: vec![aura_context_domain::ContextSourceRef::Artifact {
                artifact_id: origin_artifact,
            }],
        },
        anchor_manifest: latest_anchors.clone(),
    };
    let latest_hop = aura_context_domain::CoverageHop::from_record(
        latest_id,
        input.request.session_id,
        true,
        &latest_anchors,
        vec![event_id],
        Some(ArtifactId::new()),
        Some(latest.clone()),
    );
    input.coverage_hops = vec![latest_hop, origin_hop];
    input.section_inputs.push(ContextSectionInput {
        kind: ContextSectionKind::HistoricalSnapshotSummary,
        compressibility: ContextCompressibility::CompressibleHistory,
        privacy_class: ContextPrivacyClass::ModelVisible,
        fragments: vec![ContextFragment {
            source_ref: aura_context_domain::ContextSourceRef::Compaction {
                context_snapshot_id: latest_id,
            },
            source_kind: ModelEgressSourceKind::Compaction,
            data_class: NetworkDataClass::WorkspaceSource,
            authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content: serde_json::to_string(&latest).expect("result"),
        }],
    });
    compile_prepared(input);
}

#[test]
fn tool_continuation_rejects_a_compacted_addition_anchored_by_its_body_hash() {
    let mut input = tool_continuation_input(false);
    let event_id = authorize_addition(&mut input, "Compacted addition.", false);
    let real = aura_context_domain::user_history_anchor_hash(event_id, "Compacted addition.")
        .expect("production anchor");
    input.compacted_source_proofs.push(
        aura_context_domain::prove_user_source(event_id, "Compacted addition.", &real)
            .expect("proof"),
    );
    let result = aura_context_domain::ContextReadCompactionResult {
        source_context_snapshot_id: ContextProjectionSnapshotId::new(),
        sequence: 1,
        historical_snapshot: "history".to_owned(),
        active_turn_summary: "active".to_owned(),
        structured_compaction: "structured".to_owned(),
        source_span: aura_context_domain::CompactionSourceSpan {
            start_sequence: SessionSequence::new(1),
            end_sequence: SessionSequence::new(2),
            source_refs: vec![aura_context_domain::ContextSourceRef::Event { event_id }],
        },
        anchor_manifest: aura_context_domain::CompactionAnchorManifest {
            anchors: vec![aura_context_domain::CompactionAnchor {
                anchor: "addition".to_owned(),
                kind: aura_context_domain::CompactionAnchorKind::Event,
                source_ref: aura_context_domain::ContextSourceRef::Event { event_id },
                source_hash: ContentHash::sha256(b"Compacted addition."),
                preview: "Compacted addition.".to_owned(),
            }],
            manifest_hash: ContentHash::sha256(b"manifest"),
        },
    };
    input
        .section_inputs
        .iter_mut()
        .find(|section| section.kind == ContextSectionKind::RecentHistory)
        .expect("history section")
        .fragments
        .push(ContextFragment {
            source_ref: aura_context_domain::ContextSourceRef::Compaction {
                context_snapshot_id: result.source_context_snapshot_id,
            },
            source_kind: ModelEgressSourceKind::Compaction,
            data_class: NetworkDataClass::WorkspaceSource,
            authorization_basis: ModelEgressAuthorizationBasis::ModelChannelPolicy,
            content: serde_json::to_string(&result).expect("compaction result"),
        });
    let estimators = ContextEstimatorRegistry::default();
    let compiler = ContextPreparationCompiler::new(&estimators);
    assert!(matches!(
        compiler.compile(input),
        Err(ContextPreparationError::ToolContinuationInvalid)
    ));
}

/// The hop Core reads from the store for a compaction the request shows.
fn stored_hop(
    result: &aura_context_domain::ContextReadCompactionResult,
    session_id: SessionId,
    excluded: EventId,
) -> aura_context_domain::CoverageHop {
    aura_context_domain::CoverageHop {
        id: result.source_context_snapshot_id,
        session_id,
        completed: true,
        manifest_matches: true,
        excluded_event_ids: vec![excluded],
        result_artifact_id: None,
        result: Some(result.clone()),
    }
}
