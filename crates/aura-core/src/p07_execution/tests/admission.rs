use super::*;

#[test]
fn admission_plan_and_projection_are_prepared_without_an_attempt() {
    let fixture = Fixture::new();
    let call = fixture.call(json!({"value": "ready"}));
    let call_id = call.tool_call_id;
    let batch = fixture.batch(vec![call]);
    let outcome = fixture.prepare(
        batch,
        vec![FrozenToolCallPlanningInputV1 {
            tool_call_id: call_id,
            instance: fixture.instance.clone(),
            resources: ToolCallResourcePreflightV1::Resolved(
                aura_tool_scheduler::ResolvedPackageResourceSetV1 {
                    scheduler_claims: Vec::new(),
                    permission_scopes: Vec::new(),
                    risk_flags: Vec::new(),
                    resolved_directories: Default::default(),
                },
            ),
            runtime_required_scopes: Vec::new(),
            resolver_additional_scopes: Vec::new(),
        }],
    );
    let ToolBatchPreparationOutcomeV1::Prepared(prepared) = outcome else {
        panic!("valid batch must be prepared");
    };
    assert_eq!(prepared.planned.execution_plan.calls.len(), 1);
    assert!(prepared.commit_request.not_executed_results.is_empty());
}

#[test]
fn schema_rejection_gets_a_stable_error_and_no_execution_plan_call() {
    let fixture = Fixture::new();
    let call = fixture.call(json!({"value": 7}));
    let outcome = fixture.prepare(fixture.batch(vec![call]), Vec::new());
    let ToolBatchPreparationOutcomeV1::Prepared(prepared) = outcome else {
        panic!("per-call validation is an admitted batch with NotExecuted result");
    };
    assert!(prepared.planned.execution_plan.calls.is_empty());
    let result = &prepared.commit_request.not_executed_results[0];
    let aura_tool_contract::ToolResultOutcomeV1::NotExecuted { reason, error } = &result.outcome
    else {
        panic!("schema rejection must be NotExecuted");
    };
    assert_eq!(*reason, ToolNotExecutedReasonV1::SchemaValidationFailed);
    assert_eq!(
        error.as_ref().expect("stable error").code.as_str(),
        "tool.schema.arguments_invalid"
    );
}

#[test]
fn duplicate_aura_call_identity_never_synthesizes_results() {
    let fixture = Fixture::new();
    let first = fixture.call(json!({"value": "first"}));
    let mut second = fixture.call(json!({"value": "second"}));
    second.tool_call_id = first.tool_call_id;
    second.ordinal = 2;
    let outcome = fixture.prepare(fixture.batch(vec![first, second]), Vec::new());
    assert!(matches!(
        outcome,
        ToolBatchPreparationOutcomeV1::IdentityIntegrityFault {
            code: ToolBatchErrorCode::ToolCallIdentityConflict
        }
    ));
}
