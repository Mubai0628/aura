//! What a person reads about a tool call that did not succeed: one Chinese sentence that says what
//! happened, what it changed and what comes next. It is chosen by the code the call's error names,
//! or by the code its message begins with when the error's own code says only that the call
//! failed; a family that words its messages for the model in Chinese (`code: 句子`) is read by its
//! own sentence; otherwise the tool's family and the outcome say it. The code and the message
//! written for the model follow it as lines the row shows only while 显示调试信息 is on.
use aura_domain_common::ErrorEnvelope;
use aura_execution_contract::SideEffectStateV1;
use aura_tool_contract::{ToolResultOutcomeV1, ToolResultV1};

/// Detail lines that begin with one of these are for debugging: the renderer shows them only
/// while 设置 → 环境 → 显示调试信息 is on.
pub(crate) const CODE_LINE: &str = "错误码：";
pub(crate) const MODEL_LINE: &str = "给模型的说明：";

/// Codes that say only that a call failed or was refused; the code its message begins with says
/// more.
const GENERAL: &[&str] = &[
    "tool.runtime.dispatch_failed",
    "tool.runtime.unavailable",
    "tool.resource.identity_resolution_failed",
    "tool.resource.scope_resolution_failed",
    "tool.resource.resolver_unavailable",
    "tool.runtime.http_gateway_failed",
];

/// A call whose result Core could not establish; the family's own Chinese sentence, when it has
/// one, says more than this code's general line.
const UNKNOWN_OUTCOME: &str = "tool.runtime.dispatch_outcome_unknown";

/// For a code a person may meet: what happened, and what comes next ("" when nothing does).
const CAUSES: &[(&str, &str, &str)] = &[
    (
        "web.access_denied",
        "网站拒绝访问（HTTP 403）",
        "可用其他公开来源，或请你在已登录的浏览器打开",
    ),
    ("web.authentication_required", "网站需要登录", "可用公开来源或已登录的浏览器"),
    ("web.http_failed", "网站返回 HTTP 错误", "检查网址或按网站的回应决定是否重试"),
    (
        "web.unsupported_media_type",
        "网页工具不支持这类文件",
        "需要文件时可通过下载和相应格式的工具读取",
    ),
    ("web.bot_challenge", "网站返回了访问验证", "可改用公开来源"),
    (
        "tool.result.http_partial_response",
        "服务器只返回了分段或增量内容，文件没有发布",
        "模型可以改用能返回完整文件的地址",
    ),
    ("web.rendering_unavailable", "页面没能渲染", "可尝试读取原始 HTML 或改用浏览器"),
    ("web.deepseek_fetch_empty", "抓取服务未取得正文", "可改用直接 HTTP 或浏览器读取"),
    ("web.deepseek_fetch_refused", "抓取服务拒绝读取", "可尝试其他公开来源"),
    ("web.deepseek_unavailable", "网页抓取服务暂时不可用", "可稍后重试或使用直接 HTTP"),
    ("web.deepseek_rate_limited", "网页抓取服务限流", "按服务提示稍后重试"),
    ("fs.pdf_text_unavailable", "当前无法直接提取 PDF 文本", "可用已配置的 PDF 读取能力"),
    ("fs.document_invalid", "文档格式无效或不完整", "请核对文件或用文档应用打开"),
    ("fs.document_too_large", "文档超过提取上限", "可将所需内容导出成较小的文档或文本"),
    ("tool.approval.expired", "批准请求在你回答前过期了", "模型需要时会再次请求批准"),
    ("tool.approval.denied", "你拒绝了这个调用", ""),
    ("tool.approval.invalidated", "等待批准时会话的权限模式变了", "任务会按当前权限继续"),
    ("tool.hook.blocked", "被你设置的 Hook 拦下了", "模型会收到 Hook 给的原因"),
    ("tool.schema.arguments_invalid", "模型给的参数不完整或格式不对", "模型会收到说明并改正"),
    (
        "tool.schema.arguments_too_large",
        "参数太大（超过 512 KiB）",
        "模型会把内容分成几次较小的调用",
    ),
    (
        "tool.registry.tool_not_found",
        "模型写的工具名不在这一步可用的工具里",
        "模型会收到可用的工具名",
    ),
    ("tool.registry.disabled", "这个工具已停用", "需要时在 设置 → 工具 里启用"),
    ("tool.registry.trust_required", "这个工具的来源还没有被信任", "需要时在设置里信任它"),
    ("tool.registry.audience_denied", "这个对话不能使用这个工具", ""),
    ("tool.registry.version_mismatch", "工具在模型写下调用后更新了", "模型会重新调用"),
    ("tool.batch.rejected", "同一个回答里的调用被一起拒绝了", "模型会收到原因"),
    ("tool.runtime.timeout", "超过了时限", "模型会决定是否重试"),
    ("tool.runtime.process_launch_failed", "程序没能启动", "可以让模型重试，仍不行请重启 Aura"),
    (
        "tool.runtime.dispatch_outcome_unknown",
        "Aura 没能确认这一步有没有开始",
        "请先检查目标，再决定是否让模型重试",
    ),
    (
        "tool.runtime.http_gateway_failed",
        "请求在 Aura 的网络通道里没有完成（网络或代理出了问题）",
        "网络恢复后模型可以重试",
    ),
    ("tool.runtime.http_redirect_denied", "目标把请求转到了不允许的地址", ""),
    ("tool.runtime.io_limit_exceeded", "内容超过了大小上限", "模型可以缩小范围再试"),
    ("tool.runtime.http_control_endpoint_denied", "目标是 Aura 不允许访问的控制地址", ""),
    (
        "tool.runtime.http_credential_reflection_blocked",
        "目标会把凭据原样送回，Aura 为安全拦下了这个请求",
        "",
    ),
    ("tool.runtime.http_request_invalid", "请求的格式不对", "模型会改正后重试"),
    ("tool.network.timed_out", "网络请求超时", "目标可能已经收到了请求，模型会决定是否重试"),
    ("tool.network.connection_refused", "目标拒绝了连接", "模型会检查地址和端口"),
    ("tool.network.host_not_found", "找不到目标主机", "模型会检查网址"),
    ("tool.network.tls_failed", "和目标建立安全连接（TLS）失败", ""),
    ("tool.network.connection_reset", "连接被目标断开", "目标可能已经收到了请求"),
    ("tool.network.connection_failed", "连接不上目标", "网络恢复后模型可以重试"),
    (
        "tool.resource.http_credential_rebind_required",
        "这个工具的凭据要求变了",
        "请到 设置 → 工具 重新绑定凭据",
    ),
    (
        "tool.resource.http_credential_unavailable",
        "这个工具的凭据现在用不了",
        "请到 设置 → 工具 检查凭据",
    ),
    ("tool.resource.http_credential_origin_denied", "凭据不允许用于这个网址", ""),
    (
        "tool.dispatch_refused",
        "Aura 在开始前拦下了这个调用（准备好之后情况有变，或运行环境没有准备好）",
        "模型会收到原因并决定是否重试",
    ),
    ("turn.stopped", "会话中断时这个调用还没开始", ""),
    ("shell.stopped_before_start", "命令还在等待开始时被停止了", ""),
    ("runner.core_binding_mismatch", "命令运行环境和当前的 Aura 不匹配", "重启 Aura 后再试"),
    ("permission.mode_denied", "当前的权限模式不允许这类调用", "需要时切换权限模式后让模型重试"),
    ("permission.approval_required", "这个调用需要你批准，但这里没法询问", ""),
    ("permission.denied_for_turn", "你在这一轮里拒绝了这类调用", ""),
    ("permission.source_disabled", "这个工具的来源已被关闭", "需要时在设置里打开"),
    ("permission.source_untrusted", "这个工具的来源还没有被信任", "需要时在设置里信任它"),
    ("permission.hard_boundary", "它要改动 Aura 从不允许工具改动的地方", ""),
    ("permission.profile_prohibited", "你的设置不允许这类调用", ""),
    ("permission.backend_unavailable", "现在没有能运行它的环境", "稍后可以让模型重试"),
    ("fs.not_found", "找不到这个文件或文件夹（可能已被移动或删除）", "模型会换个路径"),
    ("fs.target_changed", "文件在模型读取之后被改过", "模型会重新读取再改"),
    ("fs.invalid_patch", "补丁和文件现在的内容对不上", "模型会重新读取后再改"),
    ("fs.file_in_use", "另一个程序正占用这个文件", "关闭那个程序后模型可以重试"),
    ("fs.range_too_large", "要读的范围太大", "模型会缩小范围再读"),
    ("fs.cancelled", "调用在完成前被停止了", "需要时模型会重新调用"),
    ("fs.changeset_failed", "改动没能完成", "模型会重新读取文件后再试"),
    (
        "fs.cursor_stale",
        "翻页位置已经失效（文件夹变了，或用了另一次列出的位置）",
        "模型会从头重新列出",
    ),
    (
        "fs.external_file_replaced",
        "工作区外的这个文件在批准之后被换成了别的文件",
        "模型确认路径后可能重新调用，那时会再请你批准",
    ),
    ("fs.internal", "文件工具遇到了 Aura 的内部错误", "可以让模型重试，反复出现时请重启 Aura"),
    ("fs.io_failed", "读写文件时系统报了错", "模型会检查路径后重试"),
    (
        "workspace.offline",
        "工作区处于离线状态，Aura 访问不到它的文件夹",
        "重新打开工作区（或重新连接它所在的磁盘）后可以让模型重试",
    ),
    ("workspace.archived", "工作区已归档，不能使用其中的文件", "需要时先恢复这个工作区"),
    (
        "workspace.invalid_root",
        "工作区文件夹不见了，或已经不是文件夹",
        "检查这个文件夹后重新打开工作区",
    ),
    (
        "workspace.root_identity_changed",
        "工作区打开期间它的文件夹被替换或移动了",
        "重新打开工作区后可以让模型重试",
    ),
    ("workspace.busy", "工作区正忙", "稍后可以让模型重试"),
    ("workspace.lifecycle_in_progress", "工作区正在打开、关闭或移动", "稍后可以让模型重试"),
    (
        "workspace.editor_reserved",
        "这个文件正在另一段对话的 Aura 编辑器里被编辑",
        "那边编辑完后可以让模型重试",
    ),
    ("workspace.catalog_unavailable", "工作区的文件索引还没准备好", "稍后可以让模型重试"),
    (
        "tool.batch.control_state_mismatch",
        "它要处理的内容在调用期间变了或已经结束",
        "模型会重新读取当前状态再决定",
    ),
    ("task_lineage.disposition_not_allowed", "当前的任务状态不允许这一步", "模型会收到说明并调整"),
    ("tool.registry.snapshot_unavailable", "这次没能读到可用的工具列表", "稍后可以让模型重试"),
    (
        "tool.registry.snapshot_corrupted",
        "Aura 读不了自己的工具列表（Aura 的内部错误）",
        "可以让模型再试一次，仍不行请重启 Aura",
    ),
    ("memory.hash_conflict", "这条记忆在模型读取之后被改过", "模型会重新读取再写"),
    ("memory.secret_refused", "记忆内容里像是有密钥，为安全没有写入", "模型会去掉密钥再写"),
    ("memory.not_found", "找不到这条记忆", ""),
    ("chrome.background_tab", "标签页在后台", "把它切到前台后可以让模型重试"),
    ("chrome.timed_out", "Chrome 没有及时回应", "模型可以重试"),
    ("chrome.cancelled", "这一步被取消了", ""),
    ("chrome.stopped", "这一步被取消了", ""),
    ("chrome.disconnected", "Chrome 和 Aura 断开了", "重新连上后模型可以重试"),
    ("computer.timeout", "程序没有及时回应", "模型可以重试"),
    ("computer.cancelled", "这一步被取消了", ""),
    ("git.repository_changed", "仓库在批准后有了变化", "模型会重新查看状态再试"),
    ("mcp.tool_error", "MCP 工具返回了错误", "模型会收到它的说明"),
    (
        "fs.deferred_by_editor_activity",
        "你正在 Aura 编辑器里改这个文件",
        "保存或关闭后模型会重新读取再改",
    ),
    ("chrome.not_drawn", "标签页在后台，Chrome 没有画出它", "把它切到前台后可以让模型重试"),
    ("computer.taken_over", "你接管了电脑", ""),
    ("computer.busy", "另一个对话正在使用电脑", "等它用完后可以让模型重试"),
    ("computer.stopped", "电脑操控被停止了", ""),
    ("browser.debugging_off", "“允许模型调试网页”没有打开", "需要时用这一行的按钮打开"),
    ("browser.disabled", "内置浏览器已在设置里关闭", "需要时在 设置 → 网络与浏览器 里打开"),
    ("browser.unavailable", "浏览器设置还没准备好", "稍后可以让模型重试"),
    ("computer.off", "电脑操控没有打开", "需要时在 设置 → 电脑操控 里打开"),
    ("computer.unavailable", "电脑操控现在用不了", ""),
    ("http.credential_rebind_required", "这个工具的凭据要求变了", "请到 设置 → 工具 重新绑定凭据"),
    ("package.timed_out", "程序超过了时限，被停止", "模型会决定是否重试"),
    ("agents.input_invalid", "子 Agent 调用的参数不完整或格式不对", "模型会收到说明并改正"),
    ("browser.input_invalid", "模型给的参数不完整或格式不对", "模型会收到说明并改正"),
    ("chrome.input_invalid", "模型给的参数不完整或格式不对", "模型会收到说明并改正"),
    ("computer.input_invalid", "模型给的参数不完整或格式不对", "模型会收到说明并改正"),
    ("media.input_invalid", "模型给的参数不完整或格式不对", "模型会收到说明并改正"),
    ("media.source_unavailable", "打不开要读的视频文件", "模型会收到原因"),
    ("agents.denied", "主 Agent 拒绝了这个调用", "模型会收到主 Agent 给的理由"),
];

/// What a call of a tool family is, by the tool's name.
const FAMILIES: &[(&str, &str)] = &[
    ("fs.", "文件操作"),
    ("memory.", "记忆操作"),
    ("shell.", "命令"),
    ("terminal.", "终端操作"),
    ("git.", "Git 操作"),
    ("browser.", "网页操作"),
    ("chrome.", "Chrome 里的操作"),
    ("computer.", "电脑操作"),
    ("mcp.", "MCP 工具"),
    ("package.", "工具包里的程序"),
    ("agents.", "子 Agent 操作"),
    ("agent.", "子 Agent 操作"),
    ("web.", "网页请求"),
    ("http.", "网络请求"),
    ("media.", "媒体操作"),
    ("skills.", "Skill 操作"),
    ("query.", "提问"),
];

/// The sentence a person reads first about `result`, a call of tool `name` that did not succeed;
/// none for a call that succeeded or carries no error.
pub(crate) fn line(name: &str, result: &ToolResultV1) -> Option<String> {
    let error = error_of(&result.outcome)?;
    let effect = effect(result);
    let found = cause(error);
    // The general line for an unknown outcome cannot say how far the call got; a family's own
    // sentence can (sent to the MCP server, which never answered).
    if (found.is_none() || error.code.as_str() == UNKNOWN_OUTCOME)
        && let Some(sentence) = worded(&error.message)
    {
        return Some(sentence);
    }
    Some(match found {
        Some((cause, "")) => format!("{cause}，{effect}。"),
        Some((cause, next)) => format!("{cause}，{effect}；{next}。"),
        None => {
            let what = family(name);
            match &result.outcome {
                ToolResultOutcomeV1::Indeterminate { .. } => {
                    format!("{what}的结果未知：{effect}；请先检查目标，再决定是否让模型重试。")
                }
                ToolResultOutcomeV1::Failed { .. } => {
                    format!("{what}没有成功，{effect}；模型会收到原因并决定下一步。")
                }
                _ => format!("{what}{effect}；模型会收到原因并决定下一步。"),
            }
        }
    })
}

fn error_of(outcome: &ToolResultOutcomeV1) -> Option<&ErrorEnvelope> {
    match outcome {
        ToolResultOutcomeV1::Failed { error }
        | ToolResultOutcomeV1::OperationRejected { error }
        | ToolResultOutcomeV1::Indeterminate { error, .. } => Some(error),
        ToolResultOutcomeV1::NotExecuted { error, .. } => error.as_ref(),
        ToolResultOutcomeV1::Succeeded | ToolResultOutcomeV1::Cancelled { .. } => None,
    }
}

/// What `code` says happened, and what comes next.
fn known(code: &str) -> Option<(&'static str, &'static str)> {
    CAUSES.iter().find(|(known, ..)| *known == code).map(|(_, cause, next)| (*cause, *next))
}

/// What the error's own code says happened, when it says more than that the call failed.
pub(super) fn cause_of_code(error: &ErrorEnvelope) -> Option<&'static str> {
    let code = error.code.as_str();
    (!GENERAL.contains(&code)).then(|| known(code)).flatten().map(|(cause, _)| cause)
}

/// What the error's code says happened, or the code its message begins with when the error's own
/// code says less.
pub(super) fn cause(error: &ErrorEnvelope) -> Option<(&'static str, &'static str)> {
    let code = error.code.as_str();
    let named = error.message.split(':').next().map(str::trim).unwrap_or_default();
    match known(code) {
        Some(found) if !GENERAL.contains(&code) => Some(found),
        found => known(named).or(found),
    }
}

/// The most characters of a family's own sentence shown as the first line.
const WORDED_CHARS: usize = 300;

/// The sentence of a message a family words in Chinese for the model, as `code: 句子`: a person
/// reads it as well, and it says more than the family's general line. None for an English
/// message or one that is only a code.
fn worded(message: &str) -> Option<String> {
    let first = message.trim().lines().next()?.trim();
    let sentence = match first.split_once(':') {
        Some((code, rest)) if is_code(code) => rest.trim(),
        _ => first,
    };
    let chinese = sentence.chars().any(|ch| (0x4e00..=0x9fff).contains(&u32::from(ch)));
    if !chinese || sentence.chars().count() < 8 {
        return None;
    }
    let mut shown: String = sentence.chars().take(WORDED_CHARS).collect();
    if sentence.chars().count() > WORDED_CHARS {
        shown.push('…');
    }
    Some(shown)
}

fn is_code(token: &str) -> bool {
    let token = token.trim();
    token.contains('.')
        && token.starts_with(|ch: char| ch.is_ascii_lowercase())
        && token.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'.'
        })
}

/// What the call changed, by its outcome and side-effect state.
pub(super) fn effect(result: &ToolResultV1) -> &'static str {
    if matches!(result.outcome, ToolResultOutcomeV1::NotExecuted { .. }) {
        return "没有执行，什么都没改";
    }
    match result.side_effect_state {
        SideEffectStateV1::NotStarted => "没有执行，什么都没改",
        SideEffectStateV1::NoEffect => "没有改动",
        SideEffectStateV1::PartiallyApplied => "可能只做了一部分",
        SideEffectStateV1::FullyApplied => "改动已经做了",
        SideEffectStateV1::Compensated => "已做的改动撤回了",
        SideEffectStateV1::Unknown | SideEffectStateV1::Unassessed => {
            "是否生效没能确认，Aura 不会自动重做"
        }
    }
}

fn family(name: &str) -> &'static str {
    FAMILIES
        .iter()
        .find(|(prefix, _)| name.starts_with(prefix))
        .map_or("这个调用", |(_, what)| *what)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::{CorrelationId, ErrorCategory, ErrorCode, UtcTimestamp};

    fn result(outcome: ToolResultOutcomeV1, state: SideEffectStateV1) -> ToolResultV1 {
        ToolResultV1 {
            tool_call_id: aura_domain_common::ToolCallId::new(),
            outcome,
            side_effect_state: state,
            attempts: vec![],
            model_visible_result_ref: None,
            user_audit_result_ref: None,
            internal_diagnostic_ref: None,
            aggregate_side_effect_evidence_ref: None,
            started_at: None,
            completed_at: UtcTimestamp::now(),
        }
    }

    fn error(code: &str, message: &str) -> ErrorEnvelope {
        ErrorEnvelope::new(
            ErrorCode::new(code).expect("code"),
            ErrorCategory::Unavailable,
            message,
            false,
            CorrelationId::new(),
            None,
        )
    }

    #[test]
    fn a_known_code_reads_as_what_happened_what_changed_and_what_comes_next() {
        let rejected = result(
            ToolResultOutcomeV1::OperationRejected {
                error: error("fs.not_found", "The requested file does not exist."),
            },
            SideEffectStateV1::NotStarted,
        );
        assert_eq!(
            line("fs.read", &rejected).as_deref(),
            Some(
                "找不到这个文件或文件夹（可能已被移动或删除），没有执行，什么都没改；模型会换个路径。"
            )
        );
    }

    /// Every failure outside the web tools is stored under one general code; the code its
    /// message begins with names the cause.
    #[test]
    fn a_general_code_gives_way_to_the_code_its_message_names() {
        let failed = result(
            ToolResultOutcomeV1::Failed {
                error: error("tool.runtime.dispatch_failed", "package.timed_out: 程序超时，已停止"),
            },
            SideEffectStateV1::PartiallyApplied,
        );
        assert_eq!(
            line("package.run", &failed).as_deref(),
            Some("程序超过了时限，被停止，可能只做了一部分；模型会决定是否重试。")
        );
    }

    #[test]
    fn an_unknown_code_is_said_by_the_tools_family_and_the_outcome() {
        let unknown = result(
            ToolResultOutcomeV1::Failed { error: error("tool.runtime.dispatch_failed", "x") },
            SideEffectStateV1::Unknown,
        );
        let said = line("git.push", &unknown).expect("line");
        assert!(said.starts_with("Git 操作没有成功，是否生效没能确认"), "{said}");
        assert!(!said.contains("tool.") && !said.contains("x"), "{said}");
    }

    /// The team, the browser, Git and the other families that word their messages in Chinese
    /// say more than the family's general line, and the person reads that sentence without its
    /// code; an English message keeps the general line.
    #[test]
    fn a_chinese_sentence_of_the_family_is_read_without_its_code() {
        let failed = result(
            ToolResultOutcomeV1::Failed {
                error: error(
                    "tool.runtime.dispatch_failed",
                    "agents.discard_failed: 工作副本 D:/w/a1 没能删掉：文件被占用。子 Agent A1 已标记为停止，什么都没合回。",
                ),
            },
            SideEffectStateV1::NoEffect,
        );
        assert_eq!(
            line("agents.discard", &failed).as_deref(),
            Some("工作副本 D:/w/a1 没能删掉：文件被占用。子 Agent A1 已标记为停止，什么都没合回。")
        );
        let english = result(
            ToolResultOutcomeV1::Failed {
                error: error("tool.runtime.dispatch_failed", "memory.failed: the store is busy"),
            },
            SideEffectStateV1::NoEffect,
        );
        let said = line("memory.write", &english).expect("line");
        assert!(said.starts_with("记忆操作没有成功，没有改动"), "{said}");
    }

    /// The file tools' and the task-state tools' refusals are worded in English for the model;
    /// a person reads what happened in Chinese, what was changed and what comes next.
    #[test]
    fn a_file_or_task_state_refusal_reads_in_chinese() {
        for (code, name) in [
            ("fs.cancelled", "fs.read"),
            ("fs.changeset_failed", "fs.patch"),
            ("fs.cursor_stale", "fs.list"),
            ("fs.external_file_replaced", "fs.write"),
            ("fs.internal", "fs.read"),
            ("fs.io_failed", "fs.read"),
            ("workspace.offline", "fs.read"),
            ("workspace.archived", "fs.read"),
            ("workspace.invalid_root", "fs.read"),
            ("workspace.root_identity_changed", "fs.read"),
            ("workspace.busy", "fs.write"),
            ("workspace.lifecycle_in_progress", "fs.write"),
            ("workspace.editor_reserved", "fs.write"),
            ("workspace.catalog_unavailable", "fs.search"),
            ("tool.batch.control_state_mismatch", "todo.update"),
            ("task_lineage.disposition_not_allowed", "mode.disposition"),
            ("tool.registry.snapshot_unavailable", "tools.search"),
            ("tool.registry.snapshot_corrupted", "tools.search"),
        ] {
            let rejected = result(
                ToolResultOutcomeV1::OperationRejected {
                    error: error(code, "The refusal in the model's words."),
                },
                SideEffectStateV1::NotStarted,
            );
            let said = line(name, &rejected).expect("line");
            assert!(said.contains("没有执行，什么都没改"), "{code}: {said}");
            assert!(
                !said.contains("model") && !said.contains("模型会收到原因并决定下一步"),
                "{code} has its own words: {said}"
            );
        }
    }

    /// A call recorded Unknown reads its family's sentence, which says how far it got; an
    /// English message keeps the general line.
    #[test]
    fn an_unknown_outcome_reads_how_far_its_call_got() {
        let unknown = |message: &str| {
            result(
                ToolResultOutcomeV1::Indeterminate {
                    reason: aura_tool_contract::ToolIndeterminateReasonV1::DispatchOutcomeUnknown,
                    error: error("tool.runtime.dispatch_outcome_unknown", message),
                },
                SideEffectStateV1::Unknown,
            )
        };
        let sent = unknown(
            "mcp.outcome_unknown: 调用已经发给 MCP 服务器，之后服务器没有在 5 秒内回应，不知道它有没有做完。",
        );
        assert_eq!(
            line("mcp.files.slow", &sent).as_deref(),
            Some("调用已经发给 MCP 服务器，之后服务器没有在 5 秒内回应，不知道它有没有做完。")
        );
        let said = line("shell.powershell", &unknown("shell.lost: the runner stopped answering"))
            .expect("line");
        assert!(said.starts_with("Aura 没能确认这一步有没有开始"), "{said}");
    }

    #[test]
    fn a_call_without_an_error_has_no_line() {
        let succeeded = result(ToolResultOutcomeV1::Succeeded, SideEffectStateV1::FullyApplied);
        assert_eq!(line("fs.write", &succeeded), None);
    }
}
