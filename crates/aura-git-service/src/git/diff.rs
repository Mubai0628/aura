use super::*;
use aura_workspace_domain::{GitDiffInput, GitDiffResultV1, GitDiffTargetV1, GitFileStatV1};
impl GitReader {
    pub(super) fn diff(&self, input: &GitDiffInput) -> Result<GitDiffResultV1> {
        page_limit(input.max_bytes)?;
        let base = diff_arguments(self, &input.target)?;
        let mut stat = base.clone();
        stat.extend(["--numstat".into(), "-z".into()]);
        stat.push("--no-textconv".into());
        paths(&mut stat, &input.paths)?;
        let mut files = parse_numstat(&self.command_owned(stat)?)?;
        self.annotate_stats(&mut files)?;
        let mut patch = base;
        patch.push("--patch".into());
        paths(&mut patch, &input.paths)?;
        let patch = patch_text(self.command_owned(patch)?)?;
        paginate(files, patch, input.offset_bytes, input.max_bytes)
    }
}
/// A patch as text. File content in another encoding cannot be paged as text.
pub(super) fn patch_text(bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes).map_err(|_| {
        anyhow::anyhow!(
            "git.patch_not_utf8: 补丁里有不是 UTF-8 编码的内容（例如 GBK 编码的文件），Aura 无法按文本读出，这次读取没有得到结果。请只读其他文件，或在终端用 git diff 查看这些文件。"
        )
    })
}
/// The page size a diff read accepts.
pub(super) fn page_limit(max_bytes: u32) -> Result<()> {
    ensure!(
        max_bytes > 0 && max_bytes <= 65536,
        "git.diff_page_limit: max_bytes 要在 1 到 65536 之间，这次是 {max_bytes}。请改用这个范围内的值再读。"
    );
    Ok(())
}
pub(super) fn paginate(
    files: Vec<GitFileStatV1>,
    patch: String,
    offset: u64,
    max_bytes: u32,
) -> Result<GitDiffResultV1> {
    let start = usize::try_from(offset).unwrap_or(usize::MAX);
    ensure!(
        start <= patch.len() && patch.is_char_boundary(start),
        "git.diff_offset: offset_bytes（{offset}）不在这份补丁里，或落在一个字符的中间；补丁共 {} 字节。请用上一页结果里的 next_offset，或从 0 开始读。",
        patch.len()
    );
    let mut end = start.saturating_add(max_bytes as usize).min(patch.len());
    while !patch.is_char_boundary(end) {
        end -= 1;
    }
    ensure!(
        end > start || start == patch.len(),
        "git.diff_page_too_small: max_bytes（{max_bytes}）装不下 offset_bytes 处的一个字符，读不出内容。请把 max_bytes 调大（至少 4）再读。"
    );
    Ok(GitDiffResultV1 {
        warnings: if files.iter().any(|file| file.comparison_note.is_some()) {
            vec![super::attributes::RAW_COMPARISON.into()]
        } else {
            vec![]
        },
        files,
        patch: patch[start..end].into(),
        offset_bytes: start as u64,
        next_offset: (end < patch.len()).then_some(end as u64),
        total_bytes: patch.len() as u64,
        truncated: end < patch.len(),
    })
}
pub(super) fn diff_arguments(reader: &GitReader, target: &GitDiffTargetV1) -> Result<Vec<String>> {
    let mut args = vec!["diff".into()];
    match target {
        GitDiffTargetV1::Unstaged {} => {}
        GitDiffTargetV1::Staged {} => args.push("--cached".into()),
        GitDiffTargetV1::Range { from, to } => {
            args.extend([reader.oid(from)?, reader.oid(to)?]);
        }
        GitDiffTargetV1::Commit { commit } => {
            let oid = reader.oid(commit)?;
            let parents =
                String::from_utf8(reader.command(&["rev-list", "--parents", "-n", "1", &oid])?)
                    .map_err(|_| super::config::unreadable_output("parents_format", "父提交"))?;
            if let Some(parent) = parents.split_whitespace().nth(1) {
                args.extend([parent.into(), oid]);
            } else {
                args = vec![
                    "diff-tree".into(),
                    "--root".into(),
                    "--no-commit-id".into(),
                    "-r".into(),
                    oid,
                ];
            }
        }
    }
    // --no-textconv and --no-ext-diff predate 2.40; never dispatch repository diff programs.
    args.extend(
        ["--no-ext-diff", "--no-textconv", "--ignore-submodules=dirty", "--find-renames"]
            .map(str::to_string),
    );
    Ok(args)
}
/// `--numstat -z --patch` writes the counts as NUL-ended records — a renamed file's record is
/// `added\tdeleted\t` with the old and new names as two more — then one empty record, then the
/// patch. Without a patch there is no empty record.
pub(super) fn split_numstat_patch(bytes: &[u8]) -> Result<(&[u8], &[u8])> {
    let next = |at: usize| {
        bytes[at..]
            .iter()
            .position(|byte| *byte == 0)
            .map(|offset| at + offset)
            .ok_or_else(numstat_format)
    };
    let mut at = 0;
    while at < bytes.len() {
        let end = next(at)?;
        if end == at {
            return Ok((&bytes[..at], &bytes[at + 1..]));
        }
        let record = &bytes[at..end];
        at = end + 1;
        if record.ends_with(b"\t") && record.iter().filter(|byte| **byte == b'\t').count() == 2 {
            at = next(next(at)? + 1)? + 1;
        }
    }
    Ok((bytes, &[]))
}

pub(super) fn parse_numstat(bytes: &[u8]) -> Result<Vec<GitFileStatV1>> {
    let mut files = vec![];
    let mut fields = bytes.split(|byte| *byte == 0).filter(|field| !field.is_empty());
    while let Some(field) = fields.next() {
        let mut parts = std::str::from_utf8(field).map_err(|_| numstat_format())?.splitn(3, '\t');
        let added = parts.next().ok_or_else(numstat_format)?;
        let deleted = parts.next().ok_or_else(numstat_format)?;
        let path = parts.next().ok_or_else(numstat_format)?;
        let (path, original_path) = if path.is_empty() {
            let rename = || super::config::unreadable_output("numstat_rename", "改名记录");
            let old =
                std::str::from_utf8(fields.next().ok_or_else(rename)?).map_err(|_| rename())?;
            let new =
                std::str::from_utf8(fields.next().ok_or_else(rename)?).map_err(|_| rename())?;
            (new, Some(old.into()))
        } else {
            (path, None)
        };
        let count = |value: &str| -> Result<Option<u64>> {
            if value == "-" {
                Ok(None)
            } else {
                value.parse().map(Some).map_err(|_| numstat_format())
            }
        };
        files.push(GitFileStatV1 {
            comparison_note: None,
            path: path.into(),
            original_path,
            added: count(added)?,
            deleted: count(deleted)?,
            binary: added == "-" || deleted == "-",
        });
    }
    Ok(files)
}

fn numstat_format() -> anyhow::Error {
    super::config::unreadable_output("numstat_format", "改动统计（--numstat）")
}
