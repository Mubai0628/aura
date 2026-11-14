//! The user lets a sub-agent in a worktree delete without asking: with 本工作区允许 on the card
//! of a deletion its command does not place, or in the settings before it starts. Its later
//! deletions in its own folders run unasked; one named outside its workspace asks again, and that
//! card offers no such answer. The settings may also let its commands write outside.
use super::l19_team::{begin, call, members, requests, results};
use super::l19_worktrees::{git, only_copy, prepare};
use super::*;
use provider_fixture::{ALLOWANCE_FIRST, ALLOWANCE_SECOND, ALLOWANCE_WRITTEN, Scenario, TeamPlan};

/// The sub-agent's next question besides those already answered, with its conversation.
pub(super) async fn asked(
    runtime: &crate::RuntimeState,
    session: SessionId,
    answered: &[Value],
) -> (Value, Value) {
    tokio::time::timeout(std::time::Duration::from_secs(90), async {
        loop {
            let team = members(runtime, session).await;
            if let Some(member) = team.first()
                && let Some(id) = member["approvals"]
                    .as_array()
                    .and_then(|ids| ids.iter().find(|id| !answered.contains(id)))
            {
                let child = member["session_id"].clone();
                let view = call(
                    runtime,
                    "agent.approval.get",
                    json!({"session_id": child, "approval_id": id}),
                )
                .await;
                return (child, view);
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("the sub-agent asks")
}

pub(super) async fn decide(
    runtime: &crate::RuntimeState,
    child: &Value,
    view: &Value,
    decision: &str,
) {
    call(
        runtime,
        "agent.approval.resolve",
        json!({
            "session_id": child,
            "approval_id": view["approval"]["approval_id"],
            "expected_revision": view["approval"]["revision"],
            "decision": decision,
        }),
    )
    .await;
}

fn offers_workspace(view: &Value) -> bool {
    view["approval"]["allowed_decisions"]
        .as_array()
        .is_some_and(|decisions| decisions.contains(&json!("allow_workspace")))
}

/// A worktree workspace with `files` committed, and a path outside it named in the message.
pub(super) async fn started(
    host: &crate::r03_tests::Fixture,
    provider: &Provider,
    files: &[&str],
    outside: &str,
    settings: Option<Value>,
) -> (crate::RuntimeState, SessionId, AgentRunRequest, std::path::PathBuf, std::path::PathBuf) {
    let (runtime, session, model, root, location) = prepare(host, provider).await;
    for name in files {
        std::fs::write(root.join(name), "x\n").expect("a file to delete");
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "files"]);
    if let Some(change) = settings {
        let read = call(&runtime, "agent.team.settings", json!({})).await;
        let mut permissions = read["permissions"].clone();
        for (name, value) in change.as_object().expect("permissions") {
            permissions[name] = value.clone();
        }
        call(
            &runtime,
            "agent.team.settings.set",
            json!({
                "limits": read["limits"],
                "permissions": permissions,
                "expected_revision": read["revision"],
            }),
        )
        .await;
    }
    let outside = host.root.path().join(outside);
    let text =
        format!("清理 AURA_EXTERNAL_PATH={} AURA_COPIES={}", outside.display(), location.display());
    let request = begin(&runtime, session, model, &text).await;
    (runtime, session, request, location, outside)
}

#[tokio::test]
async fn a_sub_agent_the_user_lets_delete_in_its_copy_is_asked_again_only_outside() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::AllowanceCard }).await;
    let files = [ALLOWANCE_FIRST, ALLOWANCE_SECOND];
    let (runtime, session, request, location, outside) =
        started(&host, &provider, &files, "outside-keep.txt", None).await;
    std::fs::write(&outside, "outside\n").expect("a file outside the workspace");

    // Its deletion by variable asks, and may be answered by letting it delete in its folders.
    let (child, first) = asked(&runtime, session, &[]).await;
    assert!(offers_workspace(&first), "{first}");
    decide(&runtime, &child, &first, "allow_workspace").await;

    // The second deletion by variable ran unasked: the next question is the deletion named
    // outside, which offers no such answer.
    let (_, second) = asked(&runtime, session, &[first["approval"]["approval_id"].clone()]).await;
    assert!(second.to_string().contains("outside-keep.txt"), "{second}");
    assert!(!offers_workspace(&second), "{second}");
    let allowed = &members(&runtime, session).await[0]["allowed"];
    assert_eq!(allowed, &json!({"deletions": true, "outside_writes": false, "downloads": false}));
    decide(&runtime, &child, &second, "deny").await;

    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    let (theirs, _) = requests(&provider);
    let done = results(theirs.last().expect("the sub-agent's last request"));
    assert_eq!(done.len(), 3, "{done:#?}");
    let copy = only_copy(&location);
    assert!(!copy.join(ALLOWANCE_FIRST).exists(), "the approved deletion ran: {done:#?}");
    assert!(!copy.join(ALLOWANCE_SECOND).exists(), "the allowed deletion ran: {done:#?}");
    assert!(outside.exists(), "the refused deletion outside did not run");
}

#[tokio::test]
async fn the_settings_let_a_new_sub_agent_delete_in_its_copy_and_write_outside() {
    let host = crate::r03_tests::Fixture::new();
    let provider = Provider::scenario(Scenario::Team { plan: TeamPlan::AllowancePresets }).await;
    let presets = json!({"allow_deletions": true, "allow_outside_writes": true});
    let (runtime, session, request, location, outside) =
        started(&host, &provider, &[ALLOWANCE_FIRST], "outside-preset.txt", Some(presets)).await;

    // Its deletion by variable and its write outside ran unasked; its deletion named outside asks.
    let (child, view) = asked(&runtime, session, &[]).await;
    let text = view.to_string();
    assert!(text.contains("outside-preset.txt") && text.contains("Remove-Item"), "{view}");
    assert!(!offers_workspace(&view), "{view}");
    let allowed = &members(&runtime, session).await[0]["allowed"];
    assert_eq!(allowed, &json!({"deletions": true, "outside_writes": true, "downloads": false}));
    decide(&runtime, &child, &view, "deny").await;

    wait(&runtime, &request, |p| matches!(p.turn.state, TurnTaskState::Completed { .. })).await;
    assert!(!only_copy(&location).join(ALLOWANCE_FIRST).exists(), "the preset deletion ran");
    let written = std::fs::read_to_string(&outside).expect("written outside");
    assert_eq!(written.trim(), ALLOWANCE_WRITTEN);

    // The desktop names its worktree's folder for Main and takes back one allowance.
    let who = json!({"session_id": session, "agent": "A1"});
    let folder = call(&runtime, "agent.team.worktree.folder", who.clone()).await;
    let copy = only_copy(&location);
    let named = std::path::PathBuf::from(folder["path"].as_str().expect("a path"));
    assert_eq!(named.file_name(), copy.file_name(), "{folder}");
    let mut revoke = who;
    revoke["allowance"] = json!("outside_writes");
    let view = call(&runtime, "agent.team.allowance.revoke", revoke).await;
    let allowed = &view["members"][0]["allowed"];
    assert_eq!(allowed, &json!({"deletions": true, "outside_writes": false, "downloads": false}));
}
