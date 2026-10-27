//! What a Shell call gives the model back: the id shell.query and shell.stop take, so the rest of
//! a long output can be read; for a command Aura stopped at its output limit, the output kept and
//! why it stopped - never a user's cancellation; and for a wrong id, what shell.query takes.
use super::*;

/// The model views of the turn's tool results, in order.
async fn tool_results(scenario: Scenario) -> Vec<Value> {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(scenario).await;
    let (runtime, session, model) =
        prepare_in_mode(&host, &provider, PermissionMode::WorkspaceReadWrite).await;
    let start = super::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let requests = provider.requests.lock().expect("provider requests").clone();
    let last = requests.last().expect("a last request");
    last["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| {
            serde_json::from_str(message["content"].as_str().expect("text")).expect("a model view")
        })
        .collect()
}

#[tokio::test]
async fn the_model_reads_the_rest_of_a_long_output_with_the_id_its_result_gave() {
    let results = tool_results(Scenario::ShellReadRest).await;
    let shell = &results[0]["outcome"]["payload"]["value"];
    assert!(
        shell["tool_call_id"].as_str().is_some_and(|id| id.starts_with("tcl_")),
        "{}",
        results[0]
    );
    assert_eq!(shell["stdout_next"], 2048, "{}", results[0]);
    let rest = &results[1];
    assert_eq!(rest["outcome"]["type"], "succeeded", "{rest}");
    let page = rest["outcome"]["payload"]["value"]["stdout"].as_str().expect("a page");
    assert!(page.contains("line 300"), "{page}");
}

#[tokio::test]
async fn a_command_stopped_at_its_output_limit_says_so_and_keeps_its_output() {
    const NOISY: Scenario = Scenario::Sequence {
        calls: &[(
            "shell.powershell",
            r#"{"script":"1..20000 | ForEach-Object { 'line ' + $_ }","max_output_bytes":4096}"#,
        )],
    };
    let results = tool_results(NOISY).await;
    let view = &results[0];
    assert_eq!(view["outcome"]["type"], "succeeded", "{view}");
    let shell = &view["outcome"]["payload"]["value"];
    assert_eq!(shell["stop_reason"], "output_limit", "{view}");
    assert_eq!(shell["output_truncated"], true, "{view}");
    assert!(shell["stdout"].as_str().is_some_and(|out| out.contains("line 1")), "{view}");
    assert!(
        shell["diagnostic"].as_str().is_some_and(|text| text.contains("max_output_bytes")),
        "{view}"
    );
    assert!(!view.to_string().contains("user_requested"), "{view}");
}

#[tokio::test]
async fn shell_query_and_shell_stop_with_an_id_that_names_no_shell_call_say_what_they_take() {
    const WRONG: Scenario = Scenario::Sequence {
        calls: &[
            ("shell.query", r#"{"tool_call_id":"tcl_01ARZ3NDEKTSV4RRFFQ69G5FAV"}"#),
            ("shell.stop", r#"{"tool_call_id":"tcl_01ARZ3NDEKTSV4RRFFQ69G5FAV"}"#),
        ],
    };
    let results = tool_results(WRONG).await;
    assert_eq!(results.len(), 2, "{results:?}");
    for result in results {
        let view = result.to_string();
        assert!(view.contains("shell.powershell or shell.cmd"), "{view}");
    }
}
