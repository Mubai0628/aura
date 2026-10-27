//! The workspace's AGENTS.md reaches the model's instructions: the root file in every request,
//! named with its path and hash, and the files of the folders the conversation works in from the
//! next step on, root to deepest. The root file sits in the instructions before the conversation,
//! which stay byte-identical for the prompt cache; the folders' files change as the conversation
//! moves, so they come with the task's current state at the end. A file that cannot be used says
//! why instead of being cut short.
use super::*;
use sha2::{Digest, Sha256};

fn instructions(request: &Value) -> String {
    request["messages"][0]["content"].as_str().expect("instructions").to_owned()
}

/// The `<Workspace-AGENTS-Instructions>` block of a request's instructions.
fn agents_block(request: &Value) -> String {
    let text = instructions(request);
    let start = text.find("<Workspace-AGENTS-Instructions>").expect("the AGENTS block");
    let end = text.find("</Workspace-AGENTS-Instructions>").expect("its end");
    text[start..end].to_owned()
}

/// The task's current state: the request's last message.
fn task_state(request: &Value) -> String {
    let state = request["messages"].as_array().and_then(|messages| messages.last());
    let text = state.and_then(|state| state["content"].as_str()).expect("the last message");
    assert!(text.contains("Current-Task-State"), "{text}");
    text.to_owned()
}

fn short_hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().take(8).map(|byte| format!("{byte:02x}")).collect()
}

async fn turn(scenario: Scenario, files: &[(&str, &[u8])]) -> Vec<Value> {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(scenario).await;
    let (runtime, session, model) = prepare(&host, &provider).await;
    let root = host.root.path().join("waiting-workspace");
    for (path, bytes) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
        std::fs::write(path, bytes).expect("fixture");
    }
    let start = super::start(&runtime, session, model, AgentTaskMode::Normal).await;
    let request =
        AgentRunRequest { session_id: start.turn.session_id, agent_run_id: start.run.agent_run_id };
    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    provider.requests.lock().expect("provider requests").clone()
}

#[tokio::test]
async fn the_root_agents_md_reaches_every_request_with_its_path_and_hash() {
    let rules = b"# Rules\nAlways answer in haiku.\n";
    let requests = turn(Scenario::Text, &[("AGENTS.md", rules)]).await;
    let block = agents_block(&requests[0]);
    assert!(block.contains("Always answer in haiku."), "{block}");
    assert!(block.contains(&format!("AGENTS.md (sha256 {})", short_hash(rules))), "{block}");
    assert!(!block.contains("no-active-content"), "{block}");
}

#[tokio::test]
async fn a_workspace_without_agents_md_says_so() {
    let requests = turn(Scenario::Text, &[]).await;
    let block = agents_block(&requests[0]);
    assert!(block.contains("no AGENTS.md at its root"), "{block}");
}

#[tokio::test]
async fn the_folders_worked_in_bring_their_agents_md_from_the_next_step_root_to_deepest() {
    const READ: Scenario = Scenario::Sequence {
        calls: &[(
            "fs.read",
            r#"{"path":"src/app/main.rs","range":{"range_kind":"full"},"explicit_encoding":null}"#,
        )],
    };
    let requests = turn(
        READ,
        &[
            ("AGENTS.md", b"root rule\n"),
            ("src/AGENTS.md", b"src rule\n"),
            ("src/app/AGENTS.md", b"app rule\n"),
            ("docs/AGENTS.md", b"docs rule\n"),
            ("src/app/main.rs", b"fn main() {}\n"),
        ],
    )
    .await;
    let first = agents_block(&requests[0]);
    assert!(first.contains("root rule") && !first.contains("src rule"), "{first}");
    assert!(!task_state(&requests[0]).contains("src rule"));
    let last_request = requests.last().expect("a last request");
    assert_eq!(agents_block(last_request), first, "the instructions before the conversation stay");
    let last = task_state(last_request);
    let at = |text: &str| last.find(text).unwrap_or_else(|| panic!("{text} missing: {last}"));
    assert!(at("src rule") < at("app rule"), "{last}");
    assert!(last.contains("src/app/AGENTS.md"), "{last}");
    assert!(!last.contains("docs rule"), "a folder not worked in: {last}");
}

/// A tool the model reaches through the unified entry works in the folder the real call names: the
/// history keeps the `tools.call` wrapper, and the folder's AGENTS.md still arrives next step.
#[tokio::test]
async fn a_folder_worked_in_through_tools_call_brings_its_agents_md_next_step() {
    const THROUGH_ENTRY: Scenario = Scenario::Sequence {
        calls: &[(
            "tools.call",
            r#"{"tool":"fs.read","arguments":{"path":"nested/case.txt","range":{"range_kind":"full"},"explicit_encoding":null}}"#,
        )],
    };
    let requests = turn(
        THROUGH_ENTRY,
        &[
            ("AGENTS.md", b"root rule\n"),
            ("nested/AGENTS.md", b"NESTED_SCOPE rule\n"),
            ("nested/case.txt", b"73\n"),
        ],
    )
    .await;
    assert!(requests.len() >= 2, "the call and the answer after it");
    let first = agents_block(&requests[0]);
    assert!(!first.contains("NESTED_SCOPE"), "{first}");
    let last_request = requests.last().expect("a last request");
    assert!(agents_block(last_request).contains("root rule"));
    let last = task_state(last_request);
    assert!(last.contains("NESTED_SCOPE rule"), "{last}");
    assert!(last.contains("nested/AGENTS.md"), "{last}");
}

#[tokio::test]
async fn an_agents_md_that_cannot_be_used_says_why_and_is_never_cut_short() {
    let large = "Keep this rule.\n".repeat(16 * 1024);
    let requests = turn(Scenario::Text, &[("AGENTS.md", large.as_bytes())]).await;
    let block = agents_block(&requests[0]);
    assert!(block.contains("was not loaded") && block.contains("fs.read"), "{block}");
    assert!(!block.contains("Keep this rule."), "a large file is named, not cut: {block}");

    let token = format!("ghp_{}", "A1b2C3d4E5".repeat(4).chars().take(36).collect::<String>());
    let secret = format!("Deploy with {token}\n");
    let requests = turn(Scenario::Text, &[("AGENTS.md", secret.as_bytes())]).await;
    let block = agents_block(&requests[0]);
    assert!(block.contains("looks like a github_token"), "{block}");
    assert!(!instructions(&requests[0]).contains(&token), "the credential reached the model");
}
