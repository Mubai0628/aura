//! Arguments a model wrote with an object or an array as a string of JSON: read as the tool's
//! schema asks, fields that slipped into the string moved back beside it, and what was changed
//! said to the model. Anything the schema takes as a string stays a string.
use aura_tool_contract::{
    CanonicalJsonSchemaDocumentV1, CompiledToolSchemaV1, repair_string_encoded_arguments,
};
use serde_json::{Value, json};

/// The shape of `agents.spawn`: a task sheet by reference, optional tools and worktree.
fn spawn_schema() -> Value {
    json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "name": {"type": "string"},
            "task": {"$ref": "#/$defs/TaskSheet"},
            "tools": {"type": ["array", "null"], "items": {"type": "string"}},
            "worktree": {"type": "boolean"},
            "text": {"type": "string"},
            "either": {"anyOf": [{"type": "string"}, {"type": "object"}]}
        },
        "required": ["name", "task"],
        "$defs": {
            "TaskSheet": {
                "type": "object",
                "properties": {"goal": {"type": "string"}, "done_when": {"type": "string"}},
                "required": ["goal", "done_when"]
            }
        }
    })
}

#[test]
fn a_task_sheet_written_as_a_string_is_read_as_one_and_the_fields_after_it_move_back() {
    let schema = spawn_schema();
    let sent = json!({
        "name": "交互",
        "task": r#"{"goal":"改手感","done_when":"测过"},"tools":["fs.read"],"worktree":true}"#
    });
    let repaired = repair_string_encoded_arguments(&schema, &sent).expect("repaired");
    assert_eq!(
        repaired.arguments,
        json!({
            "name": "交互",
            "task": {"goal": "改手感", "done_when": "测过"},
            "tools": ["fs.read"],
            "worktree": true
        })
    );
    assert_eq!(repaired.parsed, ["task"]);
    assert_eq!(repaired.lifted, ["tools", "worktree"]);
    // The repaired call is one the schema takes.
    let compiled =
        CompiledToolSchemaV1::compile(&CanonicalJsonSchemaDocumentV1::new(schema).expect("schema"))
            .expect("compiled");
    assert!(compiled.validate(&repaired.arguments).valid);
    // The model is told, so its next call is written right.
    let note = repaired.note();
    for word in ["task", "tools", "worktree", "JSON"] {
        assert!(note.contains(word), "{word}: {note}");
    }
    assert!(!note.contains("改手感"), "values stay out of the note: {note}");
}

/// The form of the third 0.3.0 trial: the model closed the call outside the string, so the fields
/// after the object end the string without the call's closing brace.
#[test]
fn fields_after_the_object_move_back_when_the_string_ends_without_a_brace() {
    let schema = spawn_schema();
    let sent = json!({
        "name": "优化玩法",
        "task": r#"{"goal":"改手感","done_when":"测过"},"worktree":true"#
    });
    let repaired = repair_string_encoded_arguments(&schema, &sent).expect("repaired");
    assert_eq!(
        repaired.arguments,
        json!({
            "name": "优化玩法",
            "task": {"goal": "改手感", "done_when": "测过"},
            "worktree": true
        })
    );
    assert_eq!(repaired.parsed, ["task"]);
    assert_eq!(repaired.lifted, ["worktree"]);
}

#[test]
fn a_whole_object_or_array_in_a_string_is_read_as_it_alone() {
    let schema = spawn_schema();
    let sent = json!({
        "name": "读",
        "task": r#" {"goal":"读","done_when":"说出入口"} "#,
        "tools": r#"["fs.read","fs.search"]"#
    });
    let repaired = repair_string_encoded_arguments(&schema, &sent).expect("repaired");
    assert_eq!(repaired.arguments["task"], json!({"goal": "读", "done_when": "说出入口"}));
    assert_eq!(repaired.arguments["tools"], json!(["fs.read", "fs.search"]));
    assert_eq!(repaired.parsed, ["task", "tools"]);
    assert!(repaired.lifted.is_empty());
}

#[test]
fn nothing_is_changed_that_the_schema_takes_or_that_cannot_be_read_for_certain() {
    let schema = spawn_schema();
    let task = json!({"goal": "读", "done_when": "说出入口"});
    let unchanged = [
        // Already right.
        json!({"name": "读", "task": task}),
        // A string field holding JSON is the user's text.
        json!({"name": "读", "task": task, "text": r#"{"goal":"x"}"#}),
        // A field that takes a string or an object is not guessed.
        json!({"name": "读", "task": task, "either": r#"{"goal":"x"}"#}),
        // Not JSON.
        json!({"name": "读", "task": "{goal: 读}"}),
        // A string of JSON of another shape.
        json!({"name": "读", "task": "[1, 2]"}),
        // A field after it that the tool does not take, or that is already there.
        json!({"name": "读", "task": r#"{"goal":"a","done_when":"b"},"extra":1}"#}),
        json!({"name": "读", "task": r#"{"goal":"a","done_when":"b"},"name":"读"}"#}),
        // Something after it that is not a list of fields.
        json!({"name": "读", "task": r#"{"goal":"a","done_when":"b"} and more"#}),
        json!({"name": "读", "task": r#"{"goal":"a","done_when":"b"},"worktree":true}}"#}),
    ];
    for sent in unchanged {
        assert_eq!(repair_string_encoded_arguments(&schema, &sent), None, "{sent}");
    }
}

/// A call that is still refused says how to write it: the value itself as JSON, its other fields
/// beside it.
#[test]
fn a_string_where_an_object_belongs_is_refused_with_how_to_write_it() {
    let schema = CanonicalJsonSchemaDocumentV1::new(spawn_schema()).expect("input schema");
    let validator = CompiledToolSchemaV1::compile(&schema).expect("compiled schema");
    let report =
        serde_json::to_value(validator.validate(&json!({"name": "读", "task": "PRIVATE_VALUE"})))
            .expect("report");
    let message = report["errors"]
        .as_array()
        .expect("errors")
        .iter()
        .filter_map(|issue| issue["issue_kind"].as_str())
        .find(|message| message.contains("/task") || message.contains("object"))
        .expect("the task's issue")
        .to_owned();
    assert!(message.contains("not a string that holds JSON"), "{message}");
    assert!(message.contains("top level"), "{message}");
    assert!(!message.contains("PRIVATE_VALUE"), "{message}");
}
