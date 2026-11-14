//! What the desktop's 多 Agent page lists - the main agent first, then each sub-agent with its
//! newest report - and the user stopping a sub-agent from it, over a real execution store.
#![allow(clippy::unwrap_used)]
use crate::support;

use aura_agent_team::{AgentsWaitInput, LIST_SUMMARY_CHARS, MemberState, Progress, ReportStatus};
use aura_domain_common::{ModelProfileId, SessionId};
use serde_json::json;
use std::time::Duration;
use support::{FakeHost, call, task};

/// Starts sub-agent `A<n>` of `parent` and returns its conversation.
async fn started(host: &FakeHost, parent: SessionId) -> SessionId {
    host.team()
        .call(call(parent, "agents.spawn", json!({"name": "读代码", "task": task("找出入口")})))
        .await
        .unwrap();
    let newest = host.opened.lock().unwrap().len() - 1;
    host.child(newest)
}

#[tokio::test]
async fn the_main_agent_comes_first_with_its_model_and_what_it_is_doing() {
    let mut host = FakeHost::new();
    let pro = host.models[2].id;
    host.parent_model = Some(pro);
    let parent = SessionId::new();
    host.show(
        parent,
        Progress {
            working: true,
            tool: Some("agents.wait".into()),
            tokens: 1_234,
            ..Progress::default()
        },
    );
    let view = host.team().list(parent).await.unwrap();
    assert_eq!(view.lead.model_profile_id, Some(pro));
    assert_eq!(view.lead.model.as_deref(), Some("Pro"));
    assert!(view.lead.working);
    assert_eq!(view.lead.tool.as_deref(), Some("agents.wait"));
    assert_eq!(view.lead.tokens, 1_234);
    assert!(view.members.is_empty());
}

#[tokio::test]
async fn a_model_that_is_no_longer_enabled_is_listed_without_a_name() {
    let mut host = FakeHost::new();
    let gone = ModelProfileId::new();
    host.parent_model = Some(gone);
    let view = host.team().list(SessionId::new()).await.unwrap();
    assert_eq!(view.lead.model_profile_id, Some(gone));
    assert_eq!(view.lead.model, None);
    assert!(!view.lead.working);
}

#[tokio::test]
async fn a_sub_agent_shows_its_newest_report_and_the_question_it_asks() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let child = started(&host, parent).await;
    let team = host.team();
    team.call(call(child, "agent.report", json!({"status": "progress", "summary": "读了 README"})))
        .await
        .unwrap();
    host.show(
        child,
        Progress {
            working: true,
            tool: Some("fs.read".into()),
            tokens: 4_321,
            ..Progress::default()
        },
    );
    let member = team.list(parent).await.unwrap().members.remove(0);
    assert_eq!(member.state, MemberState::Running);
    assert_eq!(member.last_report, Some(ReportStatus::Progress));
    assert_eq!(member.summary.as_deref(), Some("读了 README"));
    assert_eq!(member.question, None);
    assert_eq!(member.tool.as_deref(), Some("fs.read"));
    assert_eq!(member.tokens, 4_321);

    team.call(call(
        child,
        "agent.report",
        json!({"status": "waiting", "summary": "要先定分支", "question": "用哪个分支？"}),
    ))
    .await
    .unwrap();
    let member = team.list(parent).await.unwrap().members.remove(0);
    assert_eq!(member.state, MemberState::Reported);
    assert_eq!(member.last_report, Some(ReportStatus::Waiting));
    assert_eq!(member.summary.as_deref(), Some("要先定分支"));
    assert_eq!(member.question.as_deref(), Some("用哪个分支？"));
}

#[tokio::test]
async fn a_long_summary_is_cut_for_the_list() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let child = started(&host, parent).await;
    let long = "长".repeat(LIST_SUMMARY_CHARS * 3);
    support::answered(&host.team(), child, &long).await;
    let summary = host.team().list(parent).await.unwrap().members.remove(0).summary.unwrap();
    assert_eq!(summary.chars().count(), LIST_SUMMARY_CHARS);
    assert!(summary.ends_with('…'), "{summary}");
}

#[tokio::test]
async fn the_user_stops_a_sub_agent_and_the_main_agent_is_told_in_a_report() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let child = started(&host, parent).await;
    let team = host.team();
    team.stop_for_user(parent, "a1").await.unwrap();
    assert_eq!(*host.halted.lock().unwrap(), vec![child]);
    // The user stopped it: its next turn says so without a note of the team's.
    assert_eq!(*host.stop_notes.lock().unwrap(), vec![(child, None)]);
    let member = team.list(parent).await.unwrap().members.remove(0);
    assert_eq!(member.state, MemberState::Stopped);
    assert_eq!(member.last_report, Some(ReportStatus::Failed));
    let waited = team.call(call(parent, "agents.wait", json!({"agents": ["A1"]}))).await.unwrap();
    assert_eq!(waited["timed_out"], false);
    let report = &waited["reports"][0]["report"];
    assert_eq!(report["status"], "failed");
    let summary = report["summary"].as_str().unwrap();
    assert!(summary.contains("用户在多 Agent 页停止了这个子 Agent"), "{report}");
    // Stopped once: stopping it again halts nothing and reports nothing more.
    team.stop_for_user(parent, "A1").await.unwrap();
    assert_eq!(host.halted.lock().unwrap().len(), 1);
    assert_eq!(host.count("SELECT count(*) FROM agent_team_reports"), 1);
}

#[tokio::test(start_paused = true)]
async fn a_main_agent_waiting_for_the_sub_agent_wakes_when_the_user_stops_it() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    started(&host, parent).await;
    let team = host.team();
    let long = AgentsWaitInput { agents: vec!["A1".into()], timeout_seconds: Some(600) };
    let started = tokio::time::Instant::now();
    let stop = async {
        tokio::time::sleep(Duration::from_secs(5)).await;
        team.stop_for_user(parent, "A1").await.unwrap();
    };
    let ((), waited) = tokio::join!(stop, team.prewait(parent, &long));
    waited.unwrap();
    assert!(started.elapsed() < Duration::from_secs(30), "{:?}", started.elapsed());
}

#[tokio::test]
async fn the_user_stops_only_a_sub_agent_of_the_conversation_named() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    started(&host, parent).await;
    let refused = host.team().stop_for_user(SessionId::new(), "A1").await.unwrap_err();
    assert!(refused.to_string().contains("agents.unknown_agent"), "{refused}");
    assert!(host.halted.lock().unwrap().is_empty());
}

#[tokio::test]
async fn stopping_a_sub_agent_that_already_reported_keeps_its_report_and_tells_the_main_agent() {
    let host = FakeHost::new();
    let parent = SessionId::new();
    let child = started(&host, parent).await;
    let team = host.team();
    support::answered(&team, child, "在 src/main.rs").await;
    team.stop_for_user(parent, "A1").await.unwrap();
    let waited = team.call(call(parent, "agents.wait", json!({}))).await.unwrap();
    let reports = waited["reports"].as_array().unwrap();
    assert_eq!(reports.len(), 2, "{waited}");
    assert_eq!(reports[0]["report"]["summary"], "在 src/main.rs");
    assert_eq!(reports[1]["report"]["status"], "failed");
    // A stopped sub-agent takes no more messages.
    let refused = team
        .call(call(parent, "agents.send", json!({"agent": "A1", "text": "再看看"})))
        .await
        .unwrap_err();
    assert!(refused.contains("agents.stopped"), "{refused}");
}
