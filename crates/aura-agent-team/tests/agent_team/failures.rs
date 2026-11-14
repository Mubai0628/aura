//! What the model reads when a team call fails: the cause in one bounded line, what the call
//! changed, and what usually helps next; no positions, chains or debug forms. Over a real
//! execution store; the host fails where a test says.
#![allow(clippy::unwrap_used)]
use crate::support;

use aura_agent_team::{
    MergeStep, WorktreeState, cause_of, main_agent_denial, one_line, worktree as rows,
};
use aura_domain_common::{ApprovalRequestId, SessionId};
use aura_persistence_core::PersistenceCoreError;
use serde_json::json;
use std::sync::atomic::Ordering;
use support::{FakeHost, call, task};

/// Starts a sub-agent of `parent` named `name` that works in this workspace.
async fn spawn_reader(host: &FakeHost, parent: SessionId, name: &str) {
    host.team()
        .call(call(parent, "agents.spawn", json!({"name": name, "task": task("读代码")})))
        .await
        .unwrap();
}

/// Starts sub-agent A1 of `parent` in a worktree of its own; its conversation.
async fn spawn_writer(host: &FakeHost, parent: SessionId) -> SessionId {
    host.team()
        .call(call(
            parent,
            "agents.spawn",
            json!({"name": "改登录", "task": task("改登录流程"), "worktree": true}),
        ))
        .await
        .unwrap();
    host.child(0)
}

fn worktree_of(host: &FakeHost, child: SessionId) -> aura_agent_team::Worktree {
    host.with_store(|db| rows::worktree_of(db, child)).unwrap().unwrap()
}

#[tokio::test]
async fn arguments_that_do_not_fit_name_the_field_and_change_nothing() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let missing =
        host.team().call(call(parent, "agents.spawn", json!({"name": "调研"}))).await.unwrap_err();
    assert_eq!(
        missing,
        "agents.input_invalid: 参数不对：少了必填的字段 task。什么都没做；按工具说明改好这一处再调用。"
    );
    let unknown = host
        .team()
        .call(call(parent, "agents.stop", json!({"agent": "A1", "force": true})))
        .await
        .unwrap_err();
    assert!(unknown.contains("没有 force 这个字段（能用的有：agent）"), "{unknown}");
    let wrong = host
        .team()
        .call(call(
            parent,
            "agents.approve",
            json!({"agent": "A1", "approval_id": ApprovalRequestId::new(), "answer": "maybe", "reason": "理由"}),
        ))
        .await
        .unwrap_err();
    assert!(
        wrong.contains("maybe 不是能用的值（能用的有：allow、allow_workspace、deny、ask_user）"),
        "{wrong}"
    );
    assert!(host.opened.lock().unwrap().is_empty());
}

/// A failure no step put in words says what the call was doing and that nothing changed.
#[tokio::test]
async fn a_failure_no_step_explained_says_what_the_call_was_doing() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    spawn_reader(&host, parent, "调研").await;
    *host.progress_failure.lock().unwrap() = Some("Core 正在重启".to_owned());
    let failed = host.team().call(call(parent, "agents.status", json!({}))).await.unwrap_err();
    assert_eq!(
        failed,
        "agents.failed: 查看子 Agent 没有完成：Core 正在重启。什么都没改，可以再调一次。"
    );
}

#[tokio::test]
async fn a_conversation_that_could_not_open_says_nothing_started() {
    let host = FakeHost::new();
    host.fail_open.store(true, Ordering::SeqCst);
    let refused = host
        .team()
        .call(call(
            SessionId::new(),
            "agents.spawn",
            json!({"name": "调研", "task": task("读代码")}),
        ))
        .await
        .unwrap_err();
    assert_eq!(
        refused,
        "agents.start_failed: Aura 没能给子 Agent 建会话：session.create_failed。子 Agent 没有开始干活；还需要就再调一次 agents.spawn。"
    );
}

#[tokio::test]
async fn a_sub_agent_whose_task_did_not_arrive_is_said_to_be_stopped() {
    let host = FakeHost::new();
    host.fail_delivery.store(true, Ordering::SeqCst);
    let refused = host
        .team()
        .call(call(
            SessionId::new(),
            "agents.spawn",
            json!({"name": "调研", "task": task("读代码")}),
        ))
        .await
        .unwrap_err();
    assert_eq!(
        refused,
        "agents.start_failed: 子 Agent A1（调研）没能开始：任务单没有送到它那里（agent.model_binding_required）。它已标记为停止，没干任何活；还需要这个任务就重新调 agents.spawn。"
    );
}

#[tokio::test]
async fn a_message_that_may_not_have_arrived_says_what_to_check() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    spawn_reader(&host, parent, "调研").await;
    host.fail_delivery.store(true, Ordering::SeqCst);
    let failed = host
        .team()
        .call(call(parent, "agents.send", json!({"agent": "A1", "text": "再看看测试"})))
        .await
        .unwrap_err();
    assert_eq!(
        failed,
        "agents.send_failed: 给子 Agent A1 发消息时出错：agent.model_binding_required。消息可能没有送到；用 agents.status 看它现在的状态，再决定要不要重发。"
    );
    // A conversation that is gone took nothing.
    *host.delivery_failure.lock().unwrap() =
        Some("agent.session_missing: 找不到这个会话，它可能已被删除。".to_owned());
    let gone = host
        .team()
        .call(call(parent, "agents.send", json!({"agent": "A1", "text": "再看看测试"})))
        .await
        .unwrap_err();
    assert_eq!(
        gone,
        "agents.send_failed: 消息没有送到：子 Agent A1 的会话已经不在了，可能已被删除。什么都没发出；用 agents.status 看现有的子 Agent。"
    );
}

/// A sub-agent whose turn does not stop in time stays marked stopped, and the call says so
/// instead of asking for another stop.
#[tokio::test]
async fn a_stop_that_did_not_finish_says_the_sub_agent_is_marked_stopped() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    spawn_reader(&host, parent, "调研").await;
    *host.halt_failure.lock().unwrap() = Some("agent.stop_pending: 上一步还没收尾".to_owned());
    let pending =
        host.team().call(call(parent, "agents.stop", json!({"agent": "A1"}))).await.unwrap_err();
    assert_eq!(
        pending,
        "agents.stop_pending: 子 Agent A1 已标记为停止，不会再接新的活，但它正在做的那一步还没收尾。不用再调 agents.stop；过一会儿用 agents.status 看它是否停下了。"
    );
    *host.halt_failure.lock().unwrap() = None;
    spawn_reader(&host, parent, "复核").await;
    *host.halt_failure.lock().unwrap() = Some("进程列表读不出来".to_owned());
    let unconfirmed =
        host.team().call(call(parent, "agents.stop", json!({"agent": "A2"}))).await.unwrap_err();
    assert!(
        unconfirmed.starts_with(
            "agents.stop_pending: 子 Agent A2 已标记为停止，不会再接新的活，但没能确认它已经停下（进程列表读不出来）。"
        ),
        "{unconfirmed}"
    );
    assert_eq!(host.count("SELECT count(*) FROM agent_team_members WHERE state='stopped'"), 2);
}

/// The step a merge failed in says whether the workspace could have changed; Git's unknown
/// outcome keeps its code, by which the call's effect is recorded as unknown.
#[tokio::test]
async fn a_merge_that_failed_says_whether_the_workspace_changed() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let child = spawn_writer(&host, parent).await;
    support::answered(&host.team(), child, "改好了：src/login.rs；cargo test -p login 退出码 0。")
        .await;
    let merge = || call(parent, "agents.merge", json!({"agent": "A1"}));
    *host.merge_failure.lock().unwrap() =
        Some((MergeStep::Commit, "index.lock 被别的 Git 进程占着".to_owned()));
    let commit = host.team().call(merge()).await.unwrap_err();
    assert_eq!(
        commit,
        "agents.merge_failed: 在工作副本里提交它的改动时出错：index.lock 被别的 Git 进程占着。主工作区没有改动，工作副本还在；处理好原因后再调一次 agents.merge。"
    );
    *host.merge_failure.lock().unwrap() =
        Some((MergeStep::BringIn, "git.outcome_unknown: Git 写入被中断，结果未知".to_owned()));
    let unknown = host.team().call(merge()).await.unwrap_err();
    assert_eq!(
        unknown,
        "agents.merge_failed: 把改动带进主工作区时 Git 的结果没能确认：git.outcome_unknown: Git 写入被中断，结果未知。主工作区可能已经改了一部分，工作副本还在；先用 git.status 看看主工作区，再决定怎么做。"
    );
    assert!(host.removed.lock().unwrap().is_empty());
    assert_eq!(worktree_of(&host, child).state, WorktreeState::Active);
}

/// A discard that could not stop the sub-agent leaves its worktree alone; one whose folder
/// could not be removed keeps the worktree and says what to do about the folder.
#[tokio::test]
async fn a_discard_that_failed_keeps_the_worktree_and_says_so() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let child = spawn_writer(&host, parent).await;
    let discard = || call(parent, "agents.discard", json!({"agent": "A1"}));
    *host.halt_failure.lock().unwrap() = Some("agent.stop_pending: 上一步还没收尾".to_owned());
    let running = host.team().call(discard()).await.unwrap_err();
    assert_eq!(
        running,
        "agents.discard_failed: 子 Agent A1 已标记为停止，但它正在做的那一步还没收尾，所以还没删它的工作副本，什么都没合回。过一会儿再调一次 agents.discard。"
    );
    assert!(host.removed.lock().unwrap().is_empty());
    *host.halt_failure.lock().unwrap() = None;
    *host.remove_failure.lock().unwrap() =
        Some("另一个程序正在使用此文件 (os error 32)".to_owned());
    let held = host.team().call(discard()).await.unwrap_err();
    let path = worktree_of(&host, child).details.path;
    assert_eq!(
        held,
        format!(
            "agents.discard_failed: 工作副本 {path} 没能删掉：另一个程序正在使用此文件 (os error 32)。子 Agent A1 已标记为停止，什么都没合回，工作副本还留着；如果有程序占着这个文件夹，请用户先关掉，再调一次 agents.discard。"
        )
    );
    assert_eq!(worktree_of(&host, child).state, WorktreeState::Active);
}

#[test]
fn a_cause_is_one_bounded_line_and_store_failures_are_said_in_words() {
    let long = format!("{}\n第二行", "长".repeat(300));
    assert_eq!(one_line(&long), format!("{}…", "长".repeat(240)));
    assert_eq!(one_line("拒绝访问。\u{7}\n第二行"), "拒绝访问");
    assert_eq!(one_line(" \n "), "没有说明原因");
    let conflict =
        anyhow::Error::from(PersistenceCoreError::Conflict).context("agents.approve.record");
    assert_eq!(cause_of(&conflict), "子 Agent 的记录刚被另一步同时改过，这一步没有保存");
    let chained =
        anyhow::anyhow!("拒绝访问。 (os error 5)").context("打开主工作区的 Git 仓库时出错");
    assert_eq!(cause_of(&chained), "打开主工作区的 Git 仓库时出错: 拒绝访问。 (os error 5)");
    // Git's unknown outcome is kept in the line even when it is not on the first one.
    let unknown = anyhow::anyhow!("Git 进程没有退出\ngit.outcome_unknown: 结果未知");
    assert_eq!(cause_of(&unknown), "git.outcome_unknown: 结果未知");
}

#[test]
fn the_main_agents_refusal_names_who_refused_and_why() {
    assert_eq!(
        main_agent_denial("  会删到用户自己的截图\n"),
        "agents.denied: 主 Agent 拒绝了这个调用，理由：「会删到用户自己的截图」。它没有执行，什么都没改。原样再调用多半还会被拒绝；可以换个做法，或用 agent.report 问主 Agent 该怎么做。"
    );
    let long = main_agent_denial(&"长".repeat(600));
    assert!(long.contains(&format!("「{}」", "长".repeat(500))), "{long}");
}
