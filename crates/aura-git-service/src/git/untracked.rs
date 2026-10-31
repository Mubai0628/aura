//! Desktop-only raw comparison, under the same Low IL single-process reader.
use super::*;
use aura_workspace_domain::{GitDiffResultV1, GitFileStatV1, GitUntrackedDiffInputV1};

impl GitReader {
    pub(super) fn untracked_diff(
        &self,
        input: &GitUntrackedDiffInputV1,
    ) -> Result<GitDiffResultV1> {
        ensure!(input.max_bytes > 0 && input.max_bytes <= 65536, "git.diff_page_limit");
        let path = aura_domain_common::WorkspacePath::parse(&input.path)?;
        ensure!(!path.is_root(), "git.untracked_file_required: 请选择单个未跟踪文件");
        let path = path.to_string();
        let listed =
            self.command(&["ls-files", "--others", "--exclude-standard", "-z", "--", &path])?;
        ensure!(
            listed.split(|byte| *byte == 0).any(|name| name == path.as_bytes()),
            "git.not_untracked: 文件已不在未跟踪列表，请刷新后重试"
        );
        let metadata = std::fs::symlink_metadata(self.root.join(&path))?;
        ensure!(
            metadata.is_file() || metadata.file_type().is_symlink(),
            "git.untracked_file_required: 请选择单个未跟踪文件，目录可在终端查看"
        );
        let base =
            ["diff", "--no-index", "--no-textconv", "--no-ext-diff", "--ignore-submodules=dirty"];
        let empty = if cfg!(windows) { "NUL" } else { "/dev/null" };
        let output = |extra: &[&str]| -> Result<Vec<u8>> {
            let args = base
                .into_iter()
                .chain(extra.iter().copied())
                .chain(["--", empty, &path])
                .map(str::to_owned)
                .collect::<Vec<_>>();
            let result = self.raw(&args)?;
            ensure!(matches!(result.exit_code, Some(0 | 1)), "{}", config::failure(&result.stderr));
            Ok(result.stdout)
        };
        // The counts and the lines from one Git process.
        let output = output(&["--numstat", "-z", "--patch"])?;
        let (numstat, patch) = super::diff::split_numstat_patch(&output)?;
        let mut files = super::diff::parse_numstat(numstat)?;
        if files.is_empty() {
            files.push(GitFileStatV1 {
                path: path.clone(),
                original_path: None,
                added: Some(0),
                deleted: Some(0),
                binary: false,
                comparison_note: None,
            });
        }
        ensure!(files.len() == 1, "git.untracked_file_required");
        files[0].path = path.clone();
        files[0].original_path = None;
        self.annotate_stats(&mut files)?;
        let patch = String::from_utf8(patch.to_vec())?;
        super::diff::paginate(files, patch, input.offset_bytes, input.max_bytes)
    }
}
