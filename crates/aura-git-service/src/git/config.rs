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
        let (key, value) = std::str::from_utf8(pair[1])?
            .split_once('\n')
            .ok_or_else(|| anyhow::anyhow!("git.config_format"))?;
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
            "仓库所有者与当前用户不符",
            "请在终端核对仓库所有权与 safe.directory 配置",
        )
    } else if lower.contains("not a git repository") {
        ("not_repository", "此工作区不是 Git 仓库", "请选择已有仓库或在终端初始化仓库")
    } else if lower.contains("bad config")
        || lower.contains("bad boolean")
        || lower.contains("unable to read config")
    {
        ("config_invalid", "Git 配置无法读取", "请根据详情修复配置后重新检测")
    } else if lower.contains("index.lock") {
        ("index_locked", "Git 索引被锁定", "请核对其他 Git 操作；Aura 不会删除锁文件")
    } else if lower.contains("unknown revision")
        || lower.contains("needed a single revision")
        || lower.contains("bad revision")
    {
        ("revision_missing", "指定的提交或范围不存在", "请核对提交、分支或标签")
    } else {
        ("read_failed", "Git 读取失败", "请查看详情并在修复后重试")
    };
    format!(
        "git.{code}: {message}。{next}。{}",
        redact(&text).chars().take(8192).collect::<String>()
    )
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
