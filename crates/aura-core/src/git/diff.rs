use super::*;
use aura_workspace_domain::{GitDiffInput, GitDiffResultV1, GitDiffTargetV1, GitFileStatV1};
impl GitReader {
    pub(super) fn diff(&self, input: &GitDiffInput) -> Result<GitDiffResultV1> {
        ensure!(input.max_bytes > 0 && input.max_bytes <= 65536, "git.diff_page_limit");
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
        let patch = String::from_utf8(self.command_owned(patch)?)?;
        paginate(files, patch, input.offset_bytes, input.max_bytes)
    }
}
pub(super) fn paginate(
    files: Vec<GitFileStatV1>,
    patch: String,
    offset: u64,
    max_bytes: u32,
) -> Result<GitDiffResultV1> {
    let start = usize::try_from(offset)?;
    ensure!(
        start <= patch.len() && patch.is_char_boundary(start),
        "git.diff_offset: 偏移必须位于UTF-8字符边界"
    );
    let mut end = start.saturating_add(max_bytes as usize).min(patch.len());
    while !patch.is_char_boundary(end) {
        end -= 1;
    }
    ensure!(end > start || start == patch.len(), "git.diff_page_too_small");
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
                String::from_utf8(reader.command(&["rev-list", "--parents", "-n", "1", &oid])?)?;
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
            .ok_or_else(|| anyhow::anyhow!("git.numstat_format"))
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
        let mut parts = std::str::from_utf8(field)?.splitn(3, '\t');
        let added = parts.next().ok_or_else(|| anyhow::anyhow!("git.numstat_format"))?;
        let deleted = parts.next().ok_or_else(|| anyhow::anyhow!("git.numstat_format"))?;
        let path = parts.next().ok_or_else(|| anyhow::anyhow!("git.numstat_format"))?;
        let (path, original_path) = if path.is_empty() {
            let old = std::str::from_utf8(
                fields.next().ok_or_else(|| anyhow::anyhow!("git.numstat_rename"))?,
            )?;
            let new = std::str::from_utf8(
                fields.next().ok_or_else(|| anyhow::anyhow!("git.numstat_rename"))?,
            )?;
            (new, Some(old.into()))
        } else {
            (path, None)
        };
        files.push(GitFileStatV1 {
            comparison_note: None,
            path: path.into(),
            original_path,
            added: if added == "-" { None } else { Some(added.parse()?) },
            deleted: if deleted == "-" { None } else { Some(deleted.parse()?) },
            binary: added == "-" || deleted == "-",
        });
    }
    Ok(files)
}
