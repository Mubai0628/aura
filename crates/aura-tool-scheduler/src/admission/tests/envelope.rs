use super::*;

/// The observed envelope mistakes reach the rejection with their location.
///
/// The three shapes are the ones the user's Shell calls produced: business fields
/// nested inside `_aura`, a nested `arguments` inside `_aura`, and an extra top-level
/// field. The preflight is where the coarse reason used to be produced, and the fact
/// that has to survive to the correction message is *which field is in the wrong
/// place* — so this asserts the issue and its bounded diagnostic travel with the
/// rejection, and that an envelope mistake is not reported as a business-schema
/// failure.
#[test]
fn an_envelope_mistake_carries_its_location_instead_of_a_generic_schema_failure() {
    use aura_tool_contract::{
        NormalizedToolCallInputDiagnosticV1 as Diagnostic, NormalizedToolCallInputIssueV1 as Issue,
        NormalizedToolCallInputStateV1 as State,
    };
    for (label, issue, pointer, fields) in [
        (
            "fields nested inside _aura",
            Issue::AuraMetadataUnexpectedField,
            "/_aura",
            vec!["limit".to_owned(), "path".to_owned()],
        ),
        (
            "a nested arguments object",
            Issue::AuraMetadataUnexpectedField,
            "/_aura",
            vec!["arguments".to_owned()],
        ),
        ("an extra top-level field", Issue::EnvelopeUnexpectedField, "$", vec!["extra".to_owned()]),
    ] {
        // A fresh fixture per case: admission consumes the definition set it is
        // given, exactly as the production caller hands over one registry snapshot.
        let fixture = Fixture::new();
        let call = fixture.call(
            0,
            1,
            State::invalid_input_at(issue, Diagnostic::new(pointer, fields.clone())),
        );
        let batch = fixture.batch(vec![call]);
        let ToolBatchAdmissionOutcomeV1::Admitted(admitted) =
            admit_tool_batch(batch, &fixture.registry, fixture.definitions, &AllowAuthority)
                .expect("admission")
        else {
            panic!("{label}: the batch stays pairable")
        };
        assert!(admitted.validated_calls.is_empty(), "{label}: nothing was validated");
        let rejection = &admitted.not_executed[0];
        assert_eq!(rejection.input_issue, Some(issue), "{label}");
        let diagnostic = rejection.input_diagnostic.as_ref().expect(label);
        assert_eq!(diagnostic.pointer, pointer, "{label}");
        assert_eq!(diagnostic.fields, fields, "{label}");
        assert!(
            rejection.schema_validation.is_none(),
            "{label}: nothing was validated against the tool schema, so reporting a \
             business-schema failure would send the model to the wrong field"
        );
    }
}

/// The envelope diagnosis belongs to the envelope, not to a tool.
///
/// Fixing this per tool would mean a file tool explains the mistake and a
/// non-file tool falls back to the generic sentence, so the two are compared
/// here over schemas that disagree: the same envelope mistake must carry the
/// same issue and location for both, while each tool's own business schema
/// stays the only authority on its own fields.
#[test]
fn the_envelope_diagnosis_is_shared_by_a_file_tool_and_a_non_file_tool() {
    use aura_tool_contract::{
        NormalizedToolCallInputDiagnosticV1 as Diagnostic, NormalizedToolCallInputIssueV1 as Issue,
        NormalizedToolCallInputStateV1 as State,
    };
    let fixture = Fixture::with_definitions(vec![
        definition_with_schema(
            3,
            "fs.read",
            json!({
                "type": "object",
                "properties": {"path": {"type": "string"}},
                "required": ["path"],
                "additionalProperties": false
            }),
        ),
        definition_with_schema(
            4,
            "session.rename",
            json!({
                "type": "object",
                "properties": {"title": {"type": "string"}},
                "required": ["title"],
                "additionalProperties": false
            }),
        ),
    ]);

    // The same envelope mistake, once per tool: a file field left inside `_aura`.
    let issue = Issue::AuraMetadataUnexpectedField;
    for index in 0..2 {
        let tool_id = fixture.definitions[index].tool_id;
        let call = fixture.call(
            index,
            1,
            State::invalid_input_at(issue, Diagnostic::new("/_aura", vec!["path".to_owned()])),
        );
        let batch = fixture.batch(vec![call]);
        let ToolBatchAdmissionOutcomeV1::Admitted(admitted) = admit_tool_batch(
            batch,
            &fixture.registry,
            fixture.definitions.clone(),
            &AllowAuthority,
        )
        .expect("admission") else {
            panic!("{tool_id}: the batch stays pairable")
        };
        let rejection = &admitted.not_executed[0];
        assert_eq!(rejection.input_issue, Some(issue), "{tool_id}");
        let diagnostic = rejection.input_diagnostic.as_ref().expect("diagnostic");
        assert_eq!(diagnostic.pointer, "/_aura", "{tool_id}");
        assert_eq!(diagnostic.fields, vec!["path".to_owned()], "{tool_id}");
        assert!(rejection.schema_validation.is_none(), "{tool_id}");
    }

    // A real business mistake stays tool-specific: each report names the field of
    // its own schema and neither is dressed up as an envelope problem.
    for (index, missing, present) in [(0usize, "path", "title"), (1, "title", "path")] {
        let tool_id = fixture.definitions[index].tool_id;
        let call = fixture.call(index, 1, ready(json!({ present: "value" })));
        let batch = fixture.batch(vec![call]);
        let ToolBatchAdmissionOutcomeV1::Admitted(admitted) = admit_tool_batch(
            batch,
            &fixture.registry,
            fixture.definitions.clone(),
            &AllowAuthority,
        )
        .expect("admission") else {
            panic!("{tool_id}: the batch stays pairable")
        };
        let rejection = &admitted.not_executed[0];
        assert_eq!(rejection.input_issue, None, "{tool_id}");
        assert_eq!(rejection.input_diagnostic, None, "{tool_id}");
        let report = rejection
            .schema_validation
            .as_ref()
            .unwrap_or_else(|| panic!("{tool_id}: the business schema must report"));
        assert!(!report.valid, "{tool_id}");
        // The report is the real validator's, so the missing field is named by the
        // `required` issue rather than by an instance path the model never sent.
        assert!(
            report.errors.iter().any(|error| {
                error.issue_kind.contains("required") && error.issue_kind.contains(missing)
            }),
            "{tool_id}: the report must require `{missing}`: {:?}",
            report.errors
        );
        assert!(
            report.errors.iter().any(|error| {
                error.issue_kind.contains("additionalproperties")
                    && error.issue_kind.contains(present)
            }),
            "{tool_id}: the report must call `{present}` unexpected: {:?}",
            report.errors
        );
    }
}
