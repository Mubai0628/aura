use super::*;

#[test]
fn canonical_schema_validation_does_not_coerce_arguments() {
    let fixture = Fixture::new();
    let call = fixture.call(0, 1, ready(json!({"value": 7})));
    let batch = fixture.batch(vec![call]);

    let ToolBatchAdmissionOutcomeV1::Admitted(admitted) =
        admit_tool_batch(batch, &fixture.registry, fixture.definitions, &AllowAuthority)
            .expect("admission")
    else {
        panic!("batch should remain pairable")
    };
    assert!(admitted.validated_calls.is_empty());
    assert_eq!(admitted.not_executed[0].reason, ToolNotExecutedReasonV1::SchemaValidationFailed);
    assert!(admitted.not_executed[0].schema_validation.is_some());
}

/// A real business-schema failure is still a business-schema failure.
#[test]
fn a_business_schema_error_keeps_its_report_and_names_no_envelope_issue() {
    let fixture = Fixture::new();
    let call = fixture.call(0, 1, ready(json!({"value": 7})));
    let batch = fixture.batch(vec![call]);
    let ToolBatchAdmissionOutcomeV1::Admitted(admitted) =
        admit_tool_batch(batch, &fixture.registry, fixture.definitions, &AllowAuthority)
            .expect("admission")
    else {
        panic!("batch should remain pairable")
    };
    let rejection = &admitted.not_executed[0];
    assert!(rejection.schema_validation.is_some());
    assert_eq!(rejection.input_issue, None);
    assert_eq!(rejection.input_diagnostic, None);
}
