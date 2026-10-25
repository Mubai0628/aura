use aura_agent_domain::*;
use aura_domain_common::*;

struct Fixture {
    user: OriginalUserMessage,
    turn: TurnTask,
    run: AgentRun,
    lineage: TaskLineage,
    step: ModelStepId,
}
impl Fixture {
    fn new() -> Self {
        let at = UtcTimestamp::now();
        let user = OriginalUserMessage::new(
            EventId::new(),
            SessionId::new(),
            "Complete the requested work".into(),
            UserMessageOrigin::UserInput,
            at,
        )
        .expect("user event");
        let turn_id = TurnTaskId::new();
        let run_id = AgentRunId::new();
        let lineage =
            TaskLineage::from_root(TaskLineageId::new(), &user, turn_id).expect("lineage");
        let step = ModelStepId::new();
        let turn = TurnTask {
            turn_task_id: turn_id,
            session_id: user.session_id,
            workspace_id: WorkspaceId::new(),
            root_user_event_id: user.event_id,
            lineage_id: lineage.lineage_id,
            lineage_resolution_required: false,
            agent_run_id: run_id,
            mode: AgentTaskMode::Normal,
            state: TurnTaskState::Running,
            revision: Revision::INITIAL,
            created_at: at,
            updated_at: at,
        };
        let run = AgentRun {
            agent_run_id: run_id,
            turn_task_id: turn_id,
            parent_agent_run_id: None,
            state: AgentRunState::ModelRunning { model_step_id: step },
            revision: Revision::INITIAL,
            next_model_ordinal: 1,
            prompt_bundle_snapshot_id: PromptBundleSnapshotId::new(),
            created_at: at,
            updated_at: at,
        };
        Self { user, turn, run, lineage, step }
    }
    fn review_input(&self) -> TaskLineageReviewCompletionInput {
        TaskLineageReviewCompletionInput {
            lineage_id: self.lineage.lineage_id,
            expected_lineage_revision: self.lineage.revision,
            source_read_receipt_id: None,
            assessment: CompletionAssessment::FullySatisfied,
            reviewed_user_intents: vec![ReviewedUserIntent {
                description: "Requested work".into(),
                source_event_ids: self.lineage.sources.iter().map(|s| s.user_event_id).collect(),
                result: ReviewedIntentResult::Satisfied,
                evidence_refs: vec![],
            }],
            evidence_refs: vec![],
        }
    }
    fn review(
        &mut self,
        input: TaskLineageReviewCompletionInput,
        receipt: Option<&SourceReadReceipt>,
    ) -> Result<CompletionReview, AgentError> {
        let context = CompletionContext {
            turn: &self.turn,
            run: &self.run,
            model_step_id: self.step,
            at: UtcTimestamp::now(),
        };
        self.lineage.review_completion(input, receipt, CompletionReviewId::new(), &context)
    }
    fn input(&self) -> NormalCompleteInput {
        NormalCompleteInput {
            lineage_id: self.lineage.lineage_id,
            expected_lineage_revision: self.lineage.revision,
            final_report: "The model has decided to stop.".into(),
            disclosed_limitations: vec![],
        }
    }
    fn complete(&self, input: &NormalCompleteInput) -> Result<NormalCompletionOutcome, AgentError> {
        self.lineage.check_normal_completion(
            input,
            &CompletionContext {
                turn: &self.turn,
                run: &self.run,
                model_step_id: self.step,
                at: UtcTimestamp::now(),
            },
        )
    }
}
#[test]
fn explicit_model_decision_needs_no_review_or_strict_quality_proof() {
    let mut f = Fixture::new();
    f.lineage.completion_policy =
        CompletionPolicy::StrictCompletionRequired { source_event_ids: vec![f.user.event_id] };
    f.lineage.source_recheck.compacted = true;
    assert_eq!(f.complete(&f.input()), Ok(NormalCompletionOutcome::ModelDecision));
}
#[test]
fn voluntary_not_ready_review_cannot_veto_a_later_completion_decision() {
    let mut f = Fixture::new();
    let mut input = f.review_input();
    input.assessment =
        CompletionAssessment::NotReady { remaining_work: vec![], next_actions: vec![] };
    f.review(input, None).expect("voluntary assessment");
    assert!(matches!(f.lineage.completion_gate, CompletionGate::ReviewedNotReady { .. }));
    assert_eq!(f.complete(&f.input()), Ok(NormalCompletionOutcome::ModelDecision));
}
#[test]
fn normal_completion_is_allowed_while_plan_or_goal_mode_stays_active() {
    for mode in [AgentTaskMode::Plan, AgentTaskMode::Goal] {
        let mut f = Fixture::new();
        f.turn.mode = mode;
        assert_eq!(f.complete(&f.input()), Ok(NormalCompletionOutcome::ModelDecision));
        let review = f.review_input();
        assert_eq!(
            f.review(review, None).map(|review| review.lineage_id),
            Ok(f.lineage.lineage_id)
        );
    }
}
#[test]
fn completion_still_rejects_foreign_identity_stale_revision_and_empty_answer() {
    let mut f = Fixture::new();
    let mut input = f.input();
    input.lineage_id = TaskLineageId::new();
    assert!(f.complete(&input).is_err());
    input = f.input();
    input.expected_lineage_revision = Revision::new(42);
    assert!(f.complete(&input).is_err());
    input = f.input();
    input.final_report.clear();
    assert!(f.complete(&input).is_err());
    f.run.turn_task_id = TurnTaskId::new();
    assert!(f.complete(&f.input()).is_err());
}
#[test]
fn voluntary_review_cannot_forge_a_source_receipt() {
    let mut f = Fixture::new();
    let mut input = f.review_input();
    input.source_read_receipt_id = Some(SourceReadReceiptId::new());
    assert_eq!(f.review(input, None), Err(AgentError::SourceReadRequired));
}
#[test]
fn old_completion_record_keeps_its_original_review_and_outcome() {
    let review = CompletionReviewId::new();
    let old = serde_json::json!({"record_id":NormalCompletionRecordId::new(),"turn_task_id":TurnTaskId::new(),"agent_run_id":AgentRunId::new(),"lineage_id":TaskLineageId::new(),"review_id":review,"outcome":"fully_satisfied","final_report":"old","disclosed_limitations":[],"at":UtcTimestamp::now()});
    let record: NormalCompletionRecord =
        serde_json::from_value(old.clone()).expect("historical record");
    assert_eq!(record.review_id, Some(review));
    assert_eq!(serde_json::to_value(record).expect("json"), old);
}
#[test]
fn malformed_unicode_excerpt_does_not_partially_attach_a_source() {
    let mut f = Fixture::new();
    let user = OriginalUserMessage::new(
        EventId::new(),
        f.user.session_id,
        "中文 source".into(),
        UserMessageOrigin::UserInput,
        UtcTimestamp::now(),
    )
    .expect("message");
    let before = f.lineage.clone();
    assert!(
        f.lineage
            .attach_source(
                &user,
                f.turn.turn_task_id,
                vec![UserMessageTextRange { start_utf8_byte: 1, end_utf8_byte: 3 }],
                f.lineage.revision,
                user.at
            )
            .is_err()
    );
    assert_eq!(f.lineage, before);
    assert_eq!(
        UserMessageExcerptSelector { exact_text: "中文".into(), occurrence: 0 }
            .resolve(&user.text)
            .expect("range"),
        UserMessageTextRange { start_utf8_byte: 0, end_utf8_byte: 6 }
    );
}
