//! What follows a failed compaction attempt is the factory's to decide, and the failure names
//! why an answer could not be used. A compaction asked after the step's request closes with its
//! question; one asked the isolated way carries its sources in its instructions and no question.
use std::{
    collections::VecDeque,
    future::Future,
    pin::Pin,
    sync::{
        Mutex,
        atomic::{AtomicU8, Ordering},
    },
};

use aura_context_domain::{
    CompactionAnchorKind, CompactionModelAttemptState, CompactionSourceSpan, ContextSourceRef,
    ModelEgressManifestV1, ModelEgressSourceKind, SourceAuthorizationBinding,
};
use aura_context_runtime::*;
use aura_domain_common::jcs::CanonicalHashV1;
use aura_domain_common::{
    AgentRunId, ContentHash, ContextProjectionSnapshotId, EndpointId, EventId, ModelContentBlockId,
    ModelExecutionSnapshotId, ModelProfileId, ModelStepId, PromptBundleSnapshotId, ProviderId,
    Revision, SessionId, SessionSequence, ToolRegistrySnapshotId, TransportAttemptId, UtcTimestamp,
};
use aura_model_domain::{
    AssistantContentBlock, CompiledInstructionEnvelope, EndpointAuthScheme,
    LogicalInstructionBlock, LogicalInstructionKind, LogicalModelRequest, ModelApiFormat,
    ModelExecutionSnapshot, ModelOutputFormat, ModelOutputRequest, ModelRefusal, ModelUsage,
    NormalizedModelOutput, NormalizedStopReason, NormalizedToolCall, ProviderToolCallId,
    ResolvedModelRequestOptions, UsageSource,
};
use aura_security_contract::NetworkDataClass;
use aura_tool_contract::{ToolRegistrySnapshotRef, ToolSchemaSetSnapshotV1};

const PARAGRAPH: &str = "The user asked for a summary; the files are written and the tests pass.";

fn operation() -> ContextCompactionOperationSpec {
    let anchor_manifest = build_anchor_manifest(vec![CompactionSourceDraft {
        kind: CompactionAnchorKind::Event,
        source_ref: ContextSourceRef::Event { event_id: EventId::new() },
        source_hash: ContentHash::sha256(b"event"),
        preview: "event".to_owned(),
    }])
    .expect("manifest");
    ContextCompactionOperationSpec {
        session_id: SessionId::new(),
        agent_run_id: AgentRunId::new(),
        model_profile_id: ModelProfileId::new(),
        source_context_snapshot_id: ContextProjectionSnapshotId::new(),
        sequence: 1,
        source_span: CompactionSourceSpan {
            start_sequence: SessionSequence::new(1),
            end_sequence: SessionSequence::new(1),
            source_refs: anchor_manifest.anchors.iter().map(|a| a.source_ref.clone()).collect(),
        },
        anchor_manifest,
        output_character_limit: 10_000,
        retained_recent_tokens: 0,
        retention_exceptions: Vec::new(),
    }
}

fn attempt(
    operation: &ContextCompactionOperationSpec,
    ordinal: u8,
    shape: CompactionRequestShape,
    question: Option<&str>,
) -> PreparedCompactionModelAttempt {
    let blocks = [
        LogicalInstructionKind::Core,
        LogicalInstructionKind::Permission,
        LogicalInstructionKind::ToolSummary,
        LogicalInstructionKind::WorkspaceAgents,
        LogicalInstructionKind::HistoricalSnapshot,
        LogicalInstructionKind::AgentMode,
        LogicalInstructionKind::Hook,
        LogicalInstructionKind::TaskState,
    ]
    .into_iter()
    .map(|kind| {
        let content = format!("block-{kind:?}");
        LogicalInstructionBlock {
            kind,
            content_hash: ContentHash::sha256(content.as_bytes()),
            content,
            source_event_ids: Vec::new(),
        }
    })
    .collect::<Vec<_>>();
    let canonical = blocks.iter().map(|b| b.content.as_str()).collect::<Vec<_>>().join("\n");
    let context_snapshot_id = ContextProjectionSnapshotId::new();
    let model_step_id = ModelStepId::new();
    let execution_snapshot_id = ModelExecutionSnapshotId::new();
    let logical_request = LogicalModelRequest {
        model_step_id,
        execution_snapshot_id,
        context_snapshot_id,
        instruction_envelope: CompiledInstructionEnvelope::new(1, blocks, canonical)
            .expect("instruction envelope"),
        conversation_items: Vec::new(),
        fixed_tool_ids: Vec::new(),
        tool_schema_set: ToolSchemaSetSnapshotV1::new(
            ToolRegistrySnapshotRef {
                tool_registry_snapshot_id: ToolRegistrySnapshotId::new(),
                registry_revision: Revision::INITIAL,
                content_hash: CanonicalHashV1::sha256(b"compaction-attempt-registry"),
            },
            Vec::new(),
        )
        .expect("schema set"),
        attachments: Vec::new(),
        provider_continuation: None,
        output_request: ModelOutputRequest {
            format: ModelOutputFormat::Text,
            effective_max_output_tokens: 1_024,
        },
        closing_question: question.map(ToOwned::to_owned),
        closing_exchange: Vec::new(),
    };
    let execution_snapshot = ModelExecutionSnapshot {
        id: execution_snapshot_id,
        model_step_id,
        provider_id: ProviderId::new(),
        endpoint_id: EndpointId::new(),
        model_profile_id: operation.model_profile_id,
        provider_display_name: "Fixture".to_owned(),
        endpoint_display_name: "Fixture".to_owned(),
        model_name: "fixture-model".to_owned(),
        readable_ref: "Fixture / fixture-model".to_owned(),
        api_format: ModelApiFormat::OpenAiResponses,
        auth_scheme: EndpointAuthScheme::None,
        base_url: "https://api.example.test/v1".to_owned(),
        resolved_request_url: "https://api.example.test/v1/responses".to_owned(),
        openai_organization_id: None,
        openai_project_id: None,
        anthropic_workspace_id: None,
        chat_output_token_field: None,
        chat_instruction_role: None,
        endpoint_config_revision: 1,
        model_config_revision: 1,
        runtime_effective_window_tokens: 32_768,
        historical_snapshot_max_tokens: 1_024,
        request_options: ResolvedModelRequestOptions::output_only(1_024),
        historical_tool_call_wire_bindings: Vec::new(),
        credential_id: None,
        credential_revision: None,
        credential_binding_fingerprint: None,
        context_snapshot_id,
        prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
        endpoint_fingerprint: ContentHash::sha256(b"compaction-attempt-endpoint"),
        created_at: UtcTimestamp::now(),
    };
    PreparedCompactionModelAttempt {
        attempt_ordinal: ordinal,
        logical_request,
        execution_snapshot,
        egress_manifest: ModelEgressManifestV1::new(context_snapshot_id, Vec::new())
            .expect("egress manifest"),
        shape,
    }
}

/// Attempts on the step's request, its question closing it; `again` answers `asks_again`.
struct Factory {
    shape: CompactionRequestShape,
    again: Option<bool>,
    asked: Mutex<Vec<FailedCompactionAttempt>>,
}

impl Factory {
    fn new(shape: CompactionRequestShape, again: Option<bool>) -> Self {
        Self { shape, again, asked: Mutex::new(Vec::new()) }
    }
}

impl CompactionAttemptFactory for Factory {
    fn prepare_attempt<'a>(
        &'a self,
        operation: &'a ContextCompactionOperationSpec,
        ordinal: u8,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<PreparedCompactionModelAttempt, CompactionExecutionFailure>>
                + Send
                + 'a,
        >,
    > {
        let question = (self.shape == CompactionRequestShape::SessionPrefix)
            .then_some("Compaction request: write the summary.");
        Box::pin(async move { Ok(attempt(operation, ordinal, self.shape, question)) })
    }

    fn asks_again(
        &self,
        failed: &PreparedCompactionModelAttempt,
        how: FailedCompactionAttempt,
    ) -> bool {
        let mut asked = self.asked.lock().expect("asked");
        assert_eq!(usize::from(failed.attempt_ordinal), asked.len() + 1);
        asked.push(how);
        match (self.again, how) {
            (Some(again), _) => again,
            (None, FailedCompactionAttempt::UnusableAnswer { .. }) => true,
            (None, FailedCompactionAttempt::NoAnswer { retryable }) => retryable,
        }
    }
}

/// Attempts asked the isolated way that leave `asks_again` to the trait.
struct ByDefault;

impl CompactionAttemptFactory for ByDefault {
    fn prepare_attempt<'a>(
        &'a self,
        operation: &'a ContextCompactionOperationSpec,
        ordinal: u8,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<PreparedCompactionModelAttempt, CompactionExecutionFailure>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(
            async move { Ok(attempt(operation, ordinal, CompactionRequestShape::Isolated, None)) },
        )
    }
}

/// An attempt whose request does not match its shape.
struct Mismatched(CompactionRequestShape, Option<&'static str>);

impl CompactionAttemptFactory for Mismatched {
    fn prepare_attempt<'a>(
        &'a self,
        operation: &'a ContextCompactionOperationSpec,
        ordinal: u8,
    ) -> Pin<
        Box<
            dyn Future<Output = Result<PreparedCompactionModelAttempt, CompactionExecutionFailure>>
                + Send
                + 'a,
        >,
    > {
        Box::pin(async move { Ok(attempt(operation, ordinal, self.0, self.1)) })
    }
}

fn answer(text: &str, stop_reason: NormalizedStopReason) -> NormalizedModelOutput {
    NormalizedModelOutput {
        model_step_id: ModelStepId::new(),
        assistant_blocks: if text.is_empty() {
            Vec::new()
        } else {
            vec![AssistantContentBlock {
                block_id: ModelContentBlockId::new(),
                text: text.to_owned(),
                provider_reference: None,
            }]
        },
        reasoning_blocks: Vec::new(),
        tool_calls: Vec::new(),
        refusal: None,
        stop_reason,
        usage: ModelUsage {
            input_tokens: 1,
            output_tokens: 1,
            total_tokens: 2,
            reasoning_tokens: None,
            cached_input_tokens: None,
            cache_write_tokens: None,
            source: UsageSource::LocalEstimated,
        },
        provider_continuation: None,
        provider_request_id: None,
        raw_response_ref: None,
    }
}

fn answered(output: NormalizedModelOutput) -> CompactionModelCallTerminal {
    CompactionModelCallTerminal::Completed {
        output: Box::new(output),
        transport_attempt_id: TransportAttemptId::new(),
    }
}

fn failed(retryable: bool) -> CompactionModelCallTerminal {
    CompactionModelCallTerminal::Failed {
        error_code: "transport.request_timeout".to_owned(),
        transport_attempt_id: Some(TransportAttemptId::new()),
        retryable,
    }
}

/// Answers each call in turn, the answer bound to the attempt's step.
struct Caller {
    calls: AtomicU8,
    terminals: Mutex<VecDeque<CompactionModelCallTerminal>>,
}

impl Caller {
    fn new(terminals: Vec<CompactionModelCallTerminal>) -> Self {
        Self { calls: AtomicU8::new(0), terminals: Mutex::new(terminals.into()) }
    }
}

impl CompactionModelCaller for Caller {
    fn call<'a>(
        &'a self,
        attempt: &'a PreparedCompactionModelAttempt,
    ) -> Pin<Box<dyn Future<Output = CompactionModelCallTerminal> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let terminal = self.terminals.lock().expect("terminals").pop_front().expect("a terminal");
        let terminal = match terminal {
            CompactionModelCallTerminal::Completed { output, transport_attempt_id } => {
                CompactionModelCallTerminal::Completed {
                    output: Box::new(NormalizedModelOutput {
                        model_step_id: attempt.logical_request.model_step_id,
                        ..*output
                    }),
                    transport_attempt_id,
                }
            }
            other => other,
        };
        Box::pin(async move { terminal })
    }
}

#[derive(Default)]
struct Sink {
    retried: Mutex<Vec<String>>,
    ended: Mutex<Vec<(CompactionModelAttemptState, String)>>,
    completed: Mutex<Vec<u8>>,
    summaries: Mutex<Vec<String>>,
}

impl CompactionLifecycleSink for Sink {
    fn mark_attempt_running<'a>(
        &'a self,
        _: &'a ContextCompactionOperationSpec,
        _: &'a PreparedCompactionModelAttempt,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async { Ok(()) })
    }

    fn finish_retryable_attempt<'a>(
        &'a self,
        failure: &'a CompactionAttemptFailureCommit,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        self.retried.lock().expect("retried").push(failure.error_code.clone());
        Box::pin(async { Ok(()) })
    }

    fn commit_completed<'a>(
        &'a self,
        completed: &'a ValidatedCompactionCommit,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        self.completed.lock().expect("completed").push(completed.attempt.attempt_ordinal);
        self.summaries
            .lock()
            .expect("summaries")
            .push(completed.parsed.historical_snapshot.clone());
        Box::pin(async { Ok(()) })
    }

    fn finish_operation<'a>(
        &'a self,
        failure: &'a CompactionAttemptFailureCommit,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        self.ended.lock().expect("ended").push((failure.state, failure.error_code.clone()));
        Box::pin(async { Ok(()) })
    }
}

struct Facts;

impl ModelEgressAuthorizationFacts for Facts {
    fn session_model_channel_is_authorized(&self, _: SessionId, _: ModelProfileId) -> bool {
        true
    }

    async fn persisted_event(
        &self,
        _: EventId,
    ) -> Result<Option<PersistedContextEventAuthorizationFact>, ModelEgressAuthorizationError> {
        Ok(None)
    }

    fn source_grant_binding_is_valid(
        &self,
        _: SessionId,
        _: &ContextSourceRef,
        _: ModelEgressSourceKind,
        _: NetworkDataClass,
        _: &SourceAuthorizationBinding,
    ) -> Result<bool, ModelEgressAuthorizationError> {
        Ok(false)
    }
}

fn failure_code(outcome: &ContextCompactionExecutionOutcome) -> &str {
    match outcome {
        ContextCompactionExecutionOutcome::Failed { failure, .. } => &failure.code,
        other => panic!("a failed compaction: {other:?}"),
    }
}

/// An answer that calls `fs.write`, with `text` beside the call.
fn calling(text: &str) -> NormalizedModelOutput {
    let mut called = answer(text, NormalizedStopReason::ToolCalls);
    called.tool_calls = vec![NormalizedToolCall {
        tool_call_id: aura_domain_common::ToolCallId::new(),
        provider_call_id: ProviderToolCallId::new("call_1").expect("provider call id"),
        tool_name: "fs.write".to_owned(),
        arguments_json: "{}".to_owned(),
        position: 0,
    }];
    called
}

#[tokio::test]
async fn an_answer_the_step_request_cannot_give_is_not_asked_for_again_when_the_factory_declines() {
    let factory = Factory::new(CompactionRequestShape::SessionPrefix, Some(false));
    let caller = Caller::new(vec![answered(calling(""))]);
    let sink = Sink::default();
    let outcome = execute_context_compaction(operation(), &factory, &caller, &sink, &Facts).await;
    assert_eq!(failure_code(&outcome), "context.compaction.model_output_invalid.tool_calls");
    assert_eq!(caller.calls.load(Ordering::SeqCst), 1, "asked once");
    assert_eq!(
        *factory.asked.lock().expect("asked"),
        [FailedCompactionAttempt::UnusableAnswer { why: UnusableAnswer::ToolCalls }]
    );
    assert!(sink.retried.lock().expect("retried").is_empty());
    assert_eq!(sink.ended.lock().expect("ended").len(), 1);
}

/// The factory is told what kind of answer it was - one that called tools, or another it cannot
/// use - so it can ask the next attempt to suit.
#[tokio::test]
async fn an_unusable_answer_names_why_and_what_kind_it_was() {
    let mut refused = answer(PARAGRAPH, NormalizedStopReason::EndTurn);
    refused.refusal = Some(ModelRefusal { message: "no".to_owned(), provider_code: None });
    for (output, code, why) in [
        (refused, "context.compaction.model_output_invalid.refusal", UnusableAnswer::Other),
        (
            answer(PARAGRAPH, NormalizedStopReason::MaxOutputTokens),
            "context.compaction.model_output_invalid.stop_reason.max_output_tokens",
            UnusableAnswer::Other,
        ),
        (
            answer("", NormalizedStopReason::EndTurn),
            "context.compaction.model_output_invalid.no_text",
            UnusableAnswer::Other,
        ),
        (
            calling("Let me read the file first."),
            "context.compaction.model_output_invalid.tool_calls",
            UnusableAnswer::ToolCalls,
        ),
    ] {
        let factory = Factory::new(CompactionRequestShape::SessionPrefix, Some(false));
        let caller = Caller::new(vec![answered(output)]);
        let outcome =
            execute_context_compaction(operation(), &factory, &caller, &Sink::default(), &Facts)
                .await;
        assert_eq!(failure_code(&outcome), code);
        assert_eq!(
            *factory.asked.lock().expect("asked"),
            [FailedCompactionAttempt::UnusableAnswer { why }],
            "{code}"
        );
    }
}

/// The summary the ask puts between its tags is the answer, whatever else came with it: a call
/// made after it, or an end cut off once it was written.
#[tokio::test]
async fn a_summary_between_its_tags_is_used_beside_a_tool_call_or_a_cut_off_end() {
    let tagged = format!("Here it is.\n<summary>{PARAGRAPH}</summary>\nNow I will check.");
    for output in [
        calling(&tagged),
        answer(&format!("{tagged} And then"), NormalizedStopReason::MaxOutputTokens),
    ] {
        let factory = Factory::new(CompactionRequestShape::SessionPrefix, Some(false));
        let caller = Caller::new(vec![answered(output)]);
        let sink = Sink::default();
        let outcome =
            execute_context_compaction(operation(), &factory, &caller, &sink, &Facts).await;
        assert!(
            matches!(
                outcome,
                ContextCompactionExecutionOutcome::Completed { attempt_ordinal: 1, .. }
            ),
            "{outcome:?}"
        );
        assert_eq!(*sink.summaries.lock().expect("summaries"), [PARAGRAPH]);
    }
}

/// A factory may ask again after each failure, up to the limit of attempts one compaction has.
#[tokio::test]
async fn a_factory_may_ask_again_until_the_attempt_limit() {
    let factory = Factory::new(CompactionRequestShape::SessionPrefix, Some(true));
    let limit = usize::from(aura_context_domain::MAX_COMPACTION_ATTEMPTS);
    let caller = Caller::new((0..limit).map(|_| failed(true)).collect());
    let sink = Sink::default();
    let outcome = execute_context_compaction(operation(), &factory, &caller, &sink, &Facts).await;
    assert!(
        matches!(
            &outcome,
            ContextCompactionExecutionOutcome::Failed { completed_attempts, .. }
                if usize::from(*completed_attempts) == limit
        ),
        "{outcome:?}"
    );
    assert_eq!(caller.calls.load(Ordering::SeqCst) as usize, limit);
    assert_eq!(factory.asked.lock().expect("asked").len(), limit - 1, "not after the last");
    assert_eq!(sink.retried.lock().expect("retried").len(), limit - 1);
}

/// A factory that leaves the decision to the trait asks again once, as before attempts could
/// follow one another.
#[tokio::test]
async fn by_default_a_compaction_is_asked_again_once() {
    let caller = Caller::new(vec![failed(true), failed(true), failed(true)]);
    let outcome =
        execute_context_compaction(operation(), &ByDefault, &caller, &Sink::default(), &Facts)
            .await;
    assert_eq!(failure_code(&outcome), "transport.request_timeout");
    assert_eq!(caller.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn by_default_an_unusable_answer_and_a_retryable_failure_are_asked_again_once() {
    for (first, retried) in [
        (answered(answer("", NormalizedStopReason::EndTurn)), true),
        (failed(true), true),
        (failed(false), false),
    ] {
        let factory = Factory::new(CompactionRequestShape::SessionPrefix, None);
        let caller =
            Caller::new(vec![first, answered(answer(PARAGRAPH, NormalizedStopReason::EndTurn))]);
        let sink = Sink::default();
        let outcome =
            execute_context_compaction(operation(), &factory, &caller, &sink, &Facts).await;
        let calls = caller.calls.load(Ordering::SeqCst);
        if retried {
            assert!(matches!(
                outcome,
                ContextCompactionExecutionOutcome::Completed { attempt_ordinal: 2, .. }
            ));
            assert_eq!((calls, sink.completed.lock().expect("completed").clone()), (2, vec![2]));
        } else {
            assert_eq!(failure_code(&outcome), "transport.request_timeout");
            assert_eq!(calls, 1);
        }
    }
}

#[tokio::test]
async fn an_attempt_on_the_step_request_closes_with_its_question_and_an_isolated_one_has_none() {
    for factory in [
        Mismatched(CompactionRequestShape::SessionPrefix, None),
        Mismatched(CompactionRequestShape::Isolated, Some("Compaction request: summarize.")),
    ] {
        let caller = Caller::new(vec![]);
        let outcome =
            execute_context_compaction(operation(), &factory, &caller, &Sink::default(), &Facts)
                .await;
        assert_eq!(failure_code(&outcome), "context.compaction.attempt_invalid");
        assert_eq!(caller.calls.load(Ordering::SeqCst), 0, "nothing was sent");
    }
}
