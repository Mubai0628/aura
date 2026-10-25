use aura_domain_common::NormalizedIssueCode;

#[test]
fn an_unknown_broker_warning_survives_the_registered_wire_boundary() {
    let wire = "\"tool.outcome.unsettled\"";
    let warning: NormalizedIssueCode = serde_json::from_str(wire)
        .expect("the existing Broker Unknown warning must reach the model result");
    assert_eq!(serde_json::to_string(&warning).expect("wire"), wire);
    assert!(
        serde_json::from_str::<NormalizedIssueCode>("\"tool.outcome.other_unregistered\"").is_err()
    );
}
