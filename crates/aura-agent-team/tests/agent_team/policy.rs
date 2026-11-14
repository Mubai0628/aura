//! What a sub-agent may be given and what it is refused, and the model it gets.
#![allow(clippy::unwrap_used)]

use aura_agent_team::{
    ModelOffer, TaskSheet, choose_model,
    policy::{granted_tools, main_agent_only, names_aura_data, refused_call},
    session_reference,
};
use aura_domain_common::ModelProfileId;
use serde_json::json;
use std::path::PathBuf;

fn own() -> Vec<String> {
    [
        "fs.read",
        "fs.write",
        "fs.search",
        "shell.powershell",
        "browser.open",
        "memory.create",
        "skills.load",
        "skills.search",
        "computer.act",
        "chrome.click",
        "agents.spawn",
        "query.user_question",
    ]
    .map(str::to_owned)
    .to_vec()
}

fn names(tools: &[&str]) -> Option<Vec<String>> {
    Some(tools.iter().map(|tool| (*tool).to_owned()).collect())
}

#[test]
fn a_sub_agent_has_every_tool_unless_the_main_agent_narrows_it() {
    // None named, or an empty list: every tool a sub-agent may have, as its table decides.
    assert_eq!(granted_tools(None, &own()).unwrap(), None);
    assert_eq!(granted_tools(Some(&[]), &own()).unwrap(), None);
    // Some named: those once each, and the Skill tools the starter has.
    let asked = ["fs.write", "browser.open", "fs.write"].map(str::to_owned).to_vec();
    assert_eq!(
        granted_tools(Some(&asked), &own()).unwrap(),
        names(&["browser.open", "fs.write", "skills.load", "skills.search"])
    );
    // Never a tool the starter lacks, or one only the main agent has.
    for outside in [
        "web.search",
        "computer.act",
        "chrome.click",
        "agents.spawn",
        "query.user_question",
        "agent.report",
    ] {
        let refused = granted_tools(Some(&[outside.to_owned()]), &own()).unwrap_err();
        assert_eq!(refused.code, "agents.tool_not_available", "{outside}");
    }
    // A starter without the Skill tools gives none.
    let plain = vec!["fs.read".to_owned()];
    assert_eq!(granted_tools(Some(&plain), &plain).unwrap(), Some(plain.clone()));
}

#[test]
fn only_the_main_agent_ends_the_turn_asks_the_user_and_drives_the_users_desktop_and_chrome() {
    for tool in [
        "agents.spawn",
        "computer.act",
        "chrome.click",
        "plugin.chrome-control.click",
        "task_lineage.reconcile",
        "goal.complete",
        "normal.complete",
        "mode.disposition",
        "plan.complete",
        "workspace_init.complete",
        "query.user_question",
        "terminal.request_sensitive_input",
        "browser.request_user",
    ] {
        assert!(main_agent_only(tool), "{tool}");
    }
    for tool in [
        "fs.write",
        "fs.delete",
        "browser.open",
        "history.read",
        "tools.load",
        "memory.create",
        "agent.report",
    ] {
        assert!(!main_agent_only(tool), "{tool}");
    }
}

#[test]
fn a_sub_agent_is_refused_other_conversations_the_main_agents_tools_and_tools_narrowed_away() {
    let refused = |tool: &str, arguments: serde_json::Value, narrowed: Option<&[String]>| {
        refused_call(tool, &arguments, narrowed).map(|refusal| refusal.code)
    };
    assert_eq!(
        refused(
            "history.read",
            json!({"conversation": "conv_0123456789abcdef", "ids": ["T1"]}),
            None
        ),
        Some("agents.other_conversation")
    );
    assert_eq!(refused("history.read", json!({"ids": ["T1"]}), None), None);
    assert_eq!(refused("agents.spawn", json!({}), None), Some("agents.not_for_subagents"));
    assert_eq!(refused("computer.act", json!({}), None), Some("agents.main_agent_only"));
    assert_eq!(refused("query.user_question", json!({}), None), Some("agents.main_agent_only"));
    assert_eq!(refused("fs.write", json!({"path": "a.txt"}), None), None);
    assert_eq!(refused("agent.report", json!({}), None), None);
    let narrowed = vec!["fs.read".to_owned()];
    assert_eq!(
        refused("fs.write", json!({"path": "a.txt"}), Some(&narrowed)),
        Some("agents.tool_not_granted")
    );
    assert_eq!(refused("fs.read", json!({"path": "a.txt"}), Some(&narrowed)), None);
    assert_eq!(refused("agent.report", json!({}), Some(&narrowed)), None);
}

#[test]
fn a_command_naming_aura_data_is_seen_however_the_folder_is_written() {
    let folders =
        vec![PathBuf::from(r"C:\Users\me\AppData\Local\Aura"), PathBuf::from(r"D:\AuraData")];
    let variable = |name: &str| match name.to_ascii_uppercase().as_str() {
        "LOCALAPPDATA" => Some(r"C:\Users\me\AppData\Local".to_owned()),
        "USERPROFILE" => Some(r"C:\Users\me".to_owned()),
        _ => None,
    };
    for script in [
        r"Get-Content $env:LOCALAPPDATA\Aura\logs\core.jsonl",
        r"type %LOCALAPPDATA%\Aura\aura.db",
        r#"Get-ChildItem "${env:LocalAppData}/Aura""#,
        r"dir c:/users/ME/appdata/local/aura/",
        r"cat ~\AppData\Local\Aura\v2\aura-v2.db",
        r"Copy-Item D:\AuraData\aura.db .",
        r"sqlite3 \\?\C:\Users\me\AppData\Local\Aura\aura.db",
    ] {
        assert!(names_aura_data(script, &folders, &variable).is_some(), "{script}");
    }
    for script in [
        r"cargo test -p aura-core",
        r"dir C:\Users\me\AppData\Local\Aura-dev",
        r"dir C:\Users\me\AuraWorktrees",
        r"type D:\AuraDataBackup\notes.txt",
    ] {
        assert!(names_aura_data(script, &folders, &variable).is_none(), "{script}");
    }
}

#[test]
fn a_task_sheet_needs_a_goal_and_an_end_and_names_no_conversation() {
    let sheet = |goal: &str| TaskSheet {
        goal: goal.into(),
        done_when: "列出文件".into(),
        scope: Some("src/".into()),
        constraints: vec!["不改代码".into()],
        files: vec!["src/login.rs".into()],
        background: None,
    };
    assert!(sheet("找登录流程").check().is_ok());
    assert_eq!(sheet(" ").check().unwrap_err().code, "agents.task_incomplete");
    assert_eq!(
        sheet("读 conv_0123456789abcdef").check().unwrap_err().code,
        "agents.task_names_conversation"
    );
    assert_eq!(session_reference("x conv_0123456789abcdefg"), None);
    assert_eq!(session_reference("conversation_id 和 session_id"), None);
    let text = sheet("找登录流程").render("调研");
    for heading in ["## 目标", "## 做到什么算完", "## 范围", "## 约束", "## 要看的文件"]
    {
        assert!(text.contains(heading), "{text}");
    }
    assert!(!text.contains("## 背景"), "{text}");
}

#[test]
fn the_model_is_one_the_user_allowed_for_sub_agents() {
    let model = |name: &str, allowed| ModelOffer {
        id: ModelProfileId::new(),
        name: name.into(),
        for_subagents: allowed,
    };
    let models = vec![model("Big", false), model("Flash", true), model("Pro", true)];
    assert_eq!(choose_model(&models, None, None).unwrap().name, "Flash");
    assert_eq!(choose_model(&models, None, Some(models[2].id)).unwrap().name, "Pro");
    assert_eq!(choose_model(&models, None, Some(models[0].id)).unwrap().name, "Flash");
    assert_eq!(choose_model(&models, Some("pro"), None).unwrap().name, "Pro");
    assert_eq!(
        choose_model(&models, Some("Big"), None).unwrap_err().code,
        "agents.model_not_allowed"
    );
    assert_eq!(choose_model(&models[..1], None, None).unwrap_err().code, "agents.no_model");
}
