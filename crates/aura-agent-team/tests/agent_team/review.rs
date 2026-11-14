//! The main agent answering a sub-agent's question first: agents.wait ends for a question under
//! review and gives it, agents.status gives it too, agents.approve answers it once or hands it to
//! the user, and the list keeps the answer with its reason. Over a real execution store; which
//! questions are under review is the host's to say.
#![allow(clippy::unwrap_used)]
use crate::support;

use aura_agent_team::{ApprovalAnswer, Progress, ReviewRequest};
use aura_domain_common::{ApprovalRequestId, SessionId, UtcTimestamp};
use serde_json::json;
use std::time::Duration;
use support::{FakeHost, call, task};

/// A main conversation with one sub-agent, A1, whose question `approval` is under review.
async fn asking(host: &FakeHost) -> (SessionId, SessionId, ApprovalRequestId) {
    let parent = SessionId::new();
    host.team()
        .call(call(parent, "agents.spawn", json!({"name": "清理", "task": task("清理")})))
        .await
        .unwrap();
    let child = host.child(0);
    let approval = ApprovalRequestId::new();
    host.working.lock().unwrap().insert(child);
    host.shown.lock().unwrap().insert(
        child,
        Progress {
            working: true,
            reviewing: vec![ReviewRequest {
                approval_id: approval,
                tool: "shell.powershell".to_owned(),
                request: "Remove-Item $png".to_owned(),
                purpose: Some("删掉旧截图".to_owned()),
                deletes: true,
                allow_workspace: true,
                asked_at: UtcTimestamp::now(),
            }],
            ..Progress::default()
        },
    );
    (parent, child, approval)
}

#[tokio::test(start_paused = true)]
async fn a_question_under_review_ends_the_wait_and_is_given_with_what_it_does() {
    let host = FakeHost::new();
    let (parent, _, approval) = asking(&host).await;
    let input = json!({"agents": ["A1"], "timeout_seconds": 30});
    let started = tokio::time::Instant::now();
    host.team().prewait(parent, &serde_json::from_value(input.clone()).unwrap()).await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(10), "the wait ended at once");
    let waited = host.team().call(call(parent, "agents.wait", input)).await.unwrap();
    assert_eq!(waited["timed_out"], false);
    let given = &waited["approvals"][0];
    assert_eq!(given["agent"], "A1");
    assert_eq!(given["approval_id"], json!(approval));
    assert_eq!(
        (&given["request"], &given["purpose"]),
        (&json!("Remove-Item $png"), &json!("删掉旧截图"))
    );
    assert_eq!(given["deletes"], true);
    assert!(given["seconds_left"].as_u64().is_some_and(|left| left > 100 && left <= 120));
    // Once given, it no longer ends a wait at once; agents.status still lists it.
    let status = host.team().call(call(parent, "agents.status", json!({}))).await.unwrap();
    assert_eq!(status["approvals"][0]["approval_id"], json!(approval));
    let short = json!({"agents": ["A1"], "timeout_seconds": 1});
    let started = tokio::time::Instant::now();
    host.team().prewait(parent, &serde_json::from_value(short).unwrap()).await.unwrap();
    assert!(started.elapsed() >= Duration::from_secs(1), "it waited out its time");
}

#[tokio::test]
async fn the_main_agents_answer_covers_the_one_call_and_is_kept_with_its_reason() {
    let host = FakeHost::new();
    let (parent, child, approval) = asking(&host).await;
    let answered = host
        .team()
        .call(call(
            parent,
            "agents.approve",
            json!({"agent": "A1", "approval_id": approval, "answer": "allow", "reason": "只删它自己工作副本里的截图"}),
        ))
        .await
        .unwrap();
    assert_eq!((&answered["agent"], &answered["answer"]), (&json!("A1"), &json!("allow")));
    assert_eq!(*host.answered.lock().unwrap(), vec![(child, approval, ApprovalAnswer::Allow)]);
    let listed = host.team().list(parent).await.unwrap();
    let member = &listed.members[0];
    assert!(member.reviewing.is_empty() && member.approvals.is_empty());
    let kept = member.reviews[0].answered.as_ref().unwrap();
    assert_eq!(kept.reason, "只删它自己工作副本里的截图");
    // Answered, it is no longer the main agent's to answer.
    let again = host
        .team()
        .call(call(
            parent,
            "agents.approve",
            json!({"agent": "A1", "approval_id": approval, "answer": "deny", "reason": "再答一次"}),
        ))
        .await
        .unwrap_err();
    assert!(again.starts_with("agents.approval_not_reviewed"), "{again}");
}

/// allow_workspace answers as the user's card's allowance would, on a question the card offers
/// it for; the question says so.
#[tokio::test]
async fn allow_workspace_answers_as_the_cards_allowance_where_it_is_offered() {
    let host = FakeHost::new();
    let (parent, child, approval) = asking(&host).await;
    let status = host.team().call(call(parent, "agents.status", json!({}))).await.unwrap();
    assert_eq!(status["approvals"][0]["allow_workspace"], true, "{status}");
    let answered = host
        .team()
        .call(call(
            parent,
            "agents.approve",
            json!({"agent": "A1", "approval_id": approval, "answer": "allow_workspace", "reason": "删它为检查建的临时文件，之后还会再删"}),
        ))
        .await
        .unwrap();
    assert_eq!(answered["answer"], "allow_workspace");
    assert_eq!(
        *host.answered.lock().unwrap(),
        vec![(child, approval, ApprovalAnswer::AllowWorkspace)]
    );
    let member = &host.team().list(parent).await.unwrap().members[0];
    assert_eq!(member.reviews[0].answered.as_ref().unwrap().answer, ApprovalAnswer::AllowWorkspace);
}

/// On a question the card offers no allowance for, allow_workspace is refused with the answers
/// that may be given, and nothing is answered.
#[tokio::test]
async fn allow_workspace_is_refused_where_the_card_does_not_offer_it() {
    let host = FakeHost::new();
    let (parent, _, approval) = asking(&host).await;
    for progress in host.shown.lock().unwrap().values_mut() {
        for request in &mut progress.reviewing {
            request.allow_workspace = false;
        }
    }
    let status = host.team().call(call(parent, "agents.status", json!({}))).await.unwrap();
    assert_eq!(status["approvals"][0]["allow_workspace"], false, "{status}");
    let refused = host
        .team()
        .call(call(
            parent,
            "agents.approve",
            json!({"agent": "A1", "approval_id": approval, "answer": "allow_workspace", "reason": "理由"}),
        ))
        .await
        .unwrap_err();
    assert!(refused.starts_with("agents.approve_invalid"), "{refused}");
    assert!(refused.contains("allow、deny 或 ask_user"), "{refused}");
    assert!(host.answered.lock().unwrap().is_empty());
}

#[tokio::test]
async fn handed_to_the_user_it_is_the_users_question_and_nothing_is_answered() {
    let host = FakeHost::new();
    let (parent, _, approval) = asking(&host).await;
    assert_eq!(host.team().list(parent).await.unwrap().members[0].reviewing, vec![approval]);
    host.team()
        .call(call(
            parent,
            "agents.approve",
            json!({"agent": "A1", "approval_id": approval, "answer": "ask_user", "reason": "要动的文件看不出来"}),
        ))
        .await
        .unwrap();
    assert!(host.answered.lock().unwrap().is_empty());
    let member = &host.team().list(parent).await.unwrap().members[0];
    assert_eq!((member.approvals.clone(), member.reviewing.clone()), (vec![approval], vec![]));
    let status = host.team().call(call(parent, "agents.status", json!({}))).await.unwrap();
    assert_eq!(status["agents"][0]["waiting_for_approval"], true);
    assert!(status.get("approvals").is_none(), "{status}");
}

#[tokio::test]
async fn an_answer_needs_a_reason_and_a_question_under_review() {
    let host = FakeHost::new();
    let (parent, child, approval) = asking(&host).await;
    let approve = |id: ApprovalRequestId, reason: &str| {
        call(
            parent,
            "agents.approve",
            json!({"agent": "A1", "approval_id": id, "answer": "allow", "reason": reason}),
        )
    };
    let blank = host.team().call(approve(approval, "  ")).await.unwrap_err();
    assert!(blank.starts_with("agents.approve_invalid"), "{blank}");
    let other = host.team().call(approve(ApprovalRequestId::new(), "理由")).await.unwrap_err();
    assert!(other.starts_with("agents.approval_not_reviewed"), "{other}");
    // A sub-agent answers nothing.
    let own = call(
        child,
        "agents.approve",
        json!({"agent": "A1", "approval_id": approval, "answer": "allow", "reason": "理由"}),
    );
    assert!(host.team().call(own).await.is_err());
    assert!(host.answered.lock().unwrap().is_empty());
}

/// A refusal goes to the host with its reason, which the sub-agent reads as its main agent's.
#[tokio::test]
async fn a_refusal_carries_its_reason() {
    let host = FakeHost::new();
    let (parent, child, approval) = asking(&host).await;
    host.team()
        .call(call(
            parent,
            "agents.approve",
            json!({"agent": "A1", "approval_id": approval, "answer": "deny", "reason": "会删到用户自己的截图"}),
        ))
        .await
        .unwrap();
    assert_eq!(*host.answered.lock().unwrap(), vec![(child, approval, ApprovalAnswer::Deny)]);
    assert_eq!(*host.reasons.lock().unwrap(), vec!["会删到用户自己的截图".to_owned()]);
}

/// An answer that could not be given says it did not take effect and what to look at, and is
/// not kept as given.
#[tokio::test]
async fn an_answer_that_did_not_take_effect_says_so() {
    let host = FakeHost::new();
    let (parent, _, approval) = asking(&host).await;
    let deny = || {
        call(
            parent,
            "agents.approve",
            json!({"agent": "A1", "approval_id": approval, "answer": "deny", "reason": "理由"}),
        )
    };
    *host.answer_failure.lock().unwrap() = Some(
        "agent.approval_run_missing: 这个审批请求没有对应的任务，没法在这里回答；什么都没改。"
            .to_owned(),
    );
    let gone = host.team().call(deny()).await.unwrap_err();
    assert!(gone.starts_with("agents.approve_failed: A1 已经不在等这个审批了"), "{gone}");
    assert!(gone.contains("没有生效") && gone.contains("agents.status"), "{gone}");
    *host.answer_failure.lock().unwrap() = Some("Core 没有回应".to_owned());
    let failed = host.team().call(deny()).await.unwrap_err();
    assert!(
        failed.starts_with("agents.approve_failed: 回答 A1 的审批请求没有完成：Core 没有回应。"),
        "{failed}"
    );
    assert!(host.answered.lock().unwrap().is_empty());
    let member = &host.team().list(parent).await.unwrap().members[0];
    assert!(member.reviews.iter().all(|review| review.answered.is_none()));
}
