use super::*;
use aura_workspace_domain::{GitBranchV1, GitChangeV1, GitStatusInput, GitStatusResultV1};
impl GitReader {
    pub(super) fn status(&self, input: &GitStatusInput) -> Result<GitStatusResultV1> {
        ensure!(
            input.limit > 0 && input.limit <= 2000,
            "git.status_limit: limit 要在 1 到 2000 之间，这次是 {}。请改用这个范围内的值再读。",
            input.limit
        );
        let output = self.status_output(input.include_untracked)?;
        let mut result = parse_status(&output)?;
        // The Git directory the layout named.
        let git_dir = &self.git_dir;
        result.operation = [
            ("rebase-merge", "rebase"),
            ("rebase-apply", "rebase"),
            ("MERGE_HEAD", "merge"),
            ("CHERRY_PICK_HEAD", "cherry_pick"),
            ("REVERT_HEAD", "revert"),
            ("BISECT_LOG", "bisect"),
        ]
        .into_iter()
        .find(|(path, _)| git_dir.join(path).exists())
        .map(|(_, name)| name.into());
        if input.include_branches {
            // for-each-ref --format supports NUL escapes on every supported Git.
            let branches = self.command(&[
                "for-each-ref",
                "--format=%(refname)%00%(objectname)%00",
                "refs/heads",
                "refs/remotes",
            ])?;
            let fields = branches.split(|byte| *byte == 0).collect::<Vec<_>>();
            let text = |field: &[u8]| -> Result<String> {
                std::str::from_utf8(field)
                    .map(str::to_owned)
                    .map_err(|_| super::config::unreadable_output("branch_format", "分支列表"))
            };
            for pair in fields.chunks_exact(2) {
                let name = text(pair[0])?;
                let name = name.trim_start_matches('\n');
                if name.is_empty() {
                    continue;
                }
                result.branches.push(GitBranchV1 {
                    name: name.into(),
                    commit: text(pair[1])?,
                    remote: name.starts_with("refs/remotes/"),
                });
            }
        }
        result.total_entries = result.entries.len().try_into()?;
        let offset = input.offset as usize;
        let end = (offset.saturating_add(input.limit as usize)).min(result.entries.len());
        ensure!(
            offset <= result.entries.len(),
            "git.status_offset: offset（{offset}）超过了改动总数（{}）。请用上一页结果里的 next_offset，或从 0 开始读。",
            result.entries.len()
        );
        result.next_offset = (end < result.entries.len()).then_some(end as u32);
        result.truncated = result.next_offset.is_some();
        result.entries = result.entries.drain(offset..end).collect();
        self.annotate_changes(&mut result.entries)?;
        Ok(result)
    }

    pub(super) fn status_output(&self, include_untracked: bool) -> Result<Vec<u8>> {
        // porcelain v2 (-z branch headers) predates Git 2.40. Do not enter submodules.
        self.command(&[
            "status",
            "--porcelain=v2",
            "--branch",
            "--ahead-behind",
            "-z",
            "--ignore-submodules=dirty",
            if include_untracked { "--untracked-files=all" } else { "--untracked-files=no" },
        ])
    }
}
pub(super) fn parse_status(bytes: &[u8]) -> Result<GitStatusResultV1> {
    let mut result = GitStatusResultV1 {
        branch: None,
        head: None,
        detached: false,
        upstream: None,
        ahead: 0,
        behind: 0,
        operation: None,
        conflicts: vec![],
        entries: vec![],
        branches: vec![],
        total_entries: 0,
        next_offset: None,
        truncated: false,
        submodule_contents_checked: false,
    };
    let status_format = || super::config::unreadable_output("status_format", "状态");
    let branch_format = || super::config::unreadable_output("branch_format", "分支领先/落后计数");
    let mut records = bytes.split(|byte| *byte == 0).filter(|record| !record.is_empty());
    while let Some(record) = records.next() {
        let record = std::str::from_utf8(record).map_err(|_| status_format())?;
        if let Some(oid) = record.strip_prefix("# branch.oid ") {
            result.head = (oid != "(initial)").then(|| oid.into());
        } else if let Some(branch) = record.strip_prefix("# branch.head ") {
            result.detached = branch == "(detached)";
            result.branch = (!result.detached).then(|| branch.into());
        } else if let Some(upstream) = record.strip_prefix("# branch.upstream ") {
            result.upstream = Some(upstream.into());
        } else if let Some(counts) = record.strip_prefix("# branch.ab ") {
            let (ahead, behind) = counts.split_once(' ').ok_or_else(branch_format)?;
            result.ahead = ahead.trim_start_matches('+').parse().map_err(|_| branch_format())?;
            result.behind = behind.trim_start_matches('-').parse().map_err(|_| branch_format())?;
        } else if record.starts_with(['1', '2', 'u']) {
            let fields = record
                .splitn(
                    if record.starts_with('2') {
                        10
                    } else if record.starts_with('u') {
                        11
                    } else {
                        9
                    },
                    ' ',
                )
                .collect::<Vec<_>>();
            let path = fields.last().ok_or_else(status_format)?;
            let xy = fields.get(1).filter(|xy| xy.len() == 2).ok_or_else(status_format)?;
            let conflict = record.starts_with('u');
            if conflict {
                result.conflicts.push(path.to_string());
            }
            let original_path = if record.starts_with('2') {
                let rename = || super::config::unreadable_output("rename_format", "改名记录");
                Some(
                    std::str::from_utf8(records.next().ok_or_else(rename)?)
                        .map_err(|_| rename())?
                        .into(),
                )
            } else {
                None
            };
            result.entries.push(GitChangeV1 {
                comparison_note: None,
                path: path.to_string(),
                original_path,
                index_status: xy[..1].into(),
                worktree_status: xy[1..].into(),
                conflict,
                untracked: false,
                submodule: fields.get(2).is_some_and(|sub| sub.starts_with('S')),
            });
        } else if let Some(path) = record.strip_prefix("? ") {
            result.entries.push(GitChangeV1 {
                comparison_note: None,
                path: path.into(),
                original_path: None,
                index_status: "?".into(),
                worktree_status: "?".into(),
                conflict: false,
                untracked: true,
                submodule: false,
            });
        }
    }
    Ok(result)
}
