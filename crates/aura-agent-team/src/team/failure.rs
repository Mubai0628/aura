//! What the model reads when a team call fails. A refusal, or a failure a step already put in the
//! team's own words, is passed on as it is; any other error is said with what the call was doing,
//! what it may have left changed and what usually helps next, its cause cut to one bounded line.
//! The steps that know more (starting, sending, stopping, merging, discarding, answering an
//! approval) say it here too.
use crate::{
    APPROVE, ApprovalAnswer, DISCARD, MERGE, Member, MergeStep, Refusal, Worktree,
    tools::{REPORT, RESULT, SEND, SPAWN, STATUS, STOP, WAIT},
};
use aura_persistence_core::PersistenceCoreError;

/// The most characters of a cause quoted after one of Aura's own sentences.
const CAUSE_CHARS: usize = 240;

/// The most characters of the main agent's reason a refused sub-agent is given.
const REASON_CHARS: usize = 500;

/// The code a Git write carries when its end was not seen: what it changed is unknown, and the
/// execution layer records the call's effect as unknown by this code.
const GIT_UNKNOWN: &str = "git.outcome_unknown";

/// The code of a Git process whose stop was not confirmed.
const GIT_STOP_UNCONFIRMED: &str = "git.stop_unconfirmed";

const STORE_CONFLICT: &str = "子 Agent 的记录刚被另一步同时改过，这一步没有保存";
const STORE_REFUSED: &str = "Aura 的记录库这会儿太忙，这一步没有执行";
const STORE_UNKNOWN: &str = "Aura 的记录库没有及时回答，不确定这一步有没有保存";
const STORE_UNAVAILABLE: &str = "Aura 的记录库现在用不了";

/// The text the model is shown for a failed call of `tool`.
pub(super) fn explain(tool: &str, error: &anyhow::Error) -> String {
    if let Some(refusal) = error.downcast_ref::<Refusal>() {
        return clean(&refusal.to_string());
    }
    let text = error.to_string();
    if explained(&text) {
        return clean(&text);
    }
    let (code, doing, after) = context(tool);
    let gap = if doing.ends_with(|ch: char| ch.is_ascii_alphanumeric()) { " " } else { "" };
    format!("{code}: {doing}{gap}没有完成：{}。{after}", cause_of(error))
}

/// The cause of `error` as one bounded line for the model: what a failure of Aura's own store
/// means, in words, or the first line of the error's chain. A Git outcome that is unknown keeps
/// its code in the line, where the execution layer looks for it.
#[must_use]
pub fn cause_of(error: &anyhow::Error) -> String {
    if let Some(store) = store_failure(error) {
        return store.to_owned();
    }
    chain_line(&format!("{error:#}"))
}

/// `text` as one line for the model: its first line, control characters made spaces, cut to
/// [`CAUSE_CHARS`] characters, without a closing full stop.
#[must_use]
pub fn one_line(text: &str) -> String {
    let first = text.trim().lines().next().unwrap_or_default();
    let mut line = String::new();
    for (count, ch) in first.chars().enumerate() {
        if count == CAUSE_CHARS {
            line.push('…');
            break;
        }
        line.push(if ch.is_control() { ' ' } else { ch });
    }
    let line = line.trim_end().trim_end_matches(['。', '.', '；', ';']).trim_end();
    if line.is_empty() { "没有说明原因".to_owned() } else { line.to_owned() }
}

/// What a sub-agent reads in the result of a call its main agent refused with agents.approve:
/// who refused it and why, that it did not run, and what usually helps.
#[must_use]
pub fn main_agent_denial(reason: &str) -> String {
    let reason: String = reason
        .trim()
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .take(REASON_CHARS)
        .collect();
    format!(
        "agents.denied: 主 Agent 拒绝了这个调用，理由：「{reason}」。它没有执行，什么都没改。原样再调用多半还会被拒绝；可以换个做法，或用 agent.report 问主 Agent 该怎么做。"
    )
}

/// What serde found wrong with a call's arguments, in words. Positions are left out: they count
/// in Aura's own copy of the arguments, not in what the model wrote.
pub(super) fn input_problem(error: &serde_json::Error) -> String {
    let text = error.to_string();
    let text = match text.rfind(" at line ") {
        Some(at) if text[at..].contains(" column ") => text[..at].to_owned(),
        _ => text,
    };
    // serde quotes the field or value first, then the ones it expected, each in backticks.
    let names: Vec<&str> = text.split('`').skip(1).step_by(2).collect();
    let listed = |known: &[&str]| {
        if known.is_empty() {
            String::new()
        } else {
            format!("（能用的有：{}）", known.join("、"))
        }
    };
    let problem = if text.starts_with("missing field ")
        && let Some(field) = names.first()
    {
        format!("少了必填的字段 {field}")
    } else if text.starts_with("unknown field ")
        && let Some((field, known)) = names.split_first()
    {
        format!("没有 {field} 这个字段{}", listed(known))
    } else if text.starts_with("unknown variant ")
        && let Some((value, known)) = names.split_first()
    {
        format!("{value} 不是能用的值{}", listed(known))
    } else if text.starts_with("duplicate field ")
        && let Some(field) = names.first()
    {
        format!("字段 {field} 写了两次")
    } else if text.starts_with("invalid type: ") {
        format!("有个字段的类型不对（{}）", one_line(&text))
    } else if text.starts_with("invalid value: ") || text.starts_with("invalid length ") {
        format!("有个字段的值不对（{}）", one_line(&text))
    } else {
        one_line(&text)
    };
    format!("参数不对：{problem}。什么都没做；按工具说明改好这一处再调用。")
}

/// A new sub-agent's conversation that could not be opened. A failure the host already put in
/// the team's words is passed on as it is.
pub(super) fn not_opened(error: anyhow::Error) -> anyhow::Error {
    if error.downcast_ref::<Refusal>().is_some() || explained(&error.to_string()) {
        return error;
    }
    anyhow::anyhow!(
        "agents.start_failed: Aura 没能给子 Agent 建会话：{}。子 Agent 没有开始干活；还需要就再调一次 agents.spawn。",
        cause_of(&error)
    )
}

/// Why sub-agent `member` did not start: its task sheet did not reach it. `stopped` says whether
/// it could then be marked stopped, `worktree` whether it has a worktree left.
pub(super) fn start_failed(
    member: &Member,
    worktree: bool,
    stopped: bool,
    error: &anyhow::Error,
) -> String {
    let after = match (stopped, worktree) {
        (true, true) => {
            "它已标记为停止，没干任何活；它的工作副本还在，不需要就用 agents.discard 删掉，还需要这个任务就重新调 agents.spawn。"
        }
        (true, false) => "它已标记为停止，没干任何活；还需要这个任务就重新调 agents.spawn。",
        (false, _) => {
            "它也没能标记为停止，列表里可能还显示在干活；用 agents.stop 停下它，还需要这个任务就重新调 agents.spawn。"
        }
    };
    format!(
        "agents.start_failed: 子 Agent {}（{}）没能开始：任务单没有送到它那里（{}）。{after}",
        member.handle,
        member.details.name,
        cause_of(error)
    )
}

/// A message for sub-agent `handle` that may not have reached it. One whose conversation or its
/// workspace is gone was refused before anything was kept.
pub(super) fn not_delivered(handle: &str, error: &anyhow::Error) -> String {
    let text = format!("{error:#}");
    let gone = if text.contains("agent.session_missing") {
        Some("会话")
    } else if text.contains("agent.workspace_missing") {
        Some("会话所在的工作区")
    } else {
        None
    };
    match gone {
        Some(gone) => format!(
            "agents.send_failed: 消息没有送到：子 Agent {handle} 的{gone}已经不在了，可能已被删除。什么都没发出；用 agents.status 看现有的子 Agent。"
        ),
        None => format!(
            "agents.send_failed: 给子 Agent {handle} 发消息时出错：{}。消息可能没有送到；用 agents.status 看它现在的状态，再决定要不要重发。",
            cause_of(error)
        ),
    }
}

/// Sub-agent `handle`, marked stopped, whose turn did not stop in time.
pub(super) fn still_ending(handle: &str, error: &anyhow::Error) -> String {
    format!(
        "agents.stop_pending: 子 Agent {handle} 已标记为停止，不会再接新的活，但{}。不用再调 agents.stop；过一会儿用 agents.status 看它是否停下了。",
        not_halted(error)
    )
}

/// A discard that stopped at sub-agent `handle`, marked stopped, whose turn did not stop in time:
/// its worktree was not touched.
pub(super) fn discard_not_halted(handle: &str, error: &anyhow::Error) -> String {
    format!(
        "agents.discard_failed: 子 Agent {handle} 已标记为停止，但{}，所以还没删它的工作副本，什么都没合回。过一会儿再调一次 agents.discard。",
        not_halted(error)
    )
}

/// A worktree that could not be removed for agents.discard; `member` is the sub-agent stopped
/// for it, if any. A failure of the store comes after the folder was removed, when the worktree
/// is marked discarded.
pub(super) fn not_removed(
    member: Option<&Member>,
    worktree: &Worktree,
    error: &anyhow::Error,
) -> String {
    let path = &worktree.details.path;
    let cause = cause_of(error);
    let stopped = member
        .map_or_else(String::new, |member| format!("子 Agent {} 已标记为停止，", member.handle));
    if unknown(error) {
        format!(
            "agents.discard_failed: 删工作副本 {path} 时 Git 的结果没能确认：{cause}。{stopped}什么都没合回，但工作副本可能已经删了一部分；先请用户看看这个文件夹，再决定要不要再调 agents.discard。"
        )
    } else if store_failure(error).is_some() {
        format!(
            "agents.discard_failed: 工作副本 {path} 已经删了，但没能记下来：{cause}。{stopped}什么都没合回；用 agents.status 看它的状态，列表里还显示这个工作副本就再调一次 agents.discard。"
        )
    } else {
        format!(
            "agents.discard_failed: 工作副本 {path} 没能删掉：{cause}。{stopped}什么都没合回，工作副本还留着；如果有程序占着这个文件夹，请用户先关掉，再调一次 agents.discard。"
        )
    }
}

/// A merge that failed: the step it failed in, when the host named it, says whether the
/// workspace could have changed.
pub(super) fn merge_failed(error: &anyhow::Error) -> String {
    let step = error.downcast_ref::<MergeStep>().copied();
    let (doing, cause) = match step {
        // The step is the error's outermost context; its cause is the rest of the chain.
        Some(step) => {
            let rest: Vec<String> = error.chain().skip(1).map(ToString::to_string).collect();
            (step.to_string(), chain_line(&rest.join(": ")))
        }
        None => ("合回工作副本".to_owned(), cause_of(error)),
    };
    let into_workspace = !matches!(step, Some(MergeStep::Locate | MergeStep::Commit));
    match (into_workspace, unknown(error)) {
        (true, true) => format!(
            "agents.merge_failed: {doing}时 Git 的结果没能确认：{cause}。主工作区可能已经改了一部分，工作副本还在；先用 git.status 看看主工作区，再决定怎么做。"
        ),
        (false, true) => format!(
            "agents.merge_failed: {doing}时 Git 的结果没能确认：{cause}。主工作区没有改动，工作副本还在；确认工作副本的状态后再调 agents.merge。"
        ),
        (true, false) => format!(
            "agents.merge_failed: {doing}时出错：{cause}。主工作区应该没有改动（Git 出错时会把它恢复原样），工作副本还在；处理好原因后再调一次 agents.merge，拿不准就先用 git.status 看看主工作区。"
        ),
        (false, false) => format!(
            "agents.merge_failed: {doing}时出错：{cause}。主工作区没有改动，工作副本还在；处理好原因后再调一次 agents.merge。"
        ),
    }
}

/// Why the main agent's answer to sub-agent `handle`'s question did not take effect.
pub(super) fn answer_failed(handle: &str, error: &anyhow::Error) -> String {
    let text = format!("{error:#}");
    if conflict(error) || text.contains("agent.approval_run_missing") {
        format!(
            "agents.approve_failed: {handle} 已经不在等这个审批了：它这一轮可能已经结束或被停下，或者这个请求已经有人回答了。这次的回答没有生效；用 agents.status 看它现在在等什么。"
        )
    } else if text.contains("agent.approval_mode_changed") {
        format!(
            "agents.approve_failed: {handle} 等这个审批时权限模式变了，这个请求已经作废，这次的回答没有生效。不用再回答：它会按新的模式处理这个调用。"
        )
    } else if text.starts_with("agent.stop_pending") {
        format!(
            "agents.approve_failed: {handle} 这一轮的上一步还没收尾，回答没能交上去，这次的回答没有生效。过一会儿用 agents.status 看它是否还在等这个审批，再回答。"
        )
    } else {
        format!(
            "agents.approve_failed: 回答 {handle} 的审批请求没有完成：{}。这个回答可能没有生效；用 agents.status 看它是否还在等这个审批。",
            cause_of(error)
        )
    }
}

/// An answer to sub-agent `handle`'s question that was given but could not be kept on its
/// record. ask_user is only that record: the question then stays the main agent's.
pub(super) fn not_recorded(handle: &str, answer: ApprovalAnswer, error: &anyhow::Error) -> String {
    let cause = cause_of(error);
    match answer {
        ApprovalAnswer::AskUser => format!(
            "agents.approve_failed: 没能把 {handle} 的这个请求交给用户：{cause}。它还在等你回答；可以再调一次 agents.approve。"
        ),
        _ => format!(
            "agents.approve_failed: 回答已经交给 {handle}，但没能记在它的记录上：{cause}。不用再回答；只是多 Agent 页可能看不到这次回答和理由。"
        ),
    }
}

/// What is not known yet of a sub-agent marked stopped whose turn did not stop in time.
fn not_halted(error: &anyhow::Error) -> String {
    if error.to_string().starts_with("agent.stop_pending") {
        "它正在做的那一步还没收尾".to_owned()
    } else {
        format!("没能确认它已经停下（{}）", cause_of(error))
    }
}

/// The code a failure of `tool` no step explained is given, what the call was doing, and what it
/// may have left changed with what usually helps next.
fn context(tool: &str) -> (&'static str, &'static str, &'static str) {
    match tool {
        SPAWN => (
            "agents.failed",
            "派子 Agent",
            "子 Agent 没有开始干活；用 agents.status 看现有的子 Agent，还需要就再调一次 agents.spawn。",
        ),
        STATUS => ("agents.failed", "查看子 Agent", "什么都没改，可以再调一次。"),
        WAIT => (
            "agents.failed",
            "等子 Agent 的报告",
            "子 Agent 不受影响，照常在干活；可以再调一次 agents.wait，或用 agents.status 看它们的状态和最新报告。",
        ),
        SEND => (
            "agents.failed",
            "给子 Agent 发消息",
            "消息没有送到；用 agents.status 看它现在的状态，再决定要不要重发。",
        ),
        RESULT => ("agents.failed", "读子 Agent 的结果", "什么都没改，可以再调一次。"),
        STOP => (
            "agents.failed",
            "停下子 Agent",
            "它可能还没停下；用 agents.status 看它的状态，需要就再调一次 agents.stop。",
        ),
        REPORT => {
            ("agent.report_failed", "交报告", "报告可能没有交上去；可以再调一次 agent.report。")
        }
        MERGE => (
            "agents.failed",
            "合回工作副本",
            "合回没有完成；再调 agents.merge 之前，先用 git.status 看看主工作区有没有改动。",
        ),
        DISCARD => (
            "agents.failed",
            "丢弃工作副本",
            "工作副本可能还在；用 agents.status 看它的状态，需要就再调一次 agents.discard。",
        ),
        APPROVE => (
            "agents.failed",
            "回答子 Agent 的审批请求",
            "这个回答可能没有生效；用 agents.status 看它是否还在等这个审批。",
        ),
        _ => ("agents.failed", "这个调用", "用 agents.status 看子 Agent 现在的状态。"),
    }
}

/// Whether `text` is already one of the team's own messages: a team code, then a sentence.
fn explained(text: &str) -> bool {
    let Some((code, sentence)) = text.split_once(": ") else { return false };
    (code.starts_with("agents.") || code.starts_with("agent.report"))
        && code.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'.'
        })
        && !sentence.trim().is_empty()
}

/// The first line of an error chain's text, or the line from Git's unknown outcome or
/// unconfirmed stop when the first one does not carry its code.
fn chain_line(chain: &str) -> String {
    let line = one_line(chain);
    let kept = [GIT_UNKNOWN, GIT_STOP_UNCONFIRMED];
    match kept.iter().find_map(|code| chain.find(code)) {
        Some(at) if !kept.iter().any(|code| line.contains(code)) => one_line(&chain[at..]),
        _ => line,
    }
}

/// What a failure of Aura's own store means for the step, in words; none for any other error.
fn store_failure(error: &anyhow::Error) -> Option<&'static str> {
    if conflict(error) {
        return Some(STORE_CONFLICT);
    }
    let text = error.to_string();
    match text.split(':').next().unwrap_or_default() {
        "agent.store_refused" => Some(STORE_REFUSED),
        "agent.store_unknown" => Some(STORE_UNKNOWN),
        "agent.store_unavailable" => Some(STORE_UNAVAILABLE),
        _ => None,
    }
}

/// Whether a write of the store found its rows changed by another step at the same moment.
fn conflict(error: &anyhow::Error) -> bool {
    matches!(error.downcast_ref::<PersistenceCoreError>(), Some(PersistenceCoreError::Conflict))
        || format!("{error:#}").contains(PersistenceCoreError::Conflict.to_string().as_str())
}

/// Whether a Git write's end was not seen: what it changed is not known.
fn unknown(error: &anyhow::Error) -> bool {
    let text = format!("{error:#}");
    text.contains(GIT_UNKNOWN) || text.contains(GIT_STOP_UNCONFIRMED)
}

/// `text` without control characters other than line breaks and tabs, which the model's view of a
/// result keeps.
fn clean(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_control() && ch != '\n' && ch != '\t' { ' ' } else { ch })
        .collect()
}
