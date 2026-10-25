//! Native Git reads over the approved repository and config scope. No private snapshots.
mod config;
mod envelope;
pub(crate) use envelope::envelope;
mod cancellation;
pub(crate) use cancellation::{CancelOnDrop, GitCancelled, StopUnconfirmed};
mod attributes;
mod diff;
mod log;
mod remotes;
mod review;
pub(crate) mod runtime;
mod status;
mod untracked;
mod writes;
use anyhow::{Result, ensure};
use aura_backend_windows_user::git::{GitProcessResult, GitRunner};
use aura_workspace_domain::{GitReadInputV1, GitReadResultV1};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};
pub(crate) use writes::{GitWriteInterrupted, GitWriter};

pub(crate) struct GitReader {
    pub(super) root: PathBuf,
    pub(super) runner: GitRunner,
    pub(super) config: Vec<String>,
    programs: config::DisabledPrograms,
    directories: Vec<aura_windows_platform::GitReadDirectory>,
    /// The Git directory and the common one, as the layout named them; both are held above.
    git_dir: PathBuf,
    common_dir: PathBuf,
    pub(super) cancel: Arc<AtomicBool>,
    deadline: std::time::Instant,
    /// The Git processes this reader started.
    #[cfg(test)]
    processes: std::sync::atomic::AtomicUsize,
}
impl GitReader {
    pub fn open(root: &Path, cancel: Arc<AtomicBool>) -> Result<Self> {
        let detected = GitRunner::detect()?;
        let runner = detected.runner.ok_or_else(|| {
            anyhow::anyhow!(
                "git.{}: {}",
                detected.state,
                runtime::unavailable_message(detected.state)
            )
        })?;
        Self::with_runner(root, runner, cancel)
    }
    pub(super) fn with_runner(
        root: &Path,
        runner: GitRunner,
        cancel: Arc<AtomicBool>,
    ) -> Result<Self> {
        let mut value = Self {
            root: root.to_owned(),
            runner,
            config: vec![],
            programs: Default::default(),
            directories: vec![aura_windows_platform::GitReadDirectory::open(root)?],
            git_dir: PathBuf::new(),
            common_dir: PathBuf::new(),
            cancel,
            deadline: std::time::Instant::now() + Duration::from_secs(30),
            #[cfg(test)]
            processes: Default::default(),
        };
        value.verify_layout()?;
        (value.config, value.programs) = config::read_configuration(&value)?;
        Ok(value)
    }
    fn verify_layout(&mut self) -> Result<()> {
        // One rev-parse names both Git directories and the worktree (--path-format=absolute is in
        // 2.31; these were four processes). A bare repository has no worktree to label and
        // answers that with an error; the two questions below then follow.
        let directories =
            ["rev-parse", "--path-format=absolute", "--absolute-git-dir", "--git-common-dir"];
        let mut all = directories.map(str::to_owned).to_vec();
        all.push("--show-toplevel".into());
        let first = self.raw(&all)?;
        let output = if first.exit_code == Some(0) {
            String::from_utf8(first.stdout)?
        } else {
            let bare = self.command(&["rev-parse", "--is-bare-repository"])?;
            ensure!(bare == b"true\n", "{}", config::failure(&first.stderr));
            String::from_utf8(self.command(&directories)?)?
        };
        let mut paths = output.lines().map(PathBuf::from);
        let (Some(git_dir), Some(common_dir)) = (paths.next(), paths.next()) else {
            anyhow::bail!("git.layout_format: Git 没有说明仓库目录，请重试");
        };
        if let Some(worktree) = paths.next() {
            self.directories.push(aura_windows_platform::GitReadDirectory::open(&worktree)?);
        }
        for path in [&git_dir, &common_dir] {
            self.directories.push(aura_windows_platform::GitReadDirectory::open(path)?);
        }
        (self.git_dir, self.common_dir) = (git_dir, common_dir);
        Ok(())
    }
    pub fn read(&self, input: &GitReadInputV1) -> Result<GitReadResultV1> {
        Ok(match input {
            GitReadInputV1::Status(input) => GitReadResultV1::Status(self.status(input)?),
            GitReadInputV1::Diff(input) => GitReadResultV1::Diff(self.diff(input)?),
            GitReadInputV1::Log(input) => GitReadResultV1::Log(self.log(input)?),
            GitReadInputV1::UntrackedDiff(input) => {
                GitReadResultV1::Diff(self.untracked_diff(input)?)
            }
            GitReadInputV1::ReviewDiff(input) => GitReadResultV1::Diff(self.review_diff(input)?),
        })
    }
    pub fn repository_identity(&self) -> Result<aura_windows_platform::NativePathInfo> {
        // The common Git directory the layout named, held since.
        Ok(aura_windows_platform::inspect_existing_path(&self.common_dir, false)?)
    }
    pub fn facts(&self) -> Result<serde_json::Value> {
        let identity = self.repository_identity()?.identity;
        Ok(
            serde_json::json!({"repository":{"volume":identity.volume_serial_number.to_string(),"file_id":identity.file_id},
            "image":self.runner.image_binding(), "config":aura_domain_common::jcs::canonical_sha256(&self.config)?}),
        )
    }
    fn raw(&self, args: &[String]) -> Result<GitProcessResult> {
        self.execute(args, &self.config)
    }
    fn execute(&self, args: &[String], configuration: &[String]) -> Result<GitProcessResult> {
        for directory in &self.directories {
            directory.recheck()?;
        }
        // All global options below predate 2.40. Command data is always a separate argv element.
        let mut command =
            vec!["--no-pager".into(), "--no-optional-locks".into(), "--literal-pathspecs".into()];
        command.extend_from_slice(configuration);
        for value in [
            "color.ui=false",
            "core.quotePath=false",
            "core.fsmonitor=false",
            "protocol.allow=never",
            "submodule.recurse=false",
            "i18n.logOutputEncoding=UTF-8",
        ] {
            command.extend(["-c".into(), value.into()]);
        }
        command.extend_from_slice(args);
        let remaining = self.deadline.saturating_duration_since(std::time::Instant::now());
        ensure!(!remaining.is_zero(), "git.timed_out: Git读取超过时限");
        #[cfg(test)]
        self.processes.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let result =
            self.runner.run(&self.root, &command, &self.cancel, remaining, 32 * 1024 * 1024)?;
        if !result.stopped {
            return Err(StopUnconfirmed.into());
        }
        if result.reason == "cancelled" {
            return Err(GitCancelled.into());
        }
        ensure!(result.reason == "exited", "git.{}: Git已停止，读取未完成", result.reason);
        Ok(result)
    }
    // Read only by the Git tests, which build with the contract driver (see `mod tests`).
    #[cfg(all(test, feature = "p08-contract-driver"))]
    pub(super) fn processes(&self) -> usize {
        self.processes.load(std::sync::atomic::Ordering::Relaxed)
    }
    fn command(&self, args: &[&str]) -> Result<Vec<u8>> {
        self.command_owned(args.iter().map(|arg| arg.to_string()).collect())
    }
    fn command_owned(&self, args: Vec<String>) -> Result<Vec<u8>> {
        let result = self.raw(&args)?;
        if result.exit_code != Some(0) {
            anyhow::bail!("{}", config::failure(&result.stderr));
        }
        Ok(result.stdout)
    }
    fn oid(&self, revision: &str) -> Result<String> {
        ensure!(
            !revision.is_empty()
                && revision.len() <= 1024
                && !revision.starts_with('-')
                && !revision.chars().any(char::is_control),
            "git.invalid_revision"
        );
        // rev-parse --end-of-options is available since Git 2.20; validated OIDs are used afterwards.
        let oid = String::from_utf8(self.command(&[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{revision}^{{commit}}"),
        ])?)?;
        let oid = oid.trim();
        ensure!(
            (oid.len() == 40 || oid.len() == 64)
                && oid.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "git.invalid_object_id"
        );
        Ok(oid.into())
    }
}

fn paths(args: &mut Vec<String>, paths: &[String]) -> Result<()> {
    ensure!(paths.len() <= 200, "git.too_many_paths");
    args.push("--".into());
    for path in paths {
        let path = aura_domain_common::WorkspacePath::parse(path)?;
        args.push(if path.is_root() { ".".into() } else { path.to_string() });
    }
    Ok(())
}
#[cfg(all(test, feature = "p08-contract-driver"))]
mod tests;
