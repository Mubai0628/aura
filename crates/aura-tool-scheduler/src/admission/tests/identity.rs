use super::*;

#[test]
fn duplicate_tool_call_identity_fault_never_synthesizes_results() {
    let fixture = Fixture::new();
    let first = fixture.call(0, 1, ready(json!({"value": "first"})));
    let mut second = fixture.call(1, 2, ready(json!({"value": "second"})));
    second.tool_call_id = first.tool_call_id;
    let batch = fixture.batch(vec![first, second]);

    let outcome = admit_tool_batch(batch, &fixture.registry, fixture.definitions, &AllowAuthority)
        .expect("admission");
    assert_eq!(
        outcome,
        ToolBatchAdmissionOutcomeV1::IdentityIntegrityFault {
            code: ToolBatchErrorCode::ToolCallIdentityConflict
        }
    );
}

#[test]
fn invalid_call_is_rejected_without_blocking_an_independent_valid_call() {
    let fixture = Fixture::new();
    let calls = vec![
        fixture.call(
            0,
            1,
            NormalizedToolCallInputStateV1::InvalidInput {
                issue: NormalizedToolCallInputIssueV1::PurposeMissing,
                detail: None,
            },
        ),
        fixture.call(1, 2, ready(json!({"value": "second"}))),
    ];
    let batch = fixture.batch(calls);

    let ToolBatchAdmissionOutcomeV1::Admitted(admitted) =
        admit_tool_batch(batch, &fixture.registry, fixture.definitions, &AllowAuthority)
            .expect("admission")
    else {
        panic!("batch should be admitted")
    };
    assert_eq!(admitted.validated_calls.len(), 1);
    assert_eq!(admitted.validated_calls[0].call.ordinal, 2);
    assert_eq!(admitted.not_executed.len(), 1);
    assert_eq!(admitted.not_executed[0].ordinal, 1);
    assert_eq!(admitted.not_executed[0].reason, ToolNotExecutedReasonV1::PurposeRequired);
    // A rejected call carries the real classification so the model can act on
    // it, rather than being flattened into a generic "not executed" with no
    // error code. The purpose reason above stays the primary category.
    assert_eq!(
        admitted.not_executed[0].error_code,
        Some(aura_tool_contract::ToolSchemaErrorCode::ArgumentsInvalid.error_code())
    );
    validate_unique_admitted_results(&admitted).expect("unique result identities");
}

#[test]
fn p08_authority_failure_rejects_each_unique_call_before_dispatch() {
    struct DenyAuthority;
    impl BatchAdmissionAuthorityV1 for DenyAuthority {
        fn verify_batch_admission(
            &self,
            _batch: &ToolBatchRequestV1,
        ) -> Result<(), BatchAdmissionAuthorityIssueV1> {
            Err(BatchAdmissionAuthorityIssueV1::ControlStateMismatch)
        }
    }

    let fixture = Fixture::new();
    let call = fixture.call(0, 1, ready(json!({"value": "first"})));
    let batch = fixture.batch(vec![call]);
    let outcome = admit_tool_batch(batch, &fixture.registry, fixture.definitions, &DenyAuthority)
        .expect("admission");
    let ToolBatchAdmissionOutcomeV1::BatchRejected { code, calls } = outcome else {
        panic!("batch should be safely rejected")
    };
    assert_eq!(code, ToolBatchErrorCode::ControlStateMismatch);
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].reason, ToolNotExecutedReasonV1::BatchRejected);
}
