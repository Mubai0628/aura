use aura_memory_domain::MemoryWorkEvidence;

#[test]
fn only_successful_work_triggers_and_pure_qa_never_does() {
    let normal = MemoryWorkEvidence { normally_completed: true, ..Default::default() };
    assert!(normal.trigger().is_none());
    assert!(MemoryWorkEvidence { answer_characters: 50_000, ..normal }.trigger().is_none());
    assert!(
        MemoryWorkEvidence { tool_calls: 1, side_effect_calls: 1, ..normal }.trigger().is_some()
    );
    assert!(MemoryWorkEvidence { tool_calls: 8, ..normal }.trigger().is_some());
    assert!(
        MemoryWorkEvidence { tool_calls: 1, answer_characters: 2000, ..normal }.trigger().is_some()
    );
    assert!(
        MemoryWorkEvidence {
            tool_calls: 1,
            side_effect_calls: 1,
            normally_completed: false,
            ..normal
        }
        .trigger()
        .is_none()
    );
}

#[test]
fn a_maintenance_answer_in_one_code_fence_is_read_and_nothing_else_is_repaired() {
    use aura_memory_domain::parse_maintenance_output;
    let json = r#"{"schema_version":1,"candidates":[],"events":[]}"#;
    for answer in [
        json.to_owned(),
        format!("  {json}\n"),
        format!("```json\n{json}\n```"),
        format!("```\n{json}\n```\n"),
    ] {
        assert_eq!(parse_maintenance_output(&answer).expect(&answer).schema_version, 1);
    }
    for answer in [
        format!("Here is the result:\n```json\n{json}\n```"),
        format!("```json\n{json}"),
        format!("{json}\nDone."),
        r#"{"schema_version":1,"candidates":[],"events":[],"extra":true}"#.to_owned(),
    ] {
        assert!(parse_maintenance_output(&answer).is_err(), "{answer}");
    }
}
