use super::*;

pub(super) struct ChatReply<'a> {
    pub scenario: Scenario,
    pub ordinal: usize,
    pub body: &'a Value,
    pub state: &'a Value,
    pub turn: &'a Value,
    pub selected: &'a Value,
    pub recheck: bool,
    pub query: bool,
    pub write: bool,
    pub search: bool,
    pub search_then_text: bool,
    pub external_read: bool,
    pub external_list: bool,
    pub external_list_outcome: bool,
    pub external_find: bool,
    pub external_search: bool,
    pub external_write: bool,
    pub read_missing: bool,
    pub discover: bool,
    pub target: &'a str,
}

pub(super) fn delta(reply: ChatReply<'_>, call: &impl Fn(&str, Value, usize) -> Value) -> Value {
    let ChatReply {
        scenario,
        ordinal,
        body,
        state,
        turn,
        selected,
        recheck,
        query,
        write,
        search,
        search_then_text,
        external_read,
        external_list,
        external_list_outcome,
        external_find,
        external_search,
        external_write,
        read_missing,
        discover,
        target,
    } = reply;
    if matches!(scenario, Scenario::DeepSeekWebFetchMissing) {
        let fetched = body["messages"]
            .as_array()
            .is_some_and(|messages| messages.iter().any(|m| m["role"] == "tool"));
        return if fetched {
            json!({"role":"assistant","content":"Fetched."})
        } else {
            json!({"role":"assistant","tool_calls":[call("web.fetch",json!({"url":"https://example.com/missing"}),0)]})
        };
    }
    if let Some(reply) = entry_reply(scenario, ordinal, body, call) {
        return reply;
    }
    if matches!(scenario, Scenario::ShellReadRest) {
        let results = body["messages"]
            .as_array()
            .map(|messages| messages.iter().filter(|m| m["role"] == "tool").cloned().collect())
            .unwrap_or_else(Vec::new);
        return match results.as_slice() {
            [] => json!({"role":"assistant","tool_calls":[call(
                "shell.powershell",
                json!({"script":"1..3000 | ForEach-Object { 'line ' + $_ }"}),
                0,
            )]}),
            [shell] => {
                let view: Value = serde_json::from_str(shell["content"].as_str().unwrap_or("{}"))
                    .unwrap_or_default();
                let result = &view["outcome"]["payload"]["value"];
                json!({"role":"assistant","tool_calls":[call(
                    "shell.query",
                    json!({
                        "tool_call_id": result["tool_call_id"],
                        "stdout_offset": result["stdout_next"],
                        "max_bytes": 4096
                    }),
                    1,
                )]})
            }
            _ => json!({"role":"assistant","content":"Done."}),
        };
    }
    if let Scenario::Batch { calls } = scenario {
        let answered = body["messages"]
            .as_array()
            .is_some_and(|messages| messages.iter().any(|m| m["role"] == "tool"));
        if answered {
            return json!({"role":"assistant","content":"Done."});
        }
        let tool_calls = calls
            .iter()
            .enumerate()
            .map(|(index, (tool, arguments))| {
                call(
                    tool,
                    serde_json::from_str(arguments).expect("the scenario's arguments"),
                    index,
                )
            })
            .collect::<Vec<_>>();
        return json!({"role":"assistant","tool_calls":tool_calls});
    }
    if let Scenario::Sequence { calls } = scenario {
        let done = body["messages"]
            .as_array()
            .map_or(0, |messages| messages.iter().filter(|m| m["role"] == "tool").count());
        let Some((tool, arguments)) = calls.get(done) else {
            return json!({"role":"assistant","content":"Done."});
        };
        let arguments: Value = serde_json::from_str(arguments).expect("the scenario's arguments");
        let loaded = body["tools"].as_array().is_some_and(|tools| {
            tools.iter().any(|t| {
                t["function"]["name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with(&format!("{}__", tool.replace('.', "_"))))
            })
        });
        return if loaded {
            json!({"role":"assistant","tool_calls":[call(tool,arguments,done)]})
        } else {
            json!({"role":"assistant","tool_calls":[call("tools.call",json!({"tool":tool,"arguments":arguments}),done)]})
        };
    }
    if let Scenario::Browser { tool, arguments } = scenario {
        let called = body["messages"]
            .as_array()
            .is_some_and(|messages| messages.iter().any(|m| m["role"] == "tool"));
        let arguments: Value = serde_json::from_str(arguments).expect("the scenario's arguments");
        // A tool not loaded by default is reached through the unified entry, as a model does.
        let loaded = body["tools"].as_array().is_some_and(|tools| {
            tools.iter().any(|t| {
                t["function"]["name"]
                    .as_str()
                    .is_some_and(|name| name.starts_with(&format!("{}__", tool.replace('.', "_"))))
            })
        });
        return if called {
            json!({"role":"assistant","content":"Done."})
        } else if loaded {
            json!({"role":"assistant","tool_calls":[call(tool,arguments,0)]})
        } else {
            json!({"role":"assistant","tool_calls":[call("tools.call",json!({"tool":tool,"arguments":arguments}),0)]})
        };
    }
    if matches!(scenario, Scenario::DeepSeekWebSearch) {
        let searched = body["messages"]
            .as_array()
            .is_some_and(|messages| messages.iter().any(|m| m["role"] == "tool"));
        return if searched {
            json!({"role":"assistant","content":"Searched."})
        } else {
            json!({"role":"assistant","tool_calls":[call("web.search",json!({"query":"aura fixture query","max_results":3}),0)]})
        };
    }
    if matches!(scenario, Scenario::ClearOldResults) {
        return if ordinal <= 10 {
            json!({"role":"assistant","tool_calls":[call("fs.read",json!({"path":format!("read-{ordinal}.txt"),"range":{"range_kind":"full"},"explicit_encoding":null}),0)]})
        } else {
            json!({"role":"assistant","content":"Every read is done."})
        };
    }
    if matches!(scenario, Scenario::CompactOnPrefix | Scenario::CompactToolOnPrefix)
        && (2..=3).contains(&ordinal)
    {
        // Two answers cut off at their limit, a short one, then one the step continuing
        // it must compact for first.
        let repeat = if ordinal == 2 { 1 } else { 800 };
        return json!({"role":"assistant","content":format!("Observed intermediate response {ordinal}: {}","stable historical detail ".repeat(repeat))});
    }
    if matches!(
        scenario,
        Scenario::Plain
            | Scenario::PlainMeasured
            | Scenario::PlainMeasuredRefusingSecond
            | Scenario::CompactOnPrefix
            | Scenario::CompactToolOnPrefix
    ) {
        return json!({"role":"assistant","content":format!("Answered request {ordinal}.")});
    }
    if matches!(scenario, Scenario::HookRead) {
        return if ordinal == 0 {
            json!({"role":"assistant","tool_calls":[call("fs.read",json!({"path":super::HOOK_SOURCE,"range":{"range_kind":"full"},"explicit_encoding":null}),0)]})
        } else {
            json!({"role":"assistant","content":"done"})
        };
    }
    if matches!(scenario, Scenario::Init) {
        return if ordinal == 0 {
            json!({"role":"assistant","tool_calls":[call("workspace_init.complete",json!({
                "language":"english",
                "expected":{"expected_kind":"absent"},
                "project_structure":"Rust crates live under `crates/`.",
                "commands":{"entries":[{"command":INIT_COMMAND,"purpose":"Run every Rust test",
                    "shell":"powershell","evidence":{"kind":"repository_defined","path":INIT_EVIDENCE_FILE}}]},
                "coding_style":"Format Rust with rustfmt.",
                "testing":"Write a failing test before the fix.",
                "commits_and_pull_requests":"One complete change per commit."
            }),0)]})
        } else {
            json!({"role":"assistant","content":"AGENTS.md is written."})
        };
    }
    if matches!(scenario, Scenario::BranchingCompaction) && ordinal >= 2 {
        return json!({"role":"assistant","content":format!("Turn answered at request {ordinal}.")});
    }
    if matches!(scenario, Scenario::CompactAfterTool | Scenario::BranchingCompaction)
        && ordinal == 0
    {
        json!({"role":"assistant","tool_calls":[call("fs.read",json!({"path":COMPACTION_SOURCE_FILE,"range":{"range_kind":"full"},"explicit_encoding":null}),0)]})
    } else if matches!(scenario, Scenario::Compact | Scenario::CompactHold) && ordinal < 2
        || matches!(scenario, Scenario::CompactAfterTool | Scenario::BranchingCompaction)
            && ordinal == 1
        || matches!(scenario, Scenario::BranchingThenCompact) && (4..=5).contains(&ordinal)
    {
        // The response that inflates history is truncated, never a
        // final answer: a complete tool-free answer now legitimately
        // ends the turn, so compaction would never be reached.
        json!({"role":"assistant","content":format!("Observed intermediate response {ordinal}: {}","stable historical detail ".repeat(4500))})
    } else if recheck {
        json!({"role":"assistant","tool_calls":[call("task_lineage.read_sources",json!({"lineage_id":turn["lineage_id"],"expected_lineage_revision":selected["revision"],"source_event_ids":[turn["root_user_event_id"]]}),0)]})
    } else if matches!(scenario, Scenario::Plan) {
        assert_eq!(ordinal, 0);
        json!({"role":"assistant","tool_calls":[call("plan.complete",json!({"expected_turn_revision":turn["revision"],"requirements":["Check the answer"],"current_project_state":"The actual task is in plan mode","steps":[{"id":"verify","description":"Verify the requested answer","depends_on":[],"execution":{"execution":"serial"},"validation":"Read the persisted response"}]}),0)]})
    } else if matches!(scenario, Scenario::Goal) && ordinal == 0 {
        json!({"role":"assistant","tool_calls":[call("goal.check",json!({"expected_goal_revision":state["goal"]["revision"],"assessment":"The requested check is complete","remaining_work":[],"evidence_refs":[]}),0)]})
    } else if matches!(scenario, Scenario::Goal) {
        assert_eq!(ordinal, 1, "goal.check and completion must be separate model steps");
        let previous = body["messages"]
            .as_array()
            .expect("real Provider fixture request and response")
            .iter()
            .rev()
            .find(|v| v["role"] == "tool")
            .expect("real Provider fixture request and response");
        let result: Value = serde_json::from_str(
            previous["content"].as_str().expect("real Provider fixture request and response"),
        )
        .expect("real Provider fixture request and response");
        json!({"role":"assistant","content":"Verified the request and the actual recorded Goal check.","tool_calls":[call("goal.complete",json!({"expected_goal_revision":state["goal"]["revision"],"final_report":"Verified the requested result","verification":"The previous goal.check is committed","evidence_refs":[result["artifacts"][0]["artifact_id"]]}),0)]})
    } else if matches!(scenario, Scenario::GoalWithPaths | Scenario::GoalWithUnknownArtifact)
        && ordinal == 0
    {
        let evidence = if matches!(scenario, Scenario::GoalWithPaths) {
            json!(["hungry-shark.html", "tests/hungry-shark.test.cjs"])
        } else {
            json!([aura_domain_common::ArtifactId::new()])
        };
        json!({"role":"assistant","tool_calls":[call("goal.complete",json!({"expected_goal_revision":state["goal"]["revision"],"final_report":"The game is written and its tests pass.","verification":"node tests/hungry-shark.test.cjs: 14 passed","evidence_refs":evidence}),0)]})
    } else if matches!(scenario, Scenario::GoalWithPaths | Scenario::GoalWithUnknownArtifact) {
        super::recovery_response(state, call)
    } else if matches!(scenario, Scenario::MalformedArguments) && ordinal == 0 {
        let mut response = json!({
            "role": "assistant",
            "tool_calls": [
                call("tools.search", json!({"query":"first","limit":5,"cursor":null}), 0)
            ]
        });
        // A literal newline inside a JSON string: what a Provider
        // sends when a long value is not escaped correctly.
        response["tool_calls"][0]["function"]["arguments"] =
            json!("{\"aura_purpose\":\"Malformed arguments\",\"query\":\"first\nsecond\"}");
        response
    } else if matches!(scenario, Scenario::EmptyText) && ordinal == 0 {
        json!({"role":"assistant","content":""})
    } else if matches!(scenario, Scenario::ThinkingOnly | Scenario::ThinkingCutOff) && ordinal == 0
    {
        json!({"role":"assistant","reasoning_content":"Considering the greeting, but never answering it."})
    } else if matches!(scenario, Scenario::TruncatedText) && ordinal == 0 {
        json!({"role":"assistant","content":"An answer the Provider cut off mid-sentence"})
    } else if !discover && ordinal == 0 {
        json!({"role":"assistant","content":"The requested text response is ready; I will now close the task."})
    } else if discover && ordinal == 0 {
        json!({"role":"assistant","tool_calls":[call("tools.search",json!({"query":target,"limit":5,"cursor":null}),0)]})
    } else if discover && ordinal == 1 {
        let release = aura_tool_registry::agent_release::release()
            .expect("real Provider fixture request and response");
        let definition =
            &release.entry(target).expect("real Provider fixture request and response").definition;
        let messages =
            body["messages"].as_array().expect("real Provider fixture request and response");
        assert!(
            messages.iter().any(|m| m["role"] == "tool"
                && m["content"].as_str().is_some_and(|s| s.contains(target))),
            "Search must return the actual definition"
        );
        json!({"role":"assistant","tool_calls":[call("tools.load",json!({"tool_ids":[definition.tool_id],"replace":[]}),0)]})
    } else if write && ordinal == 2 {
        let text = if matches!(scenario, Scenario::LongWrite) {
            "x".repeat(40_000)
        } else {
            "actual Agent Loop effect".to_owned()
        };
        json!({"role":"assistant","tool_calls":[call("fs.write",json!({"path":"agent-result.txt","text":text,"expected":{"expected_kind":"absent"}}),0)]})
    } else if (search || search_then_text) && ordinal == 2 {
        json!({"role":"assistant","tool_calls":[call("fs.search",json!({"pattern":{"kind":"literal","text":"unique_fs_search_agent_token","case_sensitive":true},"root":".","path_glob":null,"include_ignored":false,"include_generated":false,"limit":50}),0)]})
    } else if external_search && ordinal == 2 {
        let external = named_external_path(body);
        json!({"role":"assistant","tool_calls":[call("fs.search",json!({
                    "pattern": {"kind":"literal","text":EXTERNAL_BODY_TOKEN,"case_sensitive":true},
                    "root": external,
                    "path_glob": null,
                    "include_ignored": false,
                    "include_generated": false,
                    "limit": 50
                }),0)]})
    } else if external_write && ordinal == 2 {
        // The file does not exist yet, so this is the create target: the
        // model names the absolute path and states that it expects no
        // file there.
        let external = named_external_path(body);
        json!({"role":"assistant","tool_calls":[call("fs.write",json!({
                    "path": external,
                    "text": EXTERNAL_WRITE_TOKEN,
                    "expected": {"expected_kind":"absent"}
                }),0)]})
    } else if external_write && ordinal == 3 {
        let content = tool_content(body);
        assert!(
            content.contains("\"created\":true"),
            "the external write result must reach the next model request: {content}"
        );
        json!({"role":"assistant","content":"The external file was written; the task is complete."})
    } else if external_search && ordinal == 3 {
        // The outcome is the test's to assert: this scenario only has to
        // close the turn.
        json!({"role":"assistant","content":"The external search request was handled; the task is complete."})
    } else if external_find && ordinal == 2 {
        let external = named_external_path(body);
        json!({"role":"assistant","tool_calls":[call("fs.find",json!({
                    "path": external,
                    "file_names": [format!("{EXTERNAL_LIST_TOKEN}.txt")],
                    "recursive": true,
                    "match_mode": "exact",
                    "case_sensitive": false,
                    "limit": 10,
                    "cursor": null
                }),0)]})
    } else if external_find && ordinal == 3 {
        let content = tool_content(body);
        if matches!(scenario, Scenario::ExternalFindScopeCapped) {
            // The name exists, but only below the authorized depth: the
            // walk must not have gone there, and the page must say it
            // found nothing rather than reporting an error.
            assert!(
                !content.contains(EXTERNAL_LIST_TOKEN),
                "a recursive request must not widen a non-recursive grant: {content}"
            );
            assert!(
                content.contains("\"matches\":[]"),
                "the capped walk must answer with an empty page, not a failure: \
                         {content}"
            );
            json!({"role":"assistant","content":"No match inside the authorized depth; the task is complete."})
        } else {
            assert!(
                content.contains(EXTERNAL_LIST_TOKEN),
                "the external lookup must reach the next model request: {content}"
            );
            json!({"role":"assistant","content":"The external file was found; the task is complete."})
        }
    } else if (external_list || external_list_outcome) && ordinal == 2 {
        let external = named_external_path(body);
        json!({"role":"assistant","tool_calls":[call("fs.list",json!({
                    "path": external,
                    "limit": 10,
                    "cursor": null
                }),0)]})
    } else if external_list && ordinal == 3 {
        let content = tool_content(body);
        assert!(
            content.contains(EXTERNAL_LIST_TOKEN),
            "the external listing must reach the next model request: {content}"
        );
        json!({"role":"assistant","content":"The external directory is listed; the task is complete."})
    } else if external_list_outcome && ordinal == 3 {
        // The listing may legitimately be refused, so this scenario does not
        // assert the result: the test owns that judgement.
        json!({"role":"assistant","content":"The listing request was answered; the task is complete."})
    } else if read_missing && ordinal == 2 {
        // The path is inside the workspace and does not exist: the call is admitted,
        // dispatched and really run, and the tool answers with a definite failure.
        json!({"role":"assistant","tool_calls":[call("fs.read",json!({
                    "path": "definitely-missing-agent-file.txt",
                    "range": {"range_kind":"full"},
                    "explicit_encoding": null
                }),0)]})
    } else if read_missing && ordinal == 3 {
        let content = tool_content(body);
        assert!(
            !content.is_empty(),
            "a definite tool failure must still produce a real result for the model"
        );
        json!({"role":"assistant","content":"The read was answered with a definite failure; the task is complete."})
    } else if external_read && ordinal == 2 {
        // The path comes from the user message, so the test decides which
        // authorized file the model asks for.
        let external = named_external_path(body);
        json!({"role":"assistant","tool_calls":[call("fs.read",json!({
                    "path": external,
                    "range": {"range_kind":"full"},
                    "explicit_encoding": null
                }),0)]})
    } else if external_read && ordinal == 3 {
        let messages =
            body["messages"].as_array().expect("real Provider fixture request and response");
        let content = messages
            .iter()
            .filter(|m| m["role"] == "tool")
            .filter_map(|m| m["content"].as_str())
            .collect::<Vec<_>>()
            .join("\n");
        if matches!(scenario, Scenario::ExternalReadMatrix) {
            // The matrix decides what the mode really did; asserting here
            // would only restate the expectation.
            json!({"role":"assistant","content":"The external request was handled; the task is complete."})
        } else if matches!(scenario, Scenario::ExternalReadRefused) {
            assert!(
                !content.contains(EXTERNAL_BODY_TOKEN),
                "an unauthorized external file must not be read: {content}"
            );
            assert!(
                content.contains("outside this session's authorized workspace"),
                "the refusal must be the corrigible outside-workspace class: {content}"
            );
            json!({"role":"assistant","content":"The external path is not authorized; the task is complete."})
        } else {
            assert!(
                content.contains(EXTERNAL_BODY_TOKEN),
                "the external read result must reach the next model request: {content}"
            );
            json!({"role":"assistant","content":"The external file is readable; the task is complete."})
        }
    } else if search_then_text && ordinal == 3 {
        json!({"role":"assistant","content":"The search result is ready; I will now close the task."})
    } else if matches!(scenario, Scenario::Branching) && ordinal >= 3
        || matches!(scenario, Scenario::BranchingThenCompact) && (ordinal == 3 || ordinal >= 6)
    {
        json!({"role":"assistant","content":format!("Turn answered at request {ordinal}.")})
    } else if matches!(scenario, Scenario::AdditionText | Scenario::CompactedAddition)
        && ordinal == 1
    {
        json!({"role":"assistant","content":"The added request is included; the answer is complete."})
    } else if query && ordinal == 2 {
        json!({"role":"assistant","tool_calls":[call("query.user_question",json!({"questions":[{"id":"choice","text":"Confirm the fixture result?","answer_type":{"type":"confirm"}}]}),0)]})
    } else {
        assert_eq!(
            ordinal,
            if query || search_then_text || external_read || external_write {
                4
            } else if write
                || search
                || read_missing
                || matches!(scenario, Scenario::Compact | Scenario::CompactHold)
            {
                3
            } else if matches!(scenario, Scenario::CompactAfterTool) {
                2
            } else {
                1
            },
            "unexpected additional request: {body}"
        );
        if search || search_then_text {
            let previous = body["messages"]
                .as_array()
                .expect("real Provider fixture request and response")
                .iter()
                .rev()
                .find(|v| v["role"] == "tool")
                .expect("fs.search result must reach the next model request");
            let content = previous["content"].as_str().expect("fs.search tool content");
            assert!(
                content.contains("unique_fs_search_agent_token"),
                "model request after fs.search must consume the Match excerpt: {content}"
            );
            assert!(
                content.contains("search-fixture.txt"),
                "model request after fs.search must consume the Match path: {content}"
            );
            let parsed: Value =
                serde_json::from_str(content).expect("fs.search tool result is JSON");
            assert_eq!(parsed["outcome"]["type"], "succeeded");
            let value = &parsed["outcome"]["payload"]["value"];
            assert_eq!(value["completion"]["status"], "complete");
            assert!(
                value["matches"].as_array().is_some_and(|matches| {
                    matches.iter().any(|item| {
                        item["excerpt"]
                            .as_str()
                            .is_some_and(|excerpt| excerpt.contains("unique_fs_search_agent_token"))
                            && item["path"].as_str() == Some("search-fixture.txt")
                    })
                }),
                "fs.search Match fields missing from the following model request: {content}"
            );
        }
        if query {
            assert!(
                body["messages"]
                    .as_array()
                    .expect("real Provider fixture request and response")
                    .iter()
                    .filter(|v| v["role"] == "user")
                    // Every way a question ends is a `query_reply` the model reads.
                    .any(|u| u["content"].as_str().is_some_and(|s| s.contains("query_reply"))),
                "Actual query answer is missing"
            );
        }
        let revision = state["lineages"]
            .as_array()
            .expect("real Provider fixture request and response")
            .iter()
            .find(|l| l["lineage_id"] == turn["lineage_id"])
            .expect("real Provider fixture request and response")["revision"]
            .clone();
        json!({"role":"assistant","tool_calls":[
            call("normal.complete",json!({"lineage_id":turn["lineage_id"],"expected_lineage_revision":revision,"final_report":"Verified current mode behavior","disclosed_limitations":[]}),0)
        ]})
    }
}

/// The unified entry's scenarios, in the order their requests arrive.
fn entry_reply(
    scenario: Scenario,
    ordinal: usize,
    body: &Value,
    call: &impl Fn(&str, Value, usize) -> Value,
) -> Option<Value> {
    let query = || {
        aura_tool_registry::agent_release::release()
            .expect("release")
            .entry("agent.activity.query")
            .expect("agent.activity.query")
            .definition
            .tool_id
    };
    Some(match (scenario, ordinal) {
        (Scenario::UnifiedEntry | Scenario::EntryHookQuery, 0) => {
            json!({"role":"assistant","tool_calls":[call("tools.call",json!({"tool":"agent.activity.query","arguments":{"by":"session","limit":5}}),0)]})
        }
        (Scenario::UnifiedEntry, 1) => {
            json!({"role":"assistant","tool_calls":[call("tools.load",json!({"tool_ids":[query()],"replace":[]}),0)]})
        }
        (Scenario::UnifiedEntry, _) => json!({"role":"assistant","content":"Done."}),
        (Scenario::EntryRefused, 0) => {
            json!({"role":"assistant","tool_calls":[call("tools.call",json!({"tool":"goal.complete","arguments":{}}),0)]})
        }
        (Scenario::EntryRefused, _) => json!({"role":"assistant","content":"Not run."}),
        (Scenario::EntryExternalWrite, 0) => {
            let external = named_external_path(body);
            json!({"role":"assistant","tool_calls":[call("tools.call",json!({"tool":"fs.write","arguments":{
                "path": external,
                "text": "written through the entry",
                "expected": {"expected_kind": "absent"}
            }}),0)]})
        }
        (Scenario::EntryExternalWrite, _) => json!({"role":"assistant","content":"Written."}),
        (Scenario::EntryHookQuery, _) => json!({"role":"assistant","content":"Stopped."}),
        _ => return None,
    })
}
