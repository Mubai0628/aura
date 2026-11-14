//! The desktop review page's comparison. The working tree is compared with HEAD (the
//! uncommitted changes, staged or not) or with the commit where HEAD left a branch (all the
//! changes of this work, committed or not), optionally ignoring whitespace. Untracked files are
//! listed by status and read one by one by `untracked_diff`. The page lists the files by
//! their counts alone, and a file's counts and lines come from one Git process.
use super::diff::{paginate, parse_numstat, split_numstat_patch};
use super::*;
use aura_workspace_domain::{GitDiffResultV1, GitReviewDiffInputV1};

/// The tree of a repository with no commit yet.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

impl GitReader {
    pub(super) fn review_diff(&self, input: &GitReviewDiffInputV1) -> Result<GitDiffResultV1> {
        super::diff::page_limit(input.max_bytes)?;
        let base = match &input.base {
            None => self.head()?.unwrap_or_else(|| EMPTY_TREE.into()),
            Some(branch) => {
                let head = self.head()?.ok_or_else(|| {
                    anyhow::anyhow!(
                        "git.no_commit: 这个仓库还没有任何提交，无法与分支比较。可以先查看未提交的改动，或提交一次后再比较。"
                    )
                })?;
                let other = self.oid(branch)?;
                let base = String::from_utf8(self.command(&["merge-base", &head, &other])?)
                    .map_err(|_| config::unreadable_output("merge_base_format", "共同提交"))?;
                let base = base.trim();
                ensure!(
                    !base.is_empty(),
                    "git.no_merge_base: 当前分支与「{}」没有共同的提交，无法比较。请换一个分支比较。",
                    config::shown(branch)
                );
                base.to_owned()
            }
        };
        let mut args = vec!["diff".to_owned(), base];
        if input.ignore_whitespace {
            args.push("--ignore-all-space".into());
        }
        // --no-textconv and --no-ext-diff predate 2.40; never dispatch repository diff programs.
        args.extend(
            ["--no-ext-diff", "--no-textconv", "--ignore-submodules=dirty", "--find-renames"]
                .map(str::to_string),
        );
        args.extend(["--numstat".into(), "-z".into()]);
        if !input.stats_only {
            args.push("--patch".into());
        }
        paths(&mut args, &input.paths)?;
        let output = self.command_owned(args)?;
        let (numstat, patch) = split_numstat_patch(&output)?;
        let mut files = parse_numstat(numstat)?;
        if input.ignore_whitespace {
            // A file whose only edits were whitespace still has a numstat row of zeros; a pure
            // rename keeps its row.
            files.retain(|file| {
                file.binary
                    || file.original_path.is_some()
                    || file.added != Some(0)
                    || file.deleted != Some(0)
            });
        }
        self.annotate_stats(&mut files)?;
        let patch = super::diff::patch_text(patch.to_vec())?;
        paginate(files, patch, input.offset_bytes, input.max_bytes)
    }

    /// HEAD's commit, or None in a repository with no commit yet.
    fn head(&self) -> Result<Option<String>> {
        let result = self.raw(&[
            "rev-parse".into(),
            "--verify".into(),
            "--quiet".into(),
            "--end-of-options".into(),
            "HEAD^{commit}".into(),
        ])?;
        if result.exit_code == Some(0) {
            let head = String::from_utf8(result.stdout)
                .map_err(|_| config::unreadable_output("head_format", "HEAD 提交号"))?;
            return Ok(Some(head.trim().to_owned()));
        }
        // An unborn HEAD answers 1 and says nothing; anything else is a real failure.
        ensure!(
            result.exit_code == Some(1) && result.stdout.is_empty(),
            "{}",
            config::failure(&result.stderr)
        );
        Ok(None)
    }
}
