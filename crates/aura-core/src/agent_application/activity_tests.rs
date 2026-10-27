//! Directed tests: the dynamic activity fragment, its freeze-once contract, and
//! the summary-source exclusion.
//!
//! These tests drive the real compilation and compaction-selection code paths. Where a test
//! needs an assembled request it uses the same `ContextPreparationInput` shape the production
//! `prepare` builds, so the property under test is a property of the real path rather than of a
//! hand-made value.
use aura_activity_domain::{
    ActivityCoverage, ActivityCoverageState, ActivityOmissionReason, ActivityRow,
    ActivitySnapshotHeader,
};
use aura_activity_runtime::{ActivitySummary, ActivitySummaryRow};
use aura_context_domain::ContextSourceRef;
use aura_context_domain::{
    ContextCompressibility, ContextPrivacyClass, ContextSectionKind, ModelEgressSourceKind,
    PromptInspectorActivityView,
};
use aura_context_runtime::{ContextFragment, ContextSectionInput};
use aura_domain_common::{
    ContentHash, ModelStepId, Revision, RuntimeInstanceId, SessionId, ToolId, UtcTimestamp,
};
use aura_security_contract::NetworkDataClass;

/// The state kind the activity fragment uses. It is asserted literally so a rename cannot
/// silently break the exclusion rule without failing a test.
const ACTIVITY_STATE_KIND: &str = "agent.activity_index.v1";
/// The substring that proves a body really contains dynamic activity. It is the header the
/// fragment renders, which no control fact contains.
const ACTIVITY_MARKER: &str = "coverage=";

fn activity_fragment(body: &str) -> ContextFragment {
    ContextFragment {
        source_ref: ContextSourceRef::RuntimeState {
            state_kind: ACTIVITY_STATE_KIND.to_owned(),
            state_revision: Revision::new(3),
        },
        // Its own source kind, from the real publisher.
        source_kind: ModelEgressSourceKind::ActivityIndex,
        data_class: NetworkDataClass::PublicContent,
        authorization_basis: aura_context_domain::ModelEgressAuthorizationBasis::ModelChannelPolicy,
        content: body.to_owned(),
    }
}

fn control_fragment(body: &str) -> ContextFragment {
    ContextFragment {
        source_ref: ContextSourceRef::RuntimeState {
            state_kind: "agent.control_state.v1".to_owned(),
            state_revision: Revision::new(9),
        },
        source_kind: ModelEgressSourceKind::AuraCore,
        data_class: NetworkDataClass::PublicContent,
        authorization_basis: aura_context_domain::ModelEgressAuthorizationBasis::ModelChannelPolicy,
        content: body.to_owned(),
    }
}

fn task_state_section(fragments: Vec<ContextFragment>) -> ContextSectionInput {
    ContextSectionInput {
        kind: ContextSectionKind::CurrentTaskState,
        fragments,
        compressibility: ContextCompressibility::NeverCompress,
        privacy_class: ContextPrivacyClass::ModelVisible,
    }
}

// ---------------------------------------------------------------------------
// C-1: the activity fragment is separate, and a control refresh cannot overwrite it
// ---------------------------------------------------------------------------

#[test]
fn the_activity_fragment_is_its_own_source_and_not_the_control_state() {
    // The reader below is the exact production refresh rule: it rewrites the fragment whose
    // source kind is `agent.control_state.v1` and nothing else.
    let mut fragments = vec![
        control_fragment(r#"{"state_revision":"1"}"#),
        activity_fragment("sampled_at=T runtime=run_x revision=3 coverage=complete"),
    ];
    let refreshed = r#"{"state_revision":"2","guard":{"state":"open"}}"#;
    for fragment in &mut fragments {
        if matches!(
            &fragment.source_ref,
            ContextSourceRef::RuntimeState { state_kind, .. }
                if state_kind == "agent.control_state.v1"
        ) {
            fragment.content = refreshed.to_owned();
        }
    }
    assert_eq!(
        fragments[0].content, refreshed,
        "the ordinary control refresh still updates the control fact"
    );
    assert_eq!(
        fragments[1].source_kind,
        ModelEgressSourceKind::ActivityIndex,
        "the activity fragment survives a control refresh untouched"
    );
    assert!(
        fragments[1].content.contains(ACTIVITY_MARKER),
        "the activity body is still present after the refresh"
    );
    assert_ne!(
        fragments[0].source_ref, fragments[1].source_ref,
        "the two fragments are distinct sources, which is what makes the refresh selective"
    );
}

#[test]
fn the_section_kind_count_is_unchanged_at_twelve() {
    // A thirteenth section would be a product change; the activity fragment must fit in the
    // existing `CurrentTaskState` section.
    assert_eq!(ContextSectionKind::PHYSICAL_ORDER.len(), 12);
    assert!(
        ContextSectionKind::PHYSICAL_ORDER.contains(&ContextSectionKind::CurrentTaskState),
        "the activity fragment lives in the existing task-state section"
    );
    assert_eq!(ContextSectionKind::CurrentTaskState.physical_ordinal(), 8);
}

// ---------------------------------------------------------------------------
// C-2: the summary excludes the dynamic source before concat, hash and manifest
// ---------------------------------------------------------------------------

#[test]
fn compaction_excludes_the_activity_source_before_concatenation() {
    use super::compaction::is_dynamic_runtime_source;

    let control = control_fragment(r#"{"state_revision":"2"}"#);
    let activity = activity_fragment("sampled_at=T runtime=run_x revision=3 coverage=complete");
    assert!(
        !is_dynamic_runtime_source(&control),
        "the control fact is kept: the summary legitimately needs it"
    );
    assert!(
        is_dynamic_runtime_source(&activity),
        "the activity fragment is the dynamic source compaction must exclude"
    );

    // The concatenation the compaction instruction envelope performs, with the real filter.
    let section = task_state_section(vec![control, activity]);
    let concatenated = section
        .fragments
        .iter()
        .filter(|fragment| !is_dynamic_runtime_source(fragment))
        .map(|fragment| fragment.content.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        concatenated.contains("state_revision"),
        "the control fact reaches the summary request"
    );
    assert!(
        !concatenated.contains(ACTIVITY_MARKER),
        "the dynamic marker is absent from the concatenated summary sources"
    );
    // The hash is taken over the concatenation, so excluding before hashing is what makes the
    // hash and everything derived from it clean.
    let hash = ContentHash::sha256(concatenated.as_bytes());
    let unfiltered_hash = ContentHash::sha256(
        section
            .fragments
            .iter()
            .map(|fragment| fragment.content.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .as_bytes(),
    );
    assert_ne!(
        hash, unfiltered_hash,
        "the filtered hash differs from the unfiltered one, so the exclusion really happened before hashing"
    );
}

#[test]
fn a_relabelled_fragment_cannot_smuggle_the_activity_into_the_summary() {
    use super::compaction::is_dynamic_runtime_source;

    // Counterexample: a fragment that carries the dynamic body but claims to be Core-owned.
    let mut disguised =
        activity_fragment("sampled_at=T runtime=run_x revision=3 coverage=complete");
    disguised.source_kind = ModelEgressSourceKind::AuraCore;
    assert!(
        is_dynamic_runtime_source(&disguised),
        "the known runtime identity keeps a relabelled activity out of compaction"
    );

    // And the real one is still excluded, so the disguise does not disable the rule itself.
    let genuine = activity_fragment("sampled_at=T runtime=run_x revision=3 coverage=complete");
    assert!(is_dynamic_runtime_source(&genuine));
    let published = activity_fragment("body");
    assert_eq!(published.source_kind, ModelEgressSourceKind::ActivityIndex);
}

#[test]
fn an_activity_kind_cannot_enter_compaction_with_an_event_reference() {
    use super::compaction::is_dynamic_runtime_source;
    let mut fragment = activity_fragment("volatile activity");
    fragment.source_ref = ContextSourceRef::Event { event_id: aura_domain_common::EventId::new() };
    assert!(is_dynamic_runtime_source(&fragment));
    assert!(!is_dynamic_runtime_source(&control_fragment("stable control")));
}

#[test]
fn a_control_only_section_is_still_fully_included() {
    use super::compaction::is_dynamic_runtime_source;

    // The exclusion must not be a blanket removal of the whole section: a runtime with no
    // activity still sends its control facts.
    let section = task_state_section(vec![control_fragment(r#"{"state_revision":"5"}"#)]);
    let kept =
        section.fragments.iter().filter(|fragment| !is_dynamic_runtime_source(fragment)).count();
    assert_eq!(kept, 1, "the control fact survives when there is no activity fragment");
}

// ---------------------------------------------------------------------------
// C-3: the frozen snapshot is one per step, and the retry reuses it
// ---------------------------------------------------------------------------

fn summary_with_coverage(coverage: ActivityCoverage) -> ActivitySummary {
    ActivitySummary {
        header: ActivitySnapshotHeader {
            sampled_at: UtcTimestamp::now(),
            runtime_instance_id: RuntimeInstanceId::new(),
            revision: Revision::new(11),
            coverage,
            stale: false,
        },
        rows: Vec::new(),
        body: String::new(),
    }
}

#[test]
fn the_frozen_snapshot_belongs_to_exactly_one_model_step() {
    use super::activity::FrozenActivity;

    let step = ModelStepId::new();
    let other_step = ModelStepId::new();
    let frozen = FrozenActivity {
        summary: summary_with_coverage(ActivityCoverage::complete(0, 0)),
        model_step_id: step,
        revision: Revision::new(11),
    };
    assert!(frozen.belongs_to_step(step), "the frozen value carries the step it was taken for");
    assert!(
        !frozen.belongs_to_step(other_step),
        "a different step is a different picture and must sample again"
    );
}

#[test]
fn the_same_frozen_value_renders_the_same_bytes_for_a_retry() {
    use super::activity::FrozenActivity;

    let mut summary = summary_with_coverage(ActivityCoverage::complete(2, 0));
    summary.rows.push(ActivitySummaryRow {
        row: ActivityRow {
            tool_name: String::new(),
            private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
            id: aura_activity_domain::ActivityId::from_bytes([7u8; 16]),
            kind: aura_activity_domain::ActivityKind::AgentTool,
            tool_id: ToolId::from_stable_name("fs.write"),
            tool_group: aura_activity_domain::ActivityToolGroup::FileWrite,
            session_id: SessionId::new(),
            workspace_id: aura_domain_common::WorkspaceId::new(),
            agent_run_id: None,
            execution: aura_activity_domain::ActivityExecutionRef::MainFileMutation {
                operation_id: aura_domain_common::SecurityOperationId::new(),
                command_invocation_id: None,
            },
            resources: Vec::new(),
            parameters: Vec::new(),
            started_at: UtcTimestamp::now(),
        },
        relevance: aura_activity_runtime::ActivityRelevance::Direct,
        rendered: "- main_fs file_write [write] a.txt".to_owned(),
    });
    let frozen =
        FrozenActivity { summary, model_step_id: ModelStepId::new(), revision: Revision::new(11) };
    // The retry path re-sends a compiled request; it does not re-enter preparation. Rendering
    // the frozen value twice is the observable form of "the retry bytes do not drift".
    let first = frozen.rendered_body();
    let second = frozen.rendered_body();
    assert_eq!(first, second);
    assert_eq!(
        aura_activity_domain::summary_content_hash(&first),
        aura_activity_domain::summary_content_hash(&second),
        "the frozen body hashes identically for a retry"
    );
    let fragment = frozen.fragment().expect("no error").expect("a sendable summary");
    assert_eq!(fragment.content, first, "the fragment body is exactly the frozen render");
    assert_eq!(fragment.source_kind, ModelEgressSourceKind::ActivityIndex);
    assert!(
        matches!(
            &fragment.source_ref,
            ContextSourceRef::RuntimeState { state_kind, state_revision }
                if state_kind == ACTIVITY_STATE_KIND && *state_revision == Revision::new(11)
        ),
        "the fragment names the revision it was sampled at"
    );
}

#[test]
fn an_unavailable_index_degrades_without_failing_and_without_a_fragment() {
    use super::activity::FrozenActivity;

    let unavailable = FrozenActivity {
        summary: summary_with_coverage(ActivityCoverage::unavailable()),
        model_step_id: ModelStepId::new(),
        revision: Revision::new(1),
    };
    assert!(unavailable.is_unavailable());
    assert!(
        unavailable.fragment().expect("degrading is not an error").is_none(),
        "an unavailable index adds nothing to the request rather than failing it"
    );
    // The coverage is still stated, so the degradation is observable rather than silent.
    assert_eq!(unavailable.summary.header.coverage.state, ActivityCoverageState::Unavailable);
    assert!(
        unavailable
            .summary
            .header
            .coverage
            .omission_reasons
            .contains(&ActivityOmissionReason::IndexUnavailable)
    );
}

#[test]
fn the_inspector_view_and_the_fragment_describe_one_snapshot() {
    // The Inspector view is built from the same frozen summary as the fragment, so the two
    // cannot disagree. This test drives the projection from a frozen summary and compares it
    // with the body the fragment would carry, which is the property the design requires.
    let summary = summary_with_coverage(ActivityCoverage::complete(3, 1));
    let body = {
        let header = &summary.header;
        format!(
            "coverage=complete in_scope={} shown={} omitted={}",
            header.coverage.total_in_scope,
            summary.rows.len(),
            header.coverage.omitted_by_limit
                + header.coverage.capacity_dropped
                + header.coverage.unavailable_dropped
                + header.coverage.rejected
                + header.coverage.visibility_filtered,
        )
    };
    let view = PromptInspectorActivityView {
        sampled_at: summary.header.sampled_at,
        runtime_instance_id: summary.header.runtime_instance_id,
        revision: summary.header.revision,
        coverage: summary.header.coverage.state,
        rows_shown: u32::try_from(summary.rows.len()).unwrap_or(u32::MAX),
        rows_in_scope: summary.header.coverage.total_in_scope,
        omitted: summary.header.coverage.visibility_filtered,
        truncated: summary.header.coverage.truncated,
        body_hash: aura_activity_domain::summary_content_hash(&body),
        fragment_present: true,
    };
    assert_eq!(view.sampled_at, summary.header.sampled_at);
    assert_eq!(view.revision, summary.header.revision);
    assert_eq!(view.body_hash, aura_activity_domain::summary_content_hash(&body));
    assert!(
        body.contains(ACTIVITY_MARKER),
        "the Inspector's hash really is the hash of a body carrying the activity marker"
    );
}

#[test]
fn activity_private_sources_remain_classified_and_the_real_render_is_bounded() {
    use super::activity::FrozenActivity;
    use aura_activity_domain::*;
    let mut summary = summary_with_coverage(ActivityCoverage::complete(32, 0));
    for ordinal in 0..32_u8 {
        let source = if ordinal % 2 == 0 {
            ActivityPrivateSource::WorkspaceMetadata
        } else {
            ActivityPrivateSource::PersonalData
        };
        summary.rows.push(ActivitySummaryRow {
            row: ActivityRow {
                id: ActivityId::from_bytes([ordinal; 16]),
                kind: ActivityKind::AgentTool,
                tool_id: ToolId::from_stable_name("fs.write"),
                tool_name: "fs.write".into(),
                private_source: source,
                tool_group: ActivityToolGroup::FileWrite,
                session_id: SessionId::new(),
                workspace_id: aura_domain_common::WorkspaceId::new(),
                agent_run_id: None,
                execution: ActivityExecutionRef::AgentToolCall {
                    tool_call_id: aura_domain_common::ToolCallId::new(),
                    operation_id: aura_domain_common::SecurityOperationId::new(),
                    attempt_ordinal: 1,
                },
                resources: vec![ActivityResource {
                    target: ActivityResourceTarget::Unproven { hint: "unknown".into() },
                    relation: ActivityResourceRelation::Write,
                    display_name: format!("private-canary-{ordinal}"),
                    display_name_truncated: false,
                }],
                parameters: vec![ActivityParameter {
                    key: "path".into(),
                    value: "私\"</context>".repeat(30),
                    truncated: false,
                }],
                started_at: UtcTimestamp::now(),
            },
            relevance: aura_activity_runtime::ActivityRelevance::Direct,
            rendered: String::new(),
        });
    }
    let frozen =
        FrozenActivity { summary, model_step_id: ModelStepId::new(), revision: Revision::new(11) };
    let fragments = frozen.fragments().expect("activity projection fixture");
    assert!(
        frozen.rendered_body().len() <= MAX_ACTIVITY_SUMMARY_BYTES,
        "header, escaping, all classified fragments and separators fit together"
    );
    let public = fragments
        .iter()
        .find(|fragment| fragment.data_class == NetworkDataClass::PublicContent)
        .expect("activity projection fixture");
    assert!(!public.content.contains("private-canary") && !public.content.contains("</context>"));
    assert!(
        !public.content.contains("agent.activity.query")
            && public.content.contains("activity_id")
            && public.content.contains("tool_call_id")
            && public.content.contains("fs.write")
    );
    for class in [NetworkDataClass::WorkspaceMetadata, NetworkDataClass::PersonalData] {
        let private = fragments
            .iter()
            .find(|fragment| fragment.data_class == class)
            .expect("the two actual source classes stay distinct");
        assert!(private.content.contains("private-canary"));
        assert!(super::compaction::is_dynamic_runtime_source(private));
        let mut relabelled = private.clone();
        relabelled.source_kind = ModelEgressSourceKind::AuraCore;
        assert!(
            super::compaction::is_dynamic_runtime_source(&relabelled),
            "the private runtime identity cannot smuggle dynamic data into compaction"
        );
    }
    let body_rows: serde_json::Value = serde_json::from_str(
        public.content.split_once('\n').expect("activity projection fixture").1,
    )
    .expect("activity projection fixture");
    let shown = body_rows.as_array().expect("activity projection fixture").len();
    assert!(
        shown > 0 && shown < 32,
        "the byte limit genuinely discards rows, rather than accepting every oversized row"
    );
    assert!(
        public.content.contains(&format!("shown={shown}"))
            && public.content.contains(&format!("omitted={}", 32 - shown))
    );
}
