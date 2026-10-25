use super::*;

#[test]
fn an_unbound_name_does_not_suppress_a_valid_sibling_or_create_a_security_source() {
    let f = Fixture::new();
    let mut unknown = f.call(0, 1, ready(serde_json::json!({"value":"unknown"})));
    unknown.definition_ref = None;
    let good = f.call(1, 2, ready(serde_json::json!({"value":"known"})));
    let batch = f.batch(vec![unknown.clone(), good.clone()]);
    let admitted = admit_tool_batch(batch, &f.registry, f.definitions.clone(), &AllowAuthority)
        .expect("admission");
    let ToolBatchAdmissionOutcomeV1::Admitted(admitted) = admitted else { panic!("admitted") };
    assert_eq!(admitted.not_executed.len(), 1);
    assert_eq!(admitted.not_executed[0].reason, ToolNotExecutedReasonV1::ToolNotFound);
    assert_eq!(admitted.validated_calls.len(), 1);
    assert_eq!(admitted.validated_calls[0].call, good);
    let mut forged = admitted.validated_calls[0].clone();
    forged.call = unknown;
    assert!(security_source_for_admitted_tool_call_v1(&forged, &f.registry).is_err());
}
