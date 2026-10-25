use anyhow::Result;
use aura_backend_windows_user::git::GitRunner;
use aura_workspace_domain::{GitRuntimeStateV1, GitRuntimeStatusV1};
pub(crate) fn available() -> bool {
    GitRunner::detect().is_ok_and(|result| result.state == "available")
}
pub(crate) fn constrain_candidates(
    candidates: &mut [aura_tool_registry::RegistryCandidateV1],
    available: bool,
) {
    if !available {
        for candidate in candidates {
            if candidate.definition.canonical_name.as_str().starts_with("git.") {
                candidate.instance.runtime_health =
                    aura_tool_domain::ToolRuntimeHealthV1::Unavailable;
            }
        }
    }
}
/// Git gets its one full check as soon as Core is ready, on a
/// thread of its own, so the first review or Git tool finds it checked. Once per process; tests
/// leave it out so no test starts a machine Git it did not ask for.
pub(crate) fn check_after_startup() {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if cfg!(test) || STARTED.swap(true, std::sync::atomic::Ordering::AcqRel) {
        return;
    }
    let _ = std::thread::Builder::new().name("aura-git-check".into()).spawn(|| {
        let _ = status();
    });
}

pub(crate) fn status() -> Result<GitRuntimeStatusV1> {
    let result = match GitRunner::detect() {
        Ok(result) => result,
        Err(error) => {
            crate::append_core_log(
                "git.detection_failed",
                serde_json::json!({"error":super::config::redact(&error.to_string())}),
            );
            return Ok(GitRuntimeStatusV1 {
                state: GitRuntimeStateV1::Invalid,
                executable: None,
                version: None,
                message: unavailable_message("invalid").into(),
                next_step: "请核对本机 Git 安装后重新检测；其他工具仍可使用".into(),
                installation_url: "https://git-scm.com/download/win".into(),
            });
        }
    };
    let state = match result.state {
        "available" => GitRuntimeStateV1::Available,
        "too_old" => GitRuntimeStateV1::TooOld,
        "missing" => GitRuntimeStateV1::Missing,
        _ => GitRuntimeStateV1::Invalid,
    };
    Ok(GitRuntimeStatusV1 {
        state,
        executable: result
            .executable
            .map(|path| aura_domain_common::display_path(std::path::Path::new(&path))),
        version: result.version,
        message: unavailable_message(result.state).into(),
        next_step: if result.state == "available" {
            "更换或升级 Git 后可重新检测"
        } else {
            "请从 Git 官方网站安装或升级至 2.40 以上，再点击重新检测"
        }
        .into(),
        installation_url: "https://git-scm.com/download/win".into(),
    })
}
pub(super) fn unavailable_message(state: &str) -> &'static str {
    match state {
        "available" => "本机 Git 可用",
        "missing" => "未找到本机 Git，Git 工具与审查页已停用",
        "too_old" => "本机 Git 版本低于 2.40，Git 工具与审查页已停用",
        _ => "无法验证本机 Git，Git 工具与审查页已停用",
    }
}

#[cfg(test)]
mod tests {
    /// The Git settings card showed the program as Core holds it, in Windows' verbatim
    /// spelling (`\\?\C:\Program Files\Git\…`).
    #[test]
    fn the_git_program_is_named_in_its_ordinary_spelling() {
        let status = super::status().expect("Git status");
        if let Some(executable) = status.executable {
            assert!(!executable.starts_with(r"\\?\"), "{executable}");
            assert!(std::path::Path::new(&executable).is_file(), "{executable}");
        }
    }
}
