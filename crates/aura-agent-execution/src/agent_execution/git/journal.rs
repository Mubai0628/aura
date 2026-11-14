//! The words a Git call's journal gives the model when the call fails or its end is unknown.
use aura_execution_contract::SideEffectStateV1;
use aura_persistence_core::PersistenceCoreError;
use aura_persistence_v2::tool::refusal_text;
use aura_workspace::WorkspaceError;

pub(super) const PERMISSION_WITHDRAWN: &str = "git.permission_withdrawn: 会话的权限模式在这次 Git 调用开始前改变了，新的模式不允许它；没有执行任何 Git 命令，仓库没有改变。需要时请重新调用，它会按新的模式检查。";
pub(super) const REPOSITORY_CHANGED: &str = "git.repository_changed: 准备之后仓库本身已经改变（仓库文件夹被替换、Git 程序被更换，或与外部程序有关的 Git 配置被修改），这次 Git 调用没有执行，Aura 没有改动仓库。请重新调用。";
pub(super) const REMOTE_FACTS_MISSING: &str = "git.remote_facts_missing: 这次远端操作缺少批准时记录的远端目标，没有执行任何 Git 命令，本地和远端都没有改变。请重新调用，按当前目标重新批准。";
pub(super) const UNKNOWN_TOOL: &str = "git.unknown_tool: Aura 不认识这个 Git 读取工具，没有执行。请改用 git.status、git.diff 或 git.log。";

const NOT_STARTED: &str = "git.not_started";
const CHECK_BEFORE_REDOING: &str =
    "请先用 git.status 和 git.log 核对仓库状态和历史，再决定是否重做；禁止自动重放。";

/// The journal's `error` for a Git call that did not succeed: the Git service's own `git.*`
/// sentence as it is; any other cause worded with what the call did, never a Rust value or an
/// internal identifier.
pub(super) fn error_text(error: &anyhow::Error, side_effect: SideEffectStateV1) -> String {
    let text = error.to_string();
    if worded(&text) {
        return text;
    }
    if !matches!(side_effect, SideEffectStateV1::NoEffect | SideEffectStateV1::NotStarted) {
        return format!(
            "git.outcome_unknown: Git 命令已经开始运行，之后 Aura 内部出错，结果无法确认。{CHECK_BEFORE_REDOING}"
        );
    }
    let code = leading_code(&text).unwrap_or(NOT_STARTED);
    let chain = error.chain().map(ToString::to_string).collect::<Vec<_>>().join(": ");
    if let Some(sentence) = refusal_text::known(&chain) {
        return format!("{code}: {sentence}");
    }
    if matches!(error.downcast_ref::<PersistenceCoreError>(), Some(PersistenceCoreError::Conflict))
    {
        return format!(
            "{NOT_STARTED}: 这次调用所属的轮次在 Git 运行前已经停止或改变，没有执行任何 Git 命令，仓库没有改变。需要时请重新调用。"
        );
    }
    if error.is::<serde_json::Error>() {
        return format!(
            "git.invalid_arguments: 这次 Git 调用的参数无法解析（{}），没有执行，仓库没有改变。请按工具说明改正参数后再试。",
            bounded(&text)
        );
    }
    format!(
        "{code}: Aura 内部出错，这次 Git 调用没有完成，仓库没有改变。请再试一次；仍然失败时请告诉用户。"
    )
}

/// A workspace folder Aura cannot reach, in words; Git did not run.
pub(super) fn workspace(error: WorkspaceError) -> anyhow::Error {
    let (cause, next) = match error {
        WorkspaceError::Offline => {
            ("工作区当前不可用（离线或没有打开）", "请让用户重新打开这个工作区后再试")
        }
        WorkspaceError::Archived => ("工作区已经归档", "请让用户恢复这个工作区后再试"),
        WorkspaceError::RootIdentityChanged | WorkspaceError::InvalidRoot => {
            ("工作区文件夹已被移动、替换或删除", "请让用户在 Aura 中重新定位这个工作区后再试")
        }
        WorkspaceError::Busy { .. } => ("工作区正忙", "请稍后再试"),
        _ => ("Aura 无法访问工作区文件夹", "请稍后再试；仍然失败时请告诉用户"),
    };
    anyhow::anyhow!("git.workspace_unavailable: {cause}，这次 Git 调用没有执行。{next}。")
}

/// `<git code>: <sentence>`: a line the Git service wrote for the model.
fn worded(text: &str) -> bool {
    let Some((code, rest)) = text.split_once(':') else {
        return false;
    };
    let rest = rest.trim();
    code.starts_with("git.")
        && is_code(code)
        && rest.chars().count() >= 8
        && (rest.contains(' ') || rest.chars().any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch)))
}

/// The code a bare or `<code>: …` reason starts with.
fn leading_code(text: &str) -> Option<&str> {
    let code = text.split(':').next()?.trim();
    is_code(code).then_some(code)
}

fn is_code(token: &str) -> bool {
    token.len() <= 96
        && token.contains('.')
        && token.split('.').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        })
}

/// Library text shown after a sentence: its start, on one line.
fn bounded(text: &str) -> String {
    let mut shown: String =
        text.chars().take(200).map(|ch| if ch.is_control() { ' ' } else { ch }).collect();
    if text.chars().count() > 200 {
        shown.push('…');
    }
    shown
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_git_services_own_sentence_reaches_the_model_unchanged() {
        let line = "git.index_locked: Git 索引被锁定。请等其他 Git 操作结束后再试。Git 输出：fatal";
        let said = error_text(&anyhow::anyhow!(line), SideEffectStateV1::NoEffect);
        assert_eq!(said, line);
        assert_eq!(
            error_text(&anyhow::anyhow!(REPOSITORY_CHANGED), SideEffectStateV1::NoEffect),
            REPOSITORY_CHANGED
        );
    }

    #[test]
    fn a_bare_or_internal_cause_says_what_the_call_did() {
        let conflict = anyhow::Error::from(PersistenceCoreError::Conflict);
        let said = error_text(&conflict, SideEffectStateV1::NoEffect);
        assert!(said.starts_with("git.not_started: "), "{said}");
        assert!(said.contains("没有执行任何 Git 命令") && !said.contains("conflict"), "{said}");

        let scope = error_text(
            &anyhow::anyhow!("agent.source_or_scope_changed"),
            SideEffectStateV1::NoEffect,
        );
        assert!(scope.starts_with("agent.source_or_scope_changed: "), "{scope}");
        assert!(scope.len() > "agent.source_or_scope_changed: ".len() + 8, "{scope}");

        let parse = serde_json::from_str::<u32>("\"x\"").expect_err("not a number");
        let said = error_text(&parse.into(), SideEffectStateV1::NoEffect);
        assert!(said.starts_with("git.invalid_arguments: "), "{said}");

        let raw = error_text(&anyhow::anyhow!("Thing {{ field: 1 }}"), SideEffectStateV1::NoEffect);
        assert!(raw.starts_with("git.not_started: ") && !raw.contains("Thing"), "{raw}");
    }

    #[test]
    fn a_cause_after_git_started_never_claims_nothing_changed() {
        let said = error_text(&anyhow::anyhow!("invalid type"), SideEffectStateV1::Unknown);
        assert!(said.starts_with("git.outcome_unknown: "), "{said}");
        assert!(said.contains("禁止自动重放") && !said.contains("没有改变"), "{said}");
    }

    #[test]
    fn an_unreachable_workspace_names_the_cause_and_the_next_step() {
        let said = workspace(WorkspaceError::Offline).to_string();
        assert!(said.starts_with("git.workspace_unavailable: 工作区当前不可用"), "{said}");
        assert!(said.contains("没有执行") && said.ends_with("后再试。"), "{said}");
        assert!(worded(&said), "{said}");
    }
}
