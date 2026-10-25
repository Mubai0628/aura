//! One Hook call: hold the package's files, check the trusted fingerprint, run the program
//! as the user in a Job with one JSON document on stdin, and read one decision back.
use super::registry::Candidate;
use aura_backend_windows_user::compat::CompatBackend;
use aura_backend_windows_user::program::{
    ProgramEnvironment, ProgramInput, ProgramSpec, ProgramStop, run,
};
use aura_domain_common::{SessionId, TurnTaskId};
use aura_hooks::{
    HOOK_PROTOCOL, HookCommand, HookDecision, HookEnvelope, HookEnvelopeHook, HookEvent,
    HookFailureKind, MAX_RESULT_BYTES, parse_result, scan_package,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

const MAX_STDERR_BYTES: usize = 64 * 1024;

/// What the program did, before the failure policy is applied.
pub(crate) struct Attempt {
    pub decision: Result<HookDecision, (HookFailureKind, String)>,
    pub input: String,
    pub output: String,
    pub stderr: Option<String>,
}

pub(crate) struct ExecContext<'a> {
    pub event: HookEvent,
    pub session_id: Option<SessionId>,
    pub turn_task_id: Option<TurnTaskId>,
    pub workspace_root: Option<&'a Path>,
    pub scratch_root: &'a Path,
    pub powershell: Option<&'a CompatBackend>,
    pub cancelled: &'a AtomicBool,
}

fn failed(kind: HookFailureKind, detail: impl Into<String>, input: String) -> Attempt {
    Attempt { decision: Err((kind, detail.into())), input, output: String::new(), stderr: None }
}

pub(crate) fn invoke(hook: &Candidate, context: &ExecContext<'_>, payload: &Value) -> Attempt {
    let envelope = HookEnvelope {
        protocol: HOOK_PROTOCOL,
        event: context.event,
        hook: HookEnvelopeHook {
            name: hook.definition.name.clone(),
            version: hook.definition.version.clone(),
        },
        session_id: context.session_id.map(|id| id.to_string()),
        turn_task_id: context.turn_task_id.map(|id| id.to_string()),
        workspace_root: context.workspace_root.map(|root| root.display().to_string()),
        payload: payload.clone(),
    };
    let input = serde_json::to_string(&envelope).unwrap_or_default();
    let root = Path::new(&hook.path);
    // Hold every file against writes first, then check the content: what is checked is what runs.
    let Ok(listed) = scan_package(root) else {
        return failed(HookFailureKind::ContentChanged, "Hook 包已无法读取，需要重新信任", input);
    };
    let Ok(held) = listed
        .files
        .iter()
        .map(|file| {
            aura_windows_platform::open_verified_import_source(
                &root.join(file.path.replace('/', "\\")),
                aura_windows_platform::NativeFileAccess::Read,
            )
        })
        .collect::<std::io::Result<Vec<_>>>()
    else {
        return failed(HookFailureKind::ContentChanged, "Hook 包的文件被占用或已变化", input);
    };
    let package = match scan_package(root) {
        Ok(package) if package.fingerprint == hook.trusted_fingerprint => package,
        _ => {
            return failed(
                HookFailureKind::ContentChanged,
                "Hook 包的内容在信任之后变了，需要在设置里重新信任",
                input,
            );
        }
    };
    let (image_path, arguments) = match &package.definition.command {
        HookCommand::Executable { arguments, .. } => (package.command_path(), arguments.clone()),
        HookCommand::PowershellScript { arguments, .. } => {
            let Some(powershell) = context.powershell else {
                return failed(
                    HookFailureKind::NotStarted,
                    "Aura 自带的 PowerShell 暂时不可用，脚本 Hook 没有运行",
                    input,
                );
            };
            let mut all = [
                "-MTA",
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ]
            .map(str::to_owned)
            .to_vec();
            all.push(package.command_path().display().to_string());
            all.extend(arguments.iter().cloned());
            (powershell.powershell_path().to_path_buf(), all)
        }
    };
    // The hook's program, the user's own, or Aura's PowerShell: read in full the first time, then
    // only compared until it changed or failed.
    let Ok((image, _image)) = aura_backend_windows_user::checked_program::capture(
        &image_path,
        aura_backend_windows_user::checked_program::Links::Single,
    ) else {
        return failed(HookFailureKind::NotStarted, "Hook 程序无法打开", input);
    };
    if let Err(error) = std::fs::create_dir_all(context.scratch_root) {
        return failed(HookFailureKind::NotStarted, format!("临时目录无法建立：{error}"), input);
    }
    let Ok(scratch) = tempfile::Builder::new().prefix("hook-run-").tempdir_in(context.scratch_root)
    else {
        return failed(HookFailureKind::NotStarted, "临时目录无法建立", input);
    };
    let mut declared = vec![
        ("AURA_HOOK_EVENT".to_owned(), context.event.name().to_owned()),
        ("AURA_HOOK_ROOT".to_owned(), root.display().to_string()),
    ];
    if let Some(workspace) = context.workspace_root {
        declared.push(("AURA_WORKSPACE_ROOT".to_owned(), workspace.display().to_string()));
    }
    let environment = match aura_windows_platform::current_user_environment()
        .map_err(anyhow::Error::from)
        .and_then(|block| ProgramEnvironment::from_block(block, &declared, true, scratch.path()))
    {
        Ok(environment) => environment,
        Err(error) => {
            return failed(HookFailureKind::NotStarted, format!("环境无法准备：{error}"), input);
        }
    };
    let working: PathBuf = context.workspace_root.unwrap_or(root).to_path_buf();
    let observed = run(
        ProgramSpec {
            image: &image,
            arguments: &arguments,
            working_directory: &working,
            environment: environment.entries(),
            input: ProgramInput::Document(input.as_bytes()),
            cancelled: context.cancelled,
            timeout: Duration::from_millis(package.definition.timeout_ms),
            max_output_bytes: MAX_RESULT_BYTES + MAX_STDERR_BYTES,
            limits: hook.limits,
        },
        &mut |_: &[u8]| Ok(vec![]),
        &mut |_| {},
    );
    drop(held);
    let _ = scratch.close();
    let observed = match observed {
        Ok(observed) => observed,
        Err(error) => {
            return failed(HookFailureKind::NotStarted, format!("Hook 没有启动：{error}"), input);
        }
    };
    let output = String::from_utf8_lossy(&observed.stdout).into_owned();
    let stderr = (!observed.stderr.is_empty()).then(|| {
        let text = String::from_utf8_lossy(&observed.stderr);
        text.chars().take(2000).collect::<String>()
    });
    let decision = match (observed.started, observed.reason, observed.exit_code) {
        (false, ProgramStop::Cancelled, _) | (_, ProgramStop::Cancelled, _) => {
            Err((HookFailureKind::Cancelled, "任务已停止，Hook 被中止".to_owned()))
        }
        (false, _, _) => Err((HookFailureKind::NotStarted, "Hook 没有启动".to_owned())),
        (_, ProgramStop::TimedOut, _) => Err((
            HookFailureKind::TimedOut,
            format!("超过 {} 毫秒，已停止整个进程树", package.definition.timeout_ms),
        )),
        (_, ProgramStop::OutputLimit, _) => {
            Err((HookFailureKind::OutputLimit, "输出超过上限".to_owned()))
        }
        (_, ProgramStop::Exited, Some(0)) => parse_result(context.event, &observed.stdout)
            .map_err(|error| (HookFailureKind::InvalidResult, error.to_string())),
        (_, ProgramStop::Exited, code) => {
            Err((HookFailureKind::ExitCode, format!("退出码 {}", code.map_or(-1, i64::from))))
        }
        (_, _, _) => Err((HookFailureKind::NotStarted, "Hook 运行出错".to_owned())),
    };
    let decision = match decision {
        Err((kind, detail)) if observed.memory_limit_reached => {
            Err((kind, format!("{detail}；它用到了插件设定的内存上限，可能因此失败")))
        }
        other => other,
    };
    Attempt { decision, input, output, stderr }
}
