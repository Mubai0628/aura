use super::{super::*, support::Fixture};
use aura_workspace_domain::GitStatusInput;

struct RestoreLabel(PathBuf);
impl Drop for RestoreLabel {
    fn drop(&mut self) {
        aura_windows_platform::set_git_read_fixture_integrity(&self.0, false)
            .expect("restore only our temporary fixture label");
    }
}

#[test]
fn low_integrity_worktree_git_dir_and_common_dir_are_refused_and_labels_rechecked() {
    for kind in ["worktree", "git-dir", "common-dir"] {
        let f = Fixture::new("");
        f.file("tracked", b"base\n");
        f.commit("base");
        let linked = f.root.path().join("linked");
        f.git(&["worktree", "add", "-b", "linked", linked.to_str().expect("path")]);
        let label_target = match kind {
            "worktree" => linked.clone(),
            "git-dir" => f.root.path().join(".git/worktrees/linked"),
            _ => f.root.path().join(".git"),
        };
        let runner = || {
            GitRunner::isolated(
                Path::new(&f.reader.runner.image_binding().path),
                &f.root.path().join("profile.json"),
            )
            .expect("isolated Git")
        };
        let reader = GitReader::with_runner(&linked, runner(), Arc::new(AtomicBool::new(false)))
            .expect("Medium repository accepted");
        aura_windows_platform::set_git_read_fixture_integrity(&label_target, true)
            .expect("label only temporary fixture Low");
        let _restore = RestoreLabel(label_target);
        let error = GitReader::with_runner(&linked, runner(), Arc::new(AtomicBool::new(false)))
            .err()
            .expect("Low repository rejected");
        assert!(error.to_string().contains("git.low_integrity_repository"), "{kind}: {error:#}");
        let error = reader
            .status(&GitStatusInput::default())
            .expect_err("label rechecked before every command");
        assert!(error.to_string().contains("git.low_integrity_repository"), "{kind}: {error:#}");
    }
}
