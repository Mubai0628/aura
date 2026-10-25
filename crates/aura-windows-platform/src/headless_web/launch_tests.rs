//! Exercise native creation only: no browser, AppContainer, ACL or host preparation.
use super::*;

#[test]
fn headless_root_is_in_job_before_parent_can_fail() {
    let (workspace, profile) =
        RenderWorkspace::create(NetworkIsolation::None).expect("temporary headless workspace");
    assert!(profile.is_none(), "never create an AppContainer in this test");
    let directory = workspace.root.clone();
    let args = ["/d".to_owned(), "/c".to_owned(), "echo forbidden > ran".to_owned()];
    let (result, observations) = crate::job::creation_tests::observe_creation(|| {
        spawn_job_process(
            PathBuf::from(r"C:\Windows\System32\cmd.exe"),
            &args,
            directory.clone(),
            workspace,
            None,
            None,
        )
    });
    assert!(result.is_err(), "injected failure before adoption");
    assert_eq!(observations, [(true, true)], "headless root must be owned at creation");
    assert!(!directory.join("ran").exists());
}
