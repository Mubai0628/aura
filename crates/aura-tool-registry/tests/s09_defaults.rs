//! A call that names only a tool's required fields gets the defaults its description
//! states, and the published schema requires exactly those fields.
use aura_domain_common::{TaskLineageId, ToolCallId};
use aura_tool_registry::agent_release::release;
use serde_json::{Value, json};

fn schema(name: &str) -> Value {
    release()
        .expect("verified current release")
        .entry(name)
        .unwrap_or_else(|| panic!("{name} is a built-in tool"))
        .definition
        .input_schema
        .document()
        .clone()
}

fn required(name: &str) -> Vec<String> {
    serde_json::from_value(schema(name)["required"].clone()).unwrap_or_default()
}

fn description(name: &str, property: &str) -> String {
    schema(name)["properties"][property]["description"].as_str().unwrap_or_default().to_owned()
}

fn decode<T: serde::de::DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).expect("the minimal input decodes")
}

#[test]
fn a_shell_call_needs_only_its_script() {
    use aura_windows_user_contract::{
        DEFAULT_SHELL_MAX_OUTPUT_BYTES as OUTPUT, DEFAULT_SHELL_TIMEOUT_MS as TIMEOUT,
        ShellExecuteInput, ShellQueryInput,
    };
    for tool in ["shell.powershell", "shell.cmd"] {
        assert_eq!(required(tool), ["script"], "{tool}");
        assert!(description(tool, "timeout_ms").starts_with(&format!("Default {TIMEOUT}.")));
        assert!(description(tool, "max_output_bytes").starts_with(&format!("Default {OUTPUT}.")));
    }
    let input: ShellExecuteInput = decode(json!({"script": "dir"}));
    assert_eq!(
        input,
        ShellExecuteInput {
            requires_network: false,
            script: "dir".into(),
            working_directory: ".".into(),
            stdin: String::new(),
            timeout_ms: TIMEOUT,
            max_output_bytes: OUTPUT,
            secret_environment: Vec::new(),
        }
    );
    // The description promises that the default tool time limit is 10 minutes, that long commands may raise timeout_ms, and a Shell call that keeps its own
    // default ends by its own timeout before that limit.
    assert_eq!(aura_tool_contract::DEFAULT_TOOL_TIMEOUT_MS, 600_000);
    const { assert!(TIMEOUT <= aura_tool_contract::DEFAULT_TOOL_TIMEOUT_MS) };
    let entry = release().expect("release").entry("shell.powershell").expect("PowerShell");
    assert!(entry.definition.model_description.contains("(default 10 minutes)"));
    assert!(entry.definition.model_description.contains("a larger timeout_ms"));

    assert_eq!(required("shell.query"), ["tool_call_id"]);
    let query: ShellQueryInput = decode(json!({"tool_call_id": ToolCallId::new()}));
    assert_eq!((query.stdout_offset, query.stderr_offset, query.max_bytes), (0, 0, 65_536));
}

#[test]
fn file_tools_read_whole_files_and_full_pages_from_the_workspace_root() {
    use aura_workspace_domain::{
        AgentFileFindInput, AgentFileFindMatchMode, AgentFileListInput, AgentFileReadInput,
        AgentFileSearchInput, FsSearchPattern, TextReadRange,
    };
    assert_eq!(required("fs.read"), ["path"]);
    let read: AgentFileReadInput = decode(json!({"path": "src/lib.rs"}));
    assert_eq!((read.range, read.explicit_encoding), (TextReadRange::Full, None));

    assert_eq!(required("fs.list"), ["path"]);
    let list: AgentFileListInput = decode(json!({"path": "."}));
    assert_eq!((list.limit, list.cursor.is_none()), (100, true));

    assert_eq!(required("fs.find"), ["file_names"]);
    let find: AgentFileFindInput = decode(json!({"file_names": ["a.rs"]}));
    assert_eq!(find.path.relative().as_str(), ".");
    assert!(find.recursive && !find.case_sensitive && find.cursor.is_none());
    assert_eq!((find.match_mode, find.limit), (AgentFileFindMatchMode::Exact, 100));

    assert_eq!(required("fs.search"), ["pattern"]);
    let search: AgentFileSearchInput = decode(json!({"pattern": {"kind": "literal", "text": "x"}}));
    assert_eq!(search.root.relative().as_str(), ".");
    assert!(!search.include_ignored && !search.include_generated && search.path_glob.is_none());
    assert_eq!(search.limit, 100);
    assert_eq!(
        search.pattern,
        FsSearchPattern::Literal { text: "x".into(), case_sensitive: false }
    );
    // The page cursors stay opaque to the model.
    for tool in ["fs.list", "fs.find"] {
        let cursor = schema(tool)["properties"]["cursor"].to_string();
        assert!(cursor.contains("OpaquePageCursor"), "{tool}: {cursor}");
    }
}

#[test]
fn context_reads_page_and_leave_out_the_input_by_default() {
    use aura_context_domain::{AgentContextReadEventsInput, AgentContextReadToolResultInput};
    assert_eq!(required("context.read_events"), ["selectors"]);
    let events: AgentContextReadEventsInput = decode(json!({"selectors": []}));
    assert_eq!((events.character_limit, events.cursor), (8_192, None));
    assert_eq!(required("context.read_tool_result"), ["tool_call_id"]);
    let result: AgentContextReadToolResultInput =
        decode(json!({"tool_call_id": ToolCallId::new()}));
    assert!(!result.include_input);
}

#[test]
fn workflow_lists_default_to_empty_and_new_items_to_pending() {
    use aura_agent_domain::{
        GoalCheckInput, NormalCompleteInput, PlanStep, PlanStepExecution, TodoItemDraft, TodoStatus,
    };
    assert_eq!(required("goal.check"), ["expected_goal_revision", "assessment"]);
    let check: GoalCheckInput =
        decode(json!({"expected_goal_revision": "1", "assessment": "all tests pass"}));
    assert!(check.remaining_work.is_empty() && check.evidence_refs.is_empty());

    assert_eq!(
        required("normal.complete"),
        ["lineage_id", "expected_lineage_revision", "final_report"]
    );
    let complete: NormalCompleteInput = decode(json!({"lineage_id": TaskLineageId::new(),
        "expected_lineage_revision": "1", "final_report": "done"}));
    assert!(complete.disclosed_limitations.is_empty());

    assert_eq!(
        required("task_lineage.review_completion"),
        ["lineage_id", "expected_lineage_revision", "assessment", "reviewed_user_intents"]
    );
    let step: PlanStep = decode(json!({"id": "a", "description": "read the parser"}));
    assert!(step.depends_on.is_empty() && step.validation.is_empty());
    assert_eq!(step.execution, PlanStepExecution::Serial);
    let item: TodoItemDraft = decode(json!({"title": "add a regression test"}));
    assert_eq!(item.status, TodoStatus::Pending);
}
