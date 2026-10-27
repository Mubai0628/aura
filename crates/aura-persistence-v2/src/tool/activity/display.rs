//! Small user-facing facts; model-visible envelopes and audit records stay immutable.
use crate::tool::execution::{BatchExecution, CallExecution};
use aura_domain_common::ErrorCategory;
use aura_execution_contract::SideEffectStateV1;
use aura_persistence_core::{PersistenceCoreError as Error, StoreResult, artifact};
use aura_tool_contract::*;
use aura_tool_projection::{AgentToolDisplay, AgentToolOffer};
use rusqlite::Connection;
use serde_json::Value;

fn clipped(text: &str, limit: usize) -> String {
    let mut chars = text.chars();
    let mut result: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}

/// The short line one terminal outcome shows.
///
/// It is a sentence a person reads in a row, so it names the reason and nothing else;
/// the stored explanation is put in `details` by [`error_detail`]. Nothing here reads a
/// message, and nothing here uses the broad outcome alone: `operation_rejected` covers
/// a missing path, a policy refusal and a state conflict, and calling all three "已拒绝"
/// is what made four missing directories look like a permission problem.
fn outcome_summary_text(outcome: &ToolResultOutcomeV1) -> String {
    match outcome {
        ToolResultOutcomeV1::Succeeded => "执行成功".into(),
        ToolResultOutcomeV1::OperationRejected { error } => match error.category {
            ErrorCategory::NotFound => "未执行：目标不存在".into(),
            ErrorCategory::Permission => "未执行：权限拒绝".into(),
            ErrorCategory::Conflict => "未执行：状态已变化".into(),
            ErrorCategory::Validation => "未执行：请求无效".into(),
            ErrorCategory::Unavailable | ErrorCategory::Compatibility => {
                "未执行：当前不可用".into()
            }
            _ => "未执行：操作被拒绝".into(),
        },
        ToolResultOutcomeV1::Failed { error } => match error.category {
            ErrorCategory::Unavailable => "执行失败：当前不可用".into(),
            _ => "执行失败".into(),
        },
        ToolResultOutcomeV1::NotExecuted { error: Some(error), .. }
            if error.code == ToolHookErrorCode::Blocked.error_code() =>
        {
            "未执行：被 Hook 阻止".into()
        }
        ToolResultOutcomeV1::NotExecuted { reason, .. } => match reason {
            ToolNotExecutedReasonV1::PurposeRequired => "未执行：缺少调用说明".into(),
            ToolNotExecutedReasonV1::SchemaValidationFailed => "未执行：参数错误".into(),
            ToolNotExecutedReasonV1::ResourceClaimResolutionFailed => {
                "未执行：无法确定操作目标".into()
            }
            ToolNotExecutedReasonV1::DependencyNotExecuted => "未执行：依赖的调用未执行".into(),
            ToolNotExecutedReasonV1::AtomicGroupNotExecuted => "未执行：同组调用未执行".into(),
            _ => "未执行".into(),
        },
        ToolResultOutcomeV1::Indeterminate { .. } => "结果未知".into(),
        ToolResultOutcomeV1::Cancelled { .. } => "执行已取消".into(),
    }
}

/// The bounded diagnostic lines a reader may expand for a terminal result.
///
/// The short line in `summary` is what the row shows; this is where the stored
/// explanation goes, so a truncated template is no longer the only thing the session
/// has. Each line is bounded and there are at most three, because a Tool error is
/// consumed by a person reading a row, not by a log viewer.
fn error_detail(result: &ToolResultV1) -> Vec<String> {
    let error = match &result.outcome {
        ToolResultOutcomeV1::Failed { error }
        | ToolResultOutcomeV1::OperationRejected { error }
        | ToolResultOutcomeV1::Indeterminate { error, .. } => Some(error),
        ToolResultOutcomeV1::NotExecuted { error, .. } => error.as_ref(),
        ToolResultOutcomeV1::Succeeded | ToolResultOutcomeV1::Cancelled { .. } => None,
    };
    let Some(error) = error else {
        return vec![];
    };
    let mut details = vec![format!("错误码：{}", error.code)];
    let message = error.message.trim();
    if !message.is_empty() {
        details.push(clipped(message, 400));
    }
    if let Some(fields) = error.details.get("fields").and_then(Value::as_array) {
        let names = fields
            .iter()
            .filter_map(Value::as_str)
            .take(8)
            .map(|name| clipped(name, 64))
            .collect::<Vec<_>>();
        if !names.is_empty() {
            details.push(format!("相关字段：{}", names.join(", ")));
        }
    }
    details
}

/// A page-debugging call stopped because 允许模型调试网页 is off offers the switch; the
/// decision stays the user's, on the row, never the model's.
fn debugging_offer(name: &str, result: &ToolResultV1) -> Option<AgentToolOffer> {
    let error = match &result.outcome {
        ToolResultOutcomeV1::Failed { error }
        | ToolResultOutcomeV1::OperationRejected { error }
        | ToolResultOutcomeV1::Indeterminate { error, .. } => Some(error),
        ToolResultOutcomeV1::NotExecuted { error, .. } => error.as_ref(),
        ToolResultOutcomeV1::Succeeded | ToolResultOutcomeV1::Cancelled { .. } => None,
    }?;
    (name.starts_with("browser.") && error.message.contains("browser.debugging_off"))
        .then_some(AgentToolOffer::EnableBrowserDebugging)
}

/// The directory or file a call names, as the model spelled it. Search and find work in the
/// workspace root when the model names no directory, so that is what they show. A Shell call
/// names its command.
fn target(name: &str, arguments: Option<&Value>) -> Option<String> {
    if super::shell::NAMES.contains(&name) {
        return super::shell::target(arguments);
    }
    if name.starts_with("browser.") {
        return super::browser::target(name, arguments);
    }
    if name.starts_with("chrome.") {
        return super::chrome::target(name, arguments);
    }
    let arguments = arguments?;
    let named = arguments
        .get("path")
        .or_else(|| arguments.get("root"))
        .or_else(|| arguments.get("working_directory"));
    match named.and_then(Value::as_str) {
        Some(value) => Some(clipped(value, 256)),
        None if named.is_none() && matches!(name, "fs.find" | "fs.search") => {
            Some(aura_domain_common::WorkspacePath::ROOT.to_owned())
        }
        None => None,
    }
}

/// What a call's own input adds to its expanded row, before any result.
fn input_details(name: &str, arguments: Option<&Value>) -> Vec<String> {
    if name == "terminal.request_sensitive_input" {
        vec!["已请用户输入（内容不记录）".into()]
    } else if super::shell::NAMES.contains(&name) {
        super::shell::details(arguments)
    } else if name == super::query::NAME {
        super::query::details(arguments)
    } else if name.starts_with("browser.") {
        super::browser::details(name, arguments)
    } else if name.starts_with("chrome.") {
        super::chrome::details(name, arguments)
    } else if name == "terminal.write" {
        arguments
            .and_then(|value| value["input"].as_str())
            .filter(|value| !value.is_empty())
            .map(|value| vec![format!("输入：{}", clipped(value, 400))])
            .unwrap_or_default()
    } else {
        vec![]
    }
}

pub(super) fn project(
    db: &Connection,
    call: &CallExecution,
    batch: &BatchExecution,
    name: &str,
) -> StoreResult<AgentToolDisplay> {
    let mut view = project_call(db, call, batch, name)?;
    // A question's record says how it ended, however long after the call that was.
    if name == super::query::NAME && call.result.is_some() {
        super::query::outcome(
            db,
            batch.batch.invocation_identity.turn_task_id,
            call.call.tool_call_id,
            &mut view,
        )?;
    }
    Ok(view)
}

fn project_call(
    db: &Connection,
    call: &CallExecution,
    batch: &BatchExecution,
    name: &str,
) -> StoreResult<AgentToolDisplay> {
    let arguments = match &call.call.input {
        NormalizedToolCallInputStateV1::Ready { arguments, .. } => Some(arguments),
        _ => None,
    };
    let target = if name == super::workspace_init::NAME {
        super::workspace_init::target(arguments)
    } else {
        target(name, arguments)
    };
    let mut view = AgentToolDisplay {
        title: title(name).into(),
        target,
        summary: String::new(),
        details: input_details(name, arguments),
        truncated: false,
        image: None,
        offer: None,
        internal: name.starts_with("tools.")
            || name.starts_with("task_lineage.")
            || matches!(name, "normal.complete" | "plan.complete" | "goal.check" | "goal.complete"),
    };
    let Some(result) = &call.result else {
        return Ok(view);
    };
    if result.side_effect_state == SideEffectStateV1::Unknown {
        // Unknown is a claim about what happened, not a kind of failure. It is decided
        // before any error text is read so a row can never present an unconfirmed side
        // effect as an ordinary refusal.
        view.summary = "操作已返回，实际影响尚未确认".into();
        view.details.extend(error_detail(result));
        super::network_route::append_proxy_detail(db, call, name, &mut view);
        return Ok(view);
    }
    // The short line states the reason, and the long diagnostic moves to the details a
    // reader can expand. The previous projection cut the English correction at 120
    // characters, so the row showed the beginning of a template and the actual field
    // it named fell off the end.
    view.summary = outcome_summary_text(&result.outcome);
    view.details.extend(error_detail(result));
    view.offer = debugging_offer(name, result);
    if view.internal || !matches!(result.outcome, ToolResultOutcomeV1::Succeeded) {
        super::network_route::append_proxy_detail(db, call, name, &mut view);
        return Ok(view);
    }
    let Some(id) = result.model_visible_result_ref else {
        super::network_route::append_proxy_detail(db, call, name, &mut view);
        return Ok(view);
    };
    let size: i64 = db.query_row("SELECT o.byte_length FROM artifacts a JOIN artifact_objects o ON o.hash=a.object_hash WHERE a.id=?1",[id.to_string()],|r|r.get(0))?;
    if size > 2_097_152 {
        view.truncated = true;
        super::network_route::append_proxy_detail(db, call, name, &mut view);
        return Ok(view);
    }
    let bytes = artifact::read(db, id, &batch.batch.invocation_identity.agent_run_id.to_string())?;
    let envelope: ModelVisibleToolResultEnvelopeV1 = serde_json::from_slice(&bytes)?;
    if envelope.tool_call_id != call.call.tool_call_id
        || envelope.definition_ref != call.call.definition_ref
    {
        return Err(Error::Conflict);
    }
    if let ModelVisibleToolResultOutcomeV1::Succeeded {
        payload: ModelVisibleToolSuccessPayloadV1::StructuredJson { value },
    } = envelope.outcome
    {
        summarize(&mut view, name, &value);
    }
    super::network_route::append_proxy_detail(db, call, name, &mut view);
    Ok(view)
}

fn summarize(view: &mut AgentToolDisplay, name: &str, value: &Value) {
    match name {
        "git.commit" | "git.branch" | "git.sync" | "git.push" => {
            super::git::summarize(view, name, value)
        }
        super::workspace_init::NAME => super::workspace_init::summarize(view, value),
        name if name.starts_with("browser.") => super::browser::summarize(view, name, value),
        name if name.starts_with("chrome.") => super::chrome::summarize(view, name, value),
        "terminal.request_sensitive_input" => {
            view.summary = match value["status"].as_str() {
                Some("completed") => "安全输入已完成",
                Some("cancelled") => "安全输入已取消",
                _ => "安全输入失败",
            }
            .into();
        }
        name if name.starts_with("terminal.") => {
            view.summary =
                if value["running"] == true { "终端运行中" } else { "终端已结束" }.into();
            if let Some(output) = value["output"].as_str().filter(|value| !value.is_empty()) {
                view.details.push(clipped(output, 400));
                view.truncated = output.chars().count() > 400 || value["truncated"] == true;
            }
        }
        // Where a deletion went.
        "fs.delete" => {
            view.summary = match value["outcome"].as_str() {
                Some("recycled") => "已移到回收站",
                Some("deleted") => "已永久删除",
                _ => "已删除",
            }
            .into();
        }
        "fs.read" => {
            view.summary = value["total_lines"]
                .as_u64()
                .map_or_else(|| "已读取文件".into(), |n| format!("文件共 {n} 行"));
        }
        "fs.search" => {
            if let Some(matches) = value["matches"].as_array() {
                view.truncated = value["completion"]["status"] == "partial";
                view.summary = format!(
                    "找到 {} 处匹配{}",
                    matches.len(),
                    if view.truncated { " · 结果未完整" } else { "" }
                );
                view.details = matches
                    .iter()
                    .take(5)
                    .map(|m| {
                        format!(
                            "{}:{} · {}",
                            clipped(m["path"].as_str().unwrap_or(""), 120),
                            m["line_index"].as_u64().unwrap_or(0) + 1,
                            clipped(m["excerpt"].as_str().unwrap_or(""), 120)
                        )
                    })
                    .collect();
                if matches.len() > 5 {
                    view.summary.push_str(" · 展示前 5 处");
                    view.truncated = true;
                }
            }
        }
        "fs.list" => {
            if let Some(entries) = value["entries"].as_array() {
                let directories = entries.iter().filter(|e| e["kind"] == "directory").count();
                view.truncated = value["truncated"].as_bool().unwrap_or(false);
                view.summary = format!(
                    "{} 个文件，{} 个目录{}",
                    entries.len() - directories,
                    directories,
                    if view.truncated { " · 还有下一页" } else { "" }
                );
                view.details = entries
                    .iter()
                    .take(10)
                    .filter_map(|e| e["name"].as_str())
                    .map(|n| clipped(n, 160))
                    .collect();
                if entries.len() > 10 {
                    view.summary.push_str(" · 展示前 10 项");
                    view.truncated = true;
                }
            }
        }
        "fs.find" => {
            // Filename lookup reports what it found and whether the answer is
            // whole: a partial page means "not seen yet", never "does not exist".
            if let Some(matches) = value["matches"].as_array() {
                let complete = value["complete"].as_bool().unwrap_or(true);
                view.truncated = !complete;
                view.summary = format!(
                    "找到 {} 个文件{}",
                    matches.len(),
                    if complete { "" } else { " · 结果未完整" }
                );
                view.details = matches
                    .iter()
                    .take(10)
                    .filter_map(|m| m["path"].as_str())
                    .map(|path| clipped(path, 160))
                    .collect();
                if matches.len() > 10 {
                    view.summary.push_str(" · 展示前 10 项");
                    view.truncated = true;
                }
            }
        }
        "shell.powershell" | "shell.cmd" => {
            // A command Aura stopped has a result, but it did not exit by itself.
            let stopped = match value["stop_reason"].as_str() {
                Some("output_limit") => Some("已停止：输出超过上限"),
                Some("timed_out") => Some("已停止：超过时限"),
                Some("core_disconnected") => Some("已停止：Core 连接中断"),
                _ => None,
            };
            if let Some(stopped) = stopped {
                view.summary = stopped.into();
            } else if let Some(code) = value.get("exit_code").and_then(Value::as_i64) {
                view.summary = format!("进程退出，退出码 {code}");
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;

/// The conversation's name for a tool, also for the tool a running step is writing.
pub(crate) fn title(name: &str) -> &str {
    match name {
        "fs.list" => "列出目录",
        "fs.read" => "读取文件",
        "fs.search" => "搜索文本",
        "fs.find" => "查找文件",
        "fs.write" => "写入文件",
        "fs.patch" => "修改文件",
        "fs.delete" => "删除文件",
        super::workspace_init::NAME => "初始化工作区",
        "commands.list" => "列出命令",
        "commands.describe" => "查看命令",
        "commands.save" => "保存命令",
        "commands.delete" => "删除命令",
        "shell.powershell" | "shell.cmd" => "运行命令",
        name if name.starts_with("terminal.") => "终端",
        "web.search" => "搜索网页",
        "web.fetch" | "web.read" => "读取网页",
        super::query::NAME => "向你提问",
        // Shown while the model writes the call; once admitted it is the tool it names.
        "tools.call" => "调用工具",
        name if name.starts_with("browser.") => super::browser::title(name).unwrap_or(name),
        name if name.starts_with("chrome.") => super::chrome::title(name).unwrap_or(name),
        _ => name,
    }
}
