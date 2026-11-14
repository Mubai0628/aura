//! The failures Core's request routes answer the desktop with, each `code: 中文句`: the window
//! shows the sentence after the code, so it says what did not happen and what to do next.

/// A change that came without its command invocation is not run.
pub const NO_INVOCATION: &str =
    "ipc.command_invocation_id_required: 这次修改缺少请求编号，没有执行；请重试。";
pub const RECOVERY_REQUIRED: &str = "core.recovery_required: Aura 的数据需要恢复（已进入诊断模式），这次操作没有执行；现有内容只能查看，原因记在本机日志里。";
pub const ROUTE_UNHANDLED: &str = "core.route_unhandled: Aura Core 处理不了这个请求（Aura 内部出错），没有执行；请重启 Aura 后重试。";
pub const SESSION_UNKNOWN: &str =
    "agent.session_unknown: 找不到这个会话，它可能已被删除，这次没有执行；请刷新会话列表。";
pub const SESSION_REQUIRED: &str =
    "agent.session_required: 这个请求没有指明是哪个会话，没有执行；请刷新后重试。";
pub const SETTINGS_INTERRUPTED: &str = "settings.interrupted: 设置的这次操作在后台意外中断，可能没有完成；请刷新查看后再决定是否重试。";
pub const ACTIVITY_QUERY_UNAVAILABLE: &str =
    "activity.query_unavailable: 后台同时读取的任务太多，活动记录这次没有读到；请稍后刷新。";
pub const ACTIVITY_HINTS_OVER_LIMIT: &str = "activity.request_hints_over_limit: 这次活动记录查询附带的相关文件太多，没有执行；请刷新后再查。";
pub const ACTIVITY_HINT_OVER_LIMIT: &str = "activity.request_hint_over_limit: 这次活动记录查询附带的一个相关文件超出了允许的长度或格式，没有执行；请刷新后再查。";
pub const CANCEL_INVALID: &str =
    "cancel.params_invalid: 停止请求的内容不完整，没有执行；请刷新后再点一次停止。";
pub const CANCEL_LEDGER_UNAVAILABLE: &str = "cancel.ledger_unavailable: Core 的请求记录在之前的一次操作中意外中断，停止没有执行；请重启 Aura。";
pub const CANCEL_UNCLASSIFIED: &str = "cancel.unclassified: Core 没能判断这次停止针对的是哪项操作，没有执行；请刷新后再点一次停止，仍然不行请重启 Aura。";
pub const MEDIA_UNKNOWN_METHOD: &str = "media.unknown_host_method: Aura Core 处理不了这个媒体请求（Aura 内部出错），没有执行；请重启 Aura 后重试。";
pub const MEMORY_WORKSPACE_UNAVAILABLE: &str =
    "memory.workspace_unavailable: 工作区还没有准备好，记忆库这次没有打开；请稍后重试。";
pub const MEMORY_RESPONSE_TOO_LARGE: &str =
    "memory.response_too_large: 记忆库这次的结果超过 2 MB，没有返回；请先删掉不用的记忆再试。";
pub const TOOL_PRINCIPAL_UNAVAILABLE: &str = "tools.principal_unavailable: 读不到当前的工具设置身份，凭据列表这次没有读到；请稍后重试，仍然不行请重启 Aura。";
