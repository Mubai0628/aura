//! Closed, credential-free preflight explanations. Never publish raw Git diagnostics.
pub(in crate::agent_execution) fn admission_feedback(
    error: &anyhow::Error,
) -> Option<&'static str> {
    Some(match error.to_string().split(':').next()? {
        "git.multiple_remote_urls" => {
            "git.multiple_remote_urls: 此远端有多个地址，无法绑定单一目的地批准；请在终端处理或配置单一推送地址。"
        }
        "git.opaque_remote" => {
            "git.opaque_remote: 不透明远端助手无法绑定明确的仓库URL；请在终端处理或配置标准Git远端地址。"
        }
        "git.remote_required" => {
            "git.remote_required: 请指定已配置的远端，或先为本地分支设置上游。"
        }
        "git.remote_branch_required" => {
            "git.remote_branch_required: 请明确远端分支，或先设置该远端的上游。"
        }
        "git.remote_missing" | "git.invalid_remote" => {
            "git.remote_missing: 请使用Git配置中的远端名称，不能向模型工具提供URL。"
        }
        "git.exact_lease_required" => {
            "git.exact_lease_required: force_with_lease必须为完整的期望远端提交号；请先fetch核对。"
        }
        "git.invalid_branch" => {
            "git.invalid_branch: 分支名称不合法，请使用git.status核对本地分支并明确远端分支。"
        }
        "git.invalid_remote_url" => {
            "git.invalid_remote_url: 配置中的远端地址无法规范化，请在终端核对并修复。"
        }
        // The repository itself, read while
        // the call is planned. A folder that is no repository used to be refused with only
        // "the target could not be determined".
        "git.not_repository" => {
            "git.not_repository: 此工作区不是 Git 仓库，Git 工具在这里不能使用。不需要版本记录时请不用 Git 继续；需要时请用户在终端运行 git init。"
        }
        "git.unsafe_directory" => {
            "git.unsafe_directory: 仓库所有者与当前用户不符，Git 拒绝读取；请用户在终端核对仓库所有权与 safe.directory 配置。"
        }
        "git.config_invalid" => {
            "git.config_invalid: 仓库的 Git 配置无法读取；请用户在终端修复配置后再试。"
        }
        _ => return None,
    })
}
