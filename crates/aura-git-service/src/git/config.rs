use super::*;
use std::collections::{BTreeMap, BTreeSet};

// All scopes are read without running any external program. Low IL and the
// single-process Job enforce the boundary even if a repository invents a driver.
const PROGRAMS: &str = "^(filter\\..*\\.(clean|smudge|process|required)|diff\\..*\\.(command|textconv|cachetextconv)|core\\.(pager|editor|sshcommand|hookspath|gitproxy)|sequence\\.editor|pager\\..*|interactive\\.difffilter)$";

#[derive(Default)]
pub(super) struct DisabledPrograms {
    pub filters: BTreeSet<String>,
    pub diffs: BTreeSet<String>,
}
pub(super) fn read_configuration(reader: &GitReader) -> Result<(Vec<String>, DisabledPrograms)> {
    // --show-scope arrived in 2.26; --null preserves spaces, newlines and mixed-case subsections.
    let result = reader.execute(
        &["config", "--null", "--show-scope", "--get-regexp", PROGRAMS].map(str::to_string),
        &[],
    )?;
    ensure!(matches!(result.exit_code, Some(0 | 1)), "{}", failure(&result.stderr));
    let fields = result.stdout.split(|byte| *byte == 0).collect::<Vec<_>>();
    let mut effective = BTreeMap::new();
    for pair in fields.chunks_exact(2) {
        let (key, value) = std::str::from_utf8(pair[1])
            .ok()
            .and_then(|entry| entry.split_once('\n'))
            .ok_or_else(|| unreadable_output("config_format", "配置列表"))?;
        effective.insert(key.to_owned(), value.to_owned());
    }
    let mut args = Vec::new();
    let mut filters = BTreeSet::new();
    let mut disabled = DisabledPrograms::default();
    for (key, value) in effective {
        if let Some((driver, field)) =
            key.strip_prefix("filter.").and_then(|name| name.rsplit_once('.'))
        {
            filters.insert(driver.to_owned());
            if field != "required" && !value.is_empty() {
                disabled.filters.insert(driver.to_owned());
            }
        } else {
            if let Some((driver, field)) =
                key.strip_prefix("diff.").and_then(|name| name.rsplit_once('.'))
                && matches!(field, "textconv" | "command")
                && !value.is_empty()
            {
                disabled.diffs.insert(driver.to_owned());
            }
            args.extend([
                "-c".into(),
                format!("{key}={}", if key.ends_with(".cachetextconv") { "false" } else { "" }),
            ]);
        }
    }
    for driver in filters {
        for (field, value) in
            [("clean", ""), ("smudge", ""), ("process", ""), ("required", "false")]
        {
            args.extend(["-c".into(), format!("filter.{driver}.{field}={value}")]);
        }
    }
    Ok((args, disabled))
}

pub(super) fn failure(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let lower = text.to_ascii_lowercase();
    let (code, message, next) = if lower.contains("dubious ownership") {
        (
            "unsafe_directory",
            "仓库所有者与当前用户不符，Git 拒绝读取",
            "请在终端核对仓库所有权与 safe.directory 配置",
        )
    } else if lower.contains("not a git repository") {
        ("not_repository", "此工作区不是 Git 仓库", "需要版本记录时可以在终端运行 git init")
    } else if lower.contains("bad config")
        || lower.contains("bad boolean")
        || lower.contains("unable to read config")
    {
        ("config_invalid", "仓库的 Git 配置无法读取", "请根据 Git 输出修复配置后再试")
    } else if lower.contains("index.lock") {
        (
            "index_locked",
            "Git 索引被锁定（.git/index.lock 存在，可能有其他 Git 操作正在进行）",
            "请等其他 Git 操作结束后再试；Aura 不会删除锁文件",
        )
    } else if lower.contains("unknown revision")
        || lower.contains("needed a single revision")
        || lower.contains("bad revision")
    {
        (
            "revision_missing",
            "指定的提交、分支或范围在这个仓库里不存在",
            "请核对提交号、分支名或标签名后再试",
        )
    } else {
        (
            "read_failed",
            "Git 读取没有成功",
            "请再试一次；仍然失败时可以在终端运行相同的 git 命令查看原因",
        )
    };
    format!("git.{code}: {message}。{next}。{DETAIL_LABEL}{}", detail(&text))
}

/// Introduces Git's own words after Aura's sentence. A reader that must not publish raw Git
/// output cuts the message here.
pub const DETAIL_LABEL: &str = "Git 输出：";
const DETAIL_CHARS: usize = 300;

/// The start of Git's own output as one bounded line: credentials in URLs removed, control
/// characters and line breaks turned into spaces.
pub(super) fn detail(raw: &str) -> String {
    let flat: String =
        redact(raw).chars().map(|ch| if ch.is_control() { ' ' } else { ch }).collect();
    let words = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.is_empty() {
        return "（没有输出）".into();
    }
    let mut shown: String = words.chars().take(DETAIL_CHARS).collect();
    if shown.len() < words.len() {
        shown.push('…');
    }
    shown
}

/// Output Git printed in a form Aura does not recognise; nothing was concluded from it.
pub(super) fn unreadable_output(code: &str, what: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "git.{code}: Git 返回的{what}格式无法识别，这次读取没有得到结果。请再试一次；仍然失败时可以在终端运行相同的 git 命令查看。"
    )
}

/// A branch name Git does not accept.
pub(super) fn invalid_branch(name: &str) -> String {
    format!(
        "git.invalid_branch: 分支名「{}」不合法：不能为空、不能以 - 开头、不能含控制字符或空格等 Git 不允许的字符。请用 git.status（include_branches）核对分支名后再试。",
        shown(name)
    )
}

/// A value the caller gave, shown inside a sentence: bounded, on one line.
pub(super) fn shown(value: &str) -> String {
    let mut text: String =
        value.chars().take(80).map(|ch| if ch.is_control() { ' ' } else { ch }).collect();
    if value.chars().count() > 80 {
        text.push('…');
    }
    text
}

/// Never surface URL userinfo from Git diagnostics, including passwords/tokens.
pub(super) fn redact(input: &str) -> String {
    let mut value = input.to_owned();
    let mut cursor = 0;
    while let Some(found) = value[cursor..].find("://") {
        let start = cursor + found + 3;
        let end = value[start..]
            .find(['/', ' ', '\n', '\r', '\t', '\'', '"'])
            .map_or(value.len(), |n| start + n);
        if let Some(at) = value[start..end].rfind('@') {
            value.replace_range(start..start + at + 1, "[redacted]@");
        }
        cursor = start;
        if cursor >= value.len() {
            break;
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_read_failure_names_its_cause_and_ends_with_bounded_git_output() {
        let stderr = format!(
            "fatal: Unable to create 'C:/repo/.git/index.lock': File exists.\n\x1b[31m{}\n",
            "x".repeat(2000)
        );
        let message = failure(stderr.as_bytes());
        assert!(message.starts_with("git.index_locked: Git 索引被锁定"), "{message}");
        assert!(message.contains("不会删除锁文件"), "{message}");
        let (sentence, output) = message.split_once(DETAIL_LABEL).expect("Git output follows");
        assert!(!sentence.contains("fatal"), "{sentence}");
        assert!(output.starts_with("fatal: Unable to create"), "{output}");
        assert!(output.chars().count() <= DETAIL_CHARS + 1 && output.ends_with('…'), "{output}");
        assert!(!message.chars().any(char::is_control), "{message:?}");
    }

    #[test]
    fn git_output_never_carries_url_credentials() {
        let message = failure(b"fatal: unknown revision https://user:secret-token@example.com/x");
        assert!(message.starts_with("git.revision_missing: "), "{message}");
        assert!(!message.contains("secret-token"), "{message}");
        assert_eq!(detail(""), "（没有输出）");
    }
}
