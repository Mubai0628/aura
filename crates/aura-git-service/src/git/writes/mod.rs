//! Native mutations retain ordinary Git configuration and report what can be established afterwards.
mod apply;
mod branch;
mod carry;
mod commit;
mod endings;
mod remote;
mod worktree;
use super::*;
use aura_execution_contract::SideEffectStateV1;
pub use carry::{CARRY_MAX_BYTES, CARRY_MAX_FILES};

const CHECK_STAGING: &str = "已运行暂存命令，请用 git.status 核对暂存区。";

pub struct GitWriter {
    reader: GitReader,
    started: bool,
    deadline: std::time::Instant,
    on_started: Option<Box<dyn FnOnce() + Send>>,
}

#[derive(Debug)]
pub struct GitWriteInterrupted(pub String);
impl std::fmt::Display for GitWriteInterrupted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "git.outcome_unknown: {}。请先用 git.status 和 git.log 核对仓库状态和历史，再决定是否重做；禁止自动重放。",
            self.0
        )
    }
}
impl std::error::Error for GitWriteInterrupted {}

impl GitWriter {
    pub fn new(reader: GitReader, on_started: impl FnOnce() + Send + 'static) -> Self {
        Self {
            reader,
            started: false,
            deadline: std::time::Instant::now() + Duration::from_secs(120),
            on_started: Some(Box::new(on_started)),
        }
    }
    pub fn side_effect<T>(&self, result: &Result<T>) -> SideEffectStateV1 {
        match result {
            _ if !self.started => SideEffectStateV1::NoEffect,
            Ok(_) => SideEffectStateV1::FullyApplied,
            Err(error) if error.is::<commit::EmptyCommit>() => SideEffectStateV1::NoEffect,
            Err(_) => SideEffectStateV1::Unknown,
        }
    }
    fn native(&mut self, args: &[String]) -> Result<()> {
        self.output(args).map(|_| ())
    }
    /// Runs a write and gives what it printed, trimmed; a failed write is an error.
    fn output(&mut self, args: &[String]) -> Result<String> {
        let result = self.native_result(args)?;
        if result.exit_code != Some(0) {
            // The command's name, after any `-c name=value` before it.
            let name = args
                .iter()
                .find(|arg| !arg.starts_with('-') && !arg.contains('='))
                .map_or("write", String::as_str);
            anyhow::bail!("{}", write_failure(name, result, false));
        }
        Ok(String::from_utf8_lossy(&result.stdout).trim().to_owned())
    }
    /// Marks the write as started where files are written outside Git.
    fn begin(&mut self) {
        self.started = true;
        if let Some(release) = self.on_started.take() {
            release();
        }
    }
    /// Runs a write to its end and gives its exit as it is; a stop before its end is an error.
    fn native_result(&mut self, args: &[String]) -> Result<GitProcessResult> {
        let mut command = vec![
            "--no-pager".into(),
            "--literal-pathspecs".into(),
            "-c".into(),
            "color.ui=false".into(),
            "-c".into(),
            "protocol.allow=never".into(),
        ];
        command.extend_from_slice(args);
        let remaining = self.deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            return Err(GitWriteInterrupted(
                "Git 写入用完了 120 秒的时限，后面的步骤没有执行，前面的步骤可能已经改变了仓库"
                    .into(),
            )
            .into());
        }
        let reader = &self.reader;
        let started = &mut self.started;
        let lease = &mut self.on_started;
        let result = reader.runner.run_native_at_start(
            &reader.root,
            &command,
            &reader.cancel,
            (remaining, 256 * 1024),
            &mut || {
                *started = true;
                if let Some(release) = lease.take() {
                    release();
                }
            },
        )?;
        if !result.stopped {
            return Err(StopUnconfirmed.into());
        }
        if result.reason != "exited" {
            if !self.started && result.reason == "cancelled" {
                return Err(GitCancelled.into());
            }
            return Err(GitWriteInterrupted(format!(
                "Git 在写入途中{}，写入可能已部分完成",
                match result.reason {
                    "cancelled" => "被停止",
                    "timed_out" => "用完了时限被停止",
                    "output_limit" => "因输出超过上限被停止",
                    _ => "出错退出",
                }
            ))
            .into());
        }
        Ok(result)
    }
    fn refresh(&mut self) -> Result<()> {
        self.reader.deadline = std::time::Instant::now() + Duration::from_secs(30);
        let result = config::read_configuration(&self.reader);
        let (configuration, programs) = result.map_err(|error| {
            GitWriteInterrupted(format!(
                "Git 写入已成功退出，但随后读不到仓库配置，无法核对结果（{}）",
                brief(&error)
            ))
        })?;
        self.reader.config = configuration;
        self.reader.programs = programs;
        Ok(())
    }
}

/// The cause inside another error as one short clause: no code and no Git output.
fn brief(error: &anyhow::Error) -> String {
    let text = error.to_string();
    let text = text.split(config::DETAIL_LABEL).next().unwrap_or_default();
    let text = match text.split_once(": ") {
        Some((code, rest)) if code.starts_with("git.") && !code.contains(' ') => rest,
        _ => text,
    };
    config::shown(text.split('。').next().unwrap_or_default().trim())
}

fn write_failure(command: &str, mut result: GitProcessResult, staged_first: bool) -> String {
    result.stderr.push(b'\n');
    result.stderr.extend(result.stdout);
    failure(command, &result.stderr, staged_first)
}

/// A Git write that exited unsuccessfully: why, what did not happen, and Git's own words.
fn failure(command: &str, diagnostic: &[u8], staged_first: bool) -> String {
    let text = String::from_utf8_lossy(diagnostic);
    let lower = text.to_ascii_lowercase();
    let (code, explanation) = if lower.contains("index.lock") || lower.contains("cannot lock ref") {
        (
            "locked",
            "Git 文件被锁定（例如 .git/index.lock），可能有其他 Git 操作正在进行；请等它结束后再试，Aura 不会删除锁文件",
        )
    } else if lower.contains("would be overwritten") || lower.contains("local changes") {
        ("local_changes", "本地改动会被覆盖；请先提交或收起这些改动，Aura 不会强制丢弃")
    } else if lower.contains("unmerged")
        || lower.contains("unresolved conflict")
        || lower.contains("middle of a merge")
    {
        ("operation_in_progress", "合并或其他操作尚未完成；请先在终端处理冲突或完成操作")
    } else if lower.contains("hook") {
        ("hook_rejected", "Git hook 拒绝了操作；请根据 hook 的输出修复后再试，Aura 不会跳过 hook")
    } else if lower.contains("gpg") || lower.contains("signing") {
        ("signing_failed", "Git 签名失败；请核对本机签名配置，Aura 不会关闭签名")
    } else {
        ("write_failed", "Git 写入没有成功；请根据 Git 输出排查 hook、签名或仓库问题后再试")
    };
    let effect = match command {
        "commit" => "，没有生成新提交",
        "add" => "，暂存区没有改变",
        "branch" => "，没有创建分支",
        "switch" => "；请用 git.status 核对当前分支",
        _ => "",
    };
    let note = if staged_first { CHECK_STAGING } else { "" };
    format!(
        "git.{code}: {explanation}。git {command} 没有成功{effect}。{note}{}{}",
        config::DETAIL_LABEL,
        printed(&text)
    )
}

/// The bound on a failed write's output; with the sentence before it the message stays under the
/// 8192 bytes a model-readable journal error may hold.
const PRINTED_BYTES: usize = 7600;

/// What a failed write printed, such as a hook's report, kept as lines because the next step
/// depends on it: credentials in URLs removed, other control characters turned into spaces.
fn printed(text: &str) -> String {
    let mut shown = String::new();
    for ch in config::redact(text).trim().chars() {
        let ch = if ch.is_control() && !matches!(ch, '\n' | '\r' | '\t') { ' ' } else { ch };
        if shown.len() + ch.len_utf8() > PRINTED_BYTES {
            shown.push('…');
            break;
        }
        shown.push(ch);
    }
    if shown.is_empty() { "（没有输出）".into() } else { shown }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_write_says_what_did_not_happen_with_bounded_git_output() {
        let stderr = format!(
            "pre-commit hook rejected fixture\u{1b}[0m\nsrc/lib.rs:1: {}",
            "y".repeat(9000)
        );
        let message = failure("commit", stderr.as_bytes(), true);
        assert!(message.starts_with("git.hook_rejected: "), "{message}");
        assert!(message.contains("git commit 没有成功，没有生成新提交"), "{message}");
        let output = message.split(config::DETAIL_LABEL).nth(1).expect("Git output");
        assert!(output.starts_with("pre-commit hook rejected fixture [0m\nsrc/lib.rs:1: y"));
        assert!(output.len() <= PRINTED_BYTES + '…'.len_utf8() && output.ends_with('…'));
        assert!(message.len() < 8000, "{}", message.len());
        assert_eq!(printed(" \n"), "（没有输出）");
    }

    #[test]
    fn an_interrupted_write_names_the_check_and_forbids_replay() {
        let shown = GitWriteInterrupted("git commit 已成功退出".into()).to_string();
        assert!(shown.starts_with("git.outcome_unknown: git commit 已成功退出。"), "{shown}");
        assert!(shown.contains("git.status") && shown.contains("禁止自动重放"), "{shown}");
        let inner = anyhow::anyhow!("git.index_locked: Git 索引被锁定。请等待。Git 输出：fatal");
        assert_eq!(brief(&inner), "Git 索引被锁定");
    }
}
