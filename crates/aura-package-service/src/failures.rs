//! The failures the package service answers with, each `code: 中文句`: the 工具包 page shows the
//! sentence and the package tools hand it to the model, so it says what did not happen and what
//! to do next without paths, identifiers or English.

pub const ARGUMENTS_INVALID: &str = "package.arguments_invalid: 参数不符合这个工具的输入格式，程序没有运行；请按工具说明改好参数再调用。";
pub const HTTP_REQUEST_INVALID: &str = "package.http_request_invalid: 按这个工具的声明和参数组不出合法的网络请求，请求没有发出；请检查参数和工具设置，仍然如此说明这个工具包需要更新。";
pub const MATERIALIZED_CONTENT_CHANGED: &str = "package.materialized_content_changed: 工具包的内容取出来后与保存时不一致，没有使用它；请在工具包页重新导入这个版本。";
pub const OWNER_STOPPING: &str =
    "package.owner_stopping: Aura 正在退出，工具包操作没有开始；重新打开 Aura 后再试。";
pub const PREPARATION_OWNER_MISMATCH: &str = "package.preparation_owner_mismatch: 这次验证是在 Aura 重启之前准备的，没有开始；请重新准备验证。";
pub const OWNER_UNAVAILABLE: &str = "package.owner_unavailable: 工具包的运行记录在之前的一次操作中意外中断，这次没有执行；请重启 Aura。";
pub const CONFORMANCE_CAPACITY: &str = "package.conformance_capacity: 同时进行的验证太多（最多 32 个），这次没有开始；请等其他验证结束后再试。";
pub const INVALID_PROCESS_LIMITS: &str = "package.invalid_process_limits: 这次运行的时间上限或输出上限不在允许的范围内，没有开始；请用默认的上限再试。";
pub const SETTINGS_NOT_CANONICAL: &str = "package.settings_not_canonical: 工具设置不符合这个版本的要求，没有执行；请在工具包页重新填写这个工具的设置。";
pub const STORE_UNAVAILABLE: &str = "package.store_unavailable: 工具包数据在之前的一次操作中意外中断，现在读写不了，这次没有执行；请重启 Aura。";
pub const ABSOLUTE_OWNER_ROOT_REQUIRED: &str = "package.absolute_owner_root_required: 工具包的运行文件夹不是完整路径，工具包功能没有启动；请重启 Aura，仍然如此请重新安装。";
pub const OWNER_DIRECTORY_REQUIRED: &str = "package.owner_directory_required: 工具包的运行位置不是文件夹，工具包功能没有启动；请检查 Aura 数据文件夹后重启 Aura。";
pub const OWNER_MARKER_INVALID: &str = "package.owner_marker_invalid: 工具包运行文件夹里的占用标记不对，工具包功能没有启动；请重启 Aura。";
pub const SCRATCH_ROOT_CHANGED: &str = "package.scratch_root_changed: 清理上次留下的临时运行文件夹时，它被换成了别的东西，没有清理，工具包功能没有启动；请重启 Aura。";
pub const RUN_STATUS_UNAVAILABLE: &str =
    "package.run_status_unavailable: 读不到这次验证的状态；请重启 Aura 后重新验证。";
pub const DIRECTORY_CLAIM_MISSING: &str = "package.directory_claim_missing: 这个工具的工作目录声明不完整，程序没有运行；这个工具包需要更新。";
pub const DIRECTORY_ARGUMENT_MISSING: &str = "package.directory_argument_missing: 参数里缺少这个工具要用作工作目录的路径，程序没有运行；请在参数里给出工作区里的目录再调用。";
pub const DIRECTORY_OUTSIDE_WORKSPACE: &str = "package.directory_outside_workspace: 工作目录不在工作区里，程序没有运行；请换成工作区里的目录再调用。";
pub const WORKSPACE_REQUIRED: &str = "package.workspace_required: 这个工具要在工作区里运行，现在没有可用的工作区，程序没有运行；请在工作区里的会话中使用它。";
pub const CONFORMANCE_VERSION_CHANGED: &str = "package.conformance_version_changed: 要验证的内容与所选的版本对不上（版本可能刚被替换），验证没有开始；请刷新工具包页后重新验证。";
pub const CLEANUP_UNCONFIRMED: &str = "package.cleanup_unconfirmed: 程序是否已经完全停止还没能确认，结果不明，Aura 不会自动重试；程序可能已经做了改动，再运行之前先确认它的效果。";
pub const TOOL_MISSING: &str = "package.tool_missing: 这个工具包现在的版本里没有这个工具，程序没有运行；请刷新工具列表后再试。";
pub const PROGRAM_RUNTIME_REQUIRED: &str = "package.program_runtime_required: 这个工具不是用程序运行的，程序没有运行；请重新调用，仍然如此说明这个工具包需要更新。";
pub const PROCESS_CAPABILITY_REQUIRED: &str = "package.process_capability_required: 这个工具没有声明要运行程序，程序没有运行；这个工具包的声明有误，需要更新。";
pub const CONFIGURATION_INVALID: &str = "package.configuration_invalid: 这个工具的设置不符合它的要求，程序没有运行；请在工具包页改好这个工具的设置。";
pub const CONFIGURATION_REQUIRED: &str = "package.configuration_required: 这个工具需要的设置还没有填好，程序没有运行；请在工具包页填好这个工具的设置。";
pub const SYSTEM_PATH_INVALID: &str = "package.system_path_invalid: Windows 系统文件夹的路径传不给程序，程序没有运行；这台电脑上暂时不能运行程序类工具。";
pub const PROGRAM_CREDENTIALS_UNSUPPORTED: &str = "package.program_credentials_unsupported: 运行程序的工具不能使用 Aura 保存的凭据，程序没有运行；这个工具包的凭据声明有误，需要更新。";
pub const OUTPUT_PATH_INVALID: &str = "package.output_path_invalid: 程序输出文件夹的路径传不给程序，程序没有运行；请重启 Aura 后再试。";
pub const ARTIFACT_NOT_DECLARED: &str = "package.artifact_not_declared: 程序交出了文件，但这个工具没有声明会交出文件，结果没有采用；这个工具包需要更新。";
pub const ARTIFACT_SLOT_CONFLICT: &str = "package.artifact_slot_conflict: 程序对同一个输出报了两份不同的文件，结果没有采用；这个工具包需要更新。";
pub const ARTIFACT_LIMIT: &str = "package.artifact_limit: 程序交出的文件太多或太大（个数、单个和合计大小都有上限），结果没有采用；请换更小的输入再调用。";
pub const ARTIFACT_MEDIA_TYPE: &str =
    "package.artifact_media_type: 程序交出的文件类型写法不对，结果没有采用；这个工具包需要更新。";
pub const ARTIFACT_ALIAS: &str =
    "package.artifact_alias: 程序把同一个文件报成了两份输出，结果没有采用；这个工具包需要更新。";
pub const ARTIFACT_LENGTH: &str = "package.artifact_length: 程序交出的文件大小与它报的不一致，结果没有采用；请重新调用，仍然如此说明这个工具包需要更新。";
pub const ARTIFACT_IDENTITY: &str = "package.artifact_identity: 程序交出的文件内容与它报的校验值不一致，结果没有采用；请重新调用，仍然如此说明这个工具包需要更新。";
pub const STDIO_FAILURE: &str = "package.stdio_failure: 程序没有按 Aura 的工具协议给出最终结果，结果没有采用；这个工具包需要更新。";
pub const NOT_INSTALLED: &str = "package.not_installed: 这个工具包还没有安装或还没有信任，这次没有执行；请先在工具包页安装并信任它。";
pub const IMPORT_PATH_ABSOLUTE: &str = "package.import_path_absolute: 导入要用从盘符开始的完整路径，这次没有导入；请重新选择文件夹或文件。";
pub const IMPORT_ROOT_CHANGED: &str = "package.import_root_changed: 工具包的导入暂存文件夹被换成了别的东西，没有清理，工具包功能没有启动；请重启 Aura。";
pub const PREPARATIONS_UNAVAILABLE: &str = "package.preparations_unavailable: 验证的准备记录在之前的一次操作中意外中断，这次没有执行；请重启 Aura 后重新准备验证。";
pub const PREPARATION_EXPIRED: &str =
    "package.preparation_expired: 预览已经失效，验证没有开始；请重新准备验证。";
pub const RUNS_UNAVAILABLE: &str = "package.runs_unavailable: 验证的运行记录在之前的一次操作中意外中断，这次没有执行；请重启 Aura。";
pub const RUN_NOT_ACTIVE: &str =
    "package.run_not_active: 这次验证已经结束，没有什么可以停止；请刷新工具包页查看结果。";
pub const RUN_BINDING: &str =
    "package.run_binding: 这次验证属于另一个版本，没有停止；请刷新工具包页后再试。";
pub const NOT_FOUND: &str = "package.not_found: 找不到这个工具包，它可能已被删除；请刷新列表。";
pub const VERSION_NOT_FOUND: &str =
    "package.version_not_found: 找不到这个版本，它可能已被删除或替换；请刷新列表。";
pub const CONFORMANCE_BINDING: &str =
    "package.conformance_binding: 验证记录与这个版本对不上，没有显示；请刷新后重新验证。";
pub const CONTENT_UNAVAILABLE: &str = "package.content_unavailable: 这个版本的内容现在不可用（还没装好，或者校验没有通过）；请在工具包页检查它。";
pub const ENVIRONMENT_MISSING: &str = "package.environment_missing: 运行验证需要的环境没有准备好，验证没有开始；请重试，仍然如此请重启 Aura。";
pub const UNKNOWN_METHOD: &str = "package.unknown_method: Aura Core 处理不了这个工具包请求（Aura 内部出错），没有执行；请重启 Aura 后重试。";

/// What the 工具包 page says for a bare code the package store answers with, the part after
/// `package.`.
pub fn bare_code_sentence(code: &str) -> &'static str {
    match code {
        "not_found" | "version_not_found" => "找不到这个工具包或版本，请刷新列表。",
        "conformance_not_approved" | "conformance_missing" | "conformance_incomplete" => {
            "这个版本没有通过验证，不能安装。"
        }
        "settings_invalid" | "settings_not_canonical" => "工具设置不符合这个版本的要求。",
        "not_installed" => "这个工具包尚未安装。",
        "not_trusted" => "这个工具包已撤销信任，请重新安装。",
        "program_credentials_unsupported" => "运行程序的工具不能使用 Aura 保存的凭据，不能安装。",
        "owner_stopping" => "Aura 正在退出，工具包操作没有开始。",
        "conformance_capacity" => "同时进行的验证过多，请稍后重试。",
        "run_not_active" => "这次验证已经结束。",
        _ => "工具包操作未完成。",
    }
}

/// The code a failure text begins with: the part before `: `, or the whole text when it is a bare
/// code.
pub fn code_of(text: &str) -> &str {
    let text = text.trim();
    text.split_once(": ").map_or(text, |(code, _)| code)
}

/// Whether any cause in `error` is the failure `code`, bare or with its sentence.
pub fn chain_has(error: &anyhow::Error, code: &str) -> bool {
    error.chain().any(|cause| code_of(&cause.to_string()) == code)
}
