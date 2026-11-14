//! The model's side of a conversation with sub-agents. A request that offers `agent.report` is
//! the sub-agent's; every other one is the conversation's. Each side picks its next step from the
//! tool results and messages its request already holds.
use super::*;

/// What the conversation's model does after it starts sub-agent A1, and what A1 does.
#[derive(Clone, Copy, Debug)]
pub enum TeamPlan {
    /// Start A1, wait for its report, answer. A1 reports ready, then answers.
    Report,
    /// Start A1 and wait; A1 asks a question. Send the answer and wait again, then answer. A1
    /// reports ready in the turn the answer starts.
    Send,
    /// Start A1 and stop it while its first request is held, then answer.
    Stop,
    /// Start A1 and wait for it; the user stops A1 while its first request is held. Then answer.
    UserStop,
    /// Start a sub-agent with a task sheet that names another conversation, then answer.
    SheetNamesConversation,
    /// Start A1 on a task sheet carrying the path the user's message names, wait, answer. A1
    /// asks for another conversation's history and reads that path, then reports.
    Refusals,
    /// Start A1 on a task sheet carrying the path the user's message names, wait, answer. A1
    /// reads that path, which waits for the user, then reports.
    Approval,
    /// Start A1 in a worktree on a task sheet carrying the path the user's message names, wait,
    /// merge A1, answer. A1 writes a file in its copy, then that path, then reports.
    Worktree,
    /// Start A1, wait for its report, answer. A1's first [`RATE_LIMITED`] requests, one step's
    /// every attempt, are refused with HTTP 429; then it reports ready.
    RateLimited,
    /// Start A1, wait for it, answer. A1 reads its to-do list again and again, each answer counted
    /// as [`BUDGET_ANSWER`] tokens of request; its third request is held until its turn is
    /// stopped.
    Budget,
    /// Start A1 on a task sheet carrying the path the user's message names, wait, answer. A1
    /// loads the Skill [`SKILL_NAME`], reads that path, then reports.
    Skill,
    /// Start A1 in a worktree, wait, answer. A1 loads the Skill [`SKILL_NAME`], then reports.
    WorktreeSkill,
    /// Start A1 with its task sheet written as a string of JSON that goes on with the tools
    /// field, as a model once sent it; wait, answer. A1 reports at once.
    StringTask,
    /// Start A1 in a worktree, wait, open the file A1 added in its copy in the browser, answer.
    /// A1 writes that file, then reports.
    OpenCopy,
    /// Start A1 in a worktree, wait, delete a file in A1's copy and the path the user's message
    /// names, answer. A1 writes its temporary folder and deletes in its copy with a command, runs
    /// one in its copy named in full, writes and reads its temporary folder with the file tools,
    /// reads a picture, then reports. See [`own`].
    OwnPlaces,
    /// Start A1 in the workspace and wait for it, answer. A1 writes two files; the user changes
    /// its mode between them. See [`mode`].
    ModeSwitch,
    /// Start A1 in a worktree, wait, answer. A1 deletes in its copy by variable twice, then the
    /// path the user's message names; the user lets it delete there after the first. See
    /// [`allowance`].
    AllowanceCard,
    /// As [`TeamPlan::AllowanceCard`] under the settings' presets: A1 deletes by variable, writes
    /// the path the user's message names, then deletes it.
    AllowancePresets,
    /// Start A1 in a worktree, wait, allow the question the wait gives, wait again, answer. A1
    /// deletes in its copy by variable, then reports. See [`review`].
    ReviewAllow,
    /// As [`TeamPlan::ReviewAllow`], handing the question to the user instead.
    ReviewAskUser,
    /// As [`TeamPlan::ReviewAllow`], answering with allow_workspace; A1 then deletes in its copy
    /// by variable once more before it reports.
    ReviewAllowWorkspace,
    /// Start A1 in a worktree, wait, answer. A1 fetches in its copy, then reports.
    DownloadPreset,
}

#[path = "team_allowance.rs"]
pub(super) mod allowance;

#[path = "team_mode.rs"]
pub(super) mod mode;
#[path = "team_own.rs"]
pub(super) mod own;
#[path = "team_review.rs"]
pub(super) mod review;

/// The Skill A1 loads under [`TeamPlan::Skill`] and [`TeamPlan::WorktreeSkill`].
pub const SKILL_NAME: &str = "team-guide";

/// The requests of A1's that [`TeamPlan::RateLimited`] refuses for their rate.
pub const RATE_LIMITED: usize = 3;

/// The request tokens the provider counts for each answer of A1's under [`TeamPlan::Budget`].
pub const BUDGET_ANSWER: u64 = 60_000;

/// The file A1 adds in its worktree, and what it writes there.
pub const ADDED_FILE: &str = "added.txt";
pub const ADDED_TEXT: &str = "from A1\n";

/// The sheet A1 is given; its goal carries the path the user's message names, if any.
const GOAL: &str = "读代码，说出入口在哪个文件";

/// The question A1 asks and the answer the conversation sends.
pub const QUESTION: &str = "用哪个分支？";
pub const ANSWER: &str = "用 main 分支。";
/// What A1's ready report says.
pub const SUMMARY: &str = "入口在 src/main.rs。";

/// Whether the request offers `tool`.
pub fn offers(body: &Value, tool: &str) -> bool {
    let entry = aura_tool_registry::agent_release::release()
        .expect("release")
        .entry(tool)
        .expect("a released tool")
        .definition
        .tool_id;
    let wire = provider_wire_tool_name(tool, entry);
    body["tools"].as_array().into_iter().flatten().any(|t| t["function"]["name"] == wire)
}

/// Whether the sub-agent's request of this plan is held until its turn is stopped.
pub(in crate::agent_application::tests::provider_fixture) fn holds(
    plan: TeamPlan,
    body: &Value,
) -> bool {
    offers(body, "agent.report")
        && match plan {
            TeamPlan::Stop | TeamPlan::UserStop => true,
            TeamPlan::Budget => results(body) >= 2,
            _ => false,
        }
}

fn results(body: &Value) -> usize {
    body["messages"].as_array().into_iter().flatten().filter(|m| m["role"] == "tool").count()
}

fn said(body: &Value, prefix: &str) -> bool {
    body["messages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|m| m["role"] == "user")
        .filter_map(|m| m["content"].as_str())
        .any(|text| text.starts_with(prefix))
}

fn answer(text: &str) -> Value {
    json!({"role":"assistant","content":text})
}

pub(in crate::agent_application::tests::provider_fixture) fn reply(
    plan: TeamPlan,
    body: &Value,
    call: &impl Fn(&str, Value, usize) -> Value,
) -> Value {
    let calling = |tool: &str, arguments: Value| json!({"role":"assistant","tool_calls":[call(tool, arguments, 0)]});
    if matches!(plan, TeamPlan::OwnPlaces) {
        return if offers(body, "agent.report") {
            own::sub_agent(body, &calling)
        } else {
            own::main(body, &calling)
        };
    }
    if matches!(plan, TeamPlan::AllowanceCard | TeamPlan::AllowancePresets) {
        return match (offers(body, "agent.report"), plan) {
            (false, _) => allowance::main(body, &calling),
            (true, TeamPlan::AllowanceCard) => allowance::card(body, &calling),
            (true, _) => allowance::presets(body, &calling),
        };
    }
    if matches!(
        plan,
        TeamPlan::ReviewAllow
            | TeamPlan::ReviewAskUser
            | TeamPlan::ReviewAllowWorkspace
            | TeamPlan::DownloadPreset
    ) {
        return match (offers(body, "agent.report"), plan) {
            (false, TeamPlan::ReviewAllow) => review::main(body, &calling, "allow"),
            (false, TeamPlan::ReviewAskUser) => review::main(body, &calling, "ask_user"),
            (false, TeamPlan::ReviewAllowWorkspace) => {
                review::main(body, &calling, "allow_workspace")
            }
            (false, _) => allowance::main(body, &calling),
            (true, TeamPlan::DownloadPreset) => review::downloads(body, &calling),
            (true, TeamPlan::ReviewAllowWorkspace) => review::deletes_twice(body, &calling),
            (true, _) => review::deletes(body, &calling),
        };
    }
    if matches!(plan, TeamPlan::ModeSwitch) {
        return if offers(body, "agent.report") {
            mode::sub_agent(body, &calling)
        } else {
            mode::main(body, &calling)
        };
    }
    if offers(body, "agent.report") {
        sub_agent(plan, body, &calling)
    } else {
        main(plan, body, &calling)
    }
}

fn main(plan: TeamPlan, body: &Value, calling: &impl Fn(&str, Value) -> Value) -> Value {
    let wait = || calling("agents.wait", json!({"agents":["A1"],"timeout_seconds":60}));
    let goal = match plan {
        TeamPlan::Refusals | TeamPlan::Approval | TeamPlan::Worktree | TeamPlan::Skill => {
            format!("{GOAL}；先读 AURA_EXTERNAL_PATH={}", named_external_path(body))
        }
        TeamPlan::SheetNamesConversation => "读 conv_0123456789abcdef 里的结论".to_owned(),
        _ => GOAL.to_owned(),
    };
    let spawn = || {
        let sheet = json!({"goal":goal,"done_when":"说出入口文件"});
        let mut input = json!({"name":"读代码","task":sheet});
        if matches!(plan, TeamPlan::Worktree | TeamPlan::WorktreeSkill | TeamPlan::OpenCopy) {
            input["worktree"] = json!(true);
        }
        if matches!(plan, TeamPlan::StringTask) {
            input["task"] = json!(format!("{sheet},\"tools\":[\"fs.read\"]}}"));
        }
        calling("agents.spawn", input)
    };
    match (plan, results(body)) {
        (_, 0) => spawn(),
        (TeamPlan::SheetNamesConversation, _) => answer("没有派。"),
        (TeamPlan::Stop, 1) => calling("agents.stop", json!({"agent":"A1"})),
        (TeamPlan::Send, 2) => calling("agents.send", json!({"agent":"A1","text":ANSWER})),
        (TeamPlan::Send, 1 | 3) | (_, 1) => wait(),
        (TeamPlan::Worktree, 2) => calling("agents.merge", json!({"agent":"A1"})),
        (TeamPlan::OpenCopy, 2) => calling("browser.open", json!({"path":ADDED_FILE,"agent":"A1"})),
        _ => answer("收到。"),
    }
}

fn sub_agent(plan: TeamPlan, body: &Value, calling: &impl Fn(&str, Value) -> Value) -> Value {
    // A sub-agent hands in its work with its final answer.
    let ready = || answer(SUMMARY);
    match (plan, results(body)) {
        (TeamPlan::Send, 0) => calling(
            "agent.report",
            json!({"status":"waiting","summary":"要先定分支","question":QUESTION}),
        ),
        (TeamPlan::Send, 1) if said(body, "来自主 Agent：") => ready(),
        (TeamPlan::Send, 1) => answer("等主 Agent 回复。"),
        // Its own history it may read; this names another conversation.
        (TeamPlan::Refusals, 0) => calling(
            "history.read",
            json!({"conversation": "conv_0123456789abcdef", "entries": ["T1.1"]}),
        ),
        (TeamPlan::Refusals, 1) | (TeamPlan::Approval, 0) => calling(
            "fs.read",
            json!({
                "path": named_external_path(body),
                "range": {"range_kind": "full"},
                "explicit_encoding": null
            }),
        ),
        (TeamPlan::Worktree | TeamPlan::OpenCopy, 0) => calling(
            "fs.write",
            json!({"path":ADDED_FILE,"text":ADDED_TEXT,"expected":{"expected_kind":"absent"}}),
        ),
        (TeamPlan::Worktree, 1) => calling(
            "fs.write",
            json!({
                "path": named_external_path(body),
                "text": ADDED_TEXT,
                "expected": {"expected_kind": "absent"}
            }),
        ),
        (TeamPlan::Budget, _) => calling("todo.get", json!({})),
        (TeamPlan::Skill | TeamPlan::WorktreeSkill, 0) => {
            calling("skills.load", json!({"name": SKILL_NAME}))
        }
        (TeamPlan::Skill, 1) => calling(
            "fs.read",
            json!({
                "path": named_external_path(body),
                "range": {"range_kind": "full"},
                "explicit_encoding": null
            }),
        ),
        (TeamPlan::Refusals, 2)
        | (TeamPlan::Report | TeamPlan::RateLimited | TeamPlan::StringTask, 0)
        | (TeamPlan::Approval, 1)
        | (TeamPlan::Worktree, 2)
        | (TeamPlan::OpenCopy, 1)
        | (TeamPlan::Skill, 2)
        | (TeamPlan::WorktreeSkill, 1) => ready(),
        _ => answer("交了。"),
    }
}
