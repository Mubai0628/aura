use super::super::*;
pub(super) struct Fixture {
    pub reader: GitReader,
    pub root: tempfile::TempDir,
}
impl Fixture {
    pub fn new(global_extra: &str) -> Self {
        let root = tempfile::Builder::new().prefix("aura-git-test-").tempdir().expect("fixture");
        let global = root.path().join("global.cfg");
        let system = root.path().join("system.cfg");
        std::fs::write(&global, format!("[credential]\nhelper =\n[user]\nname = Fixture\nemail = fixture@example.invalid\n[commit]\ngpgsign = false\n{global_extra}")).expect("global");
        std::fs::write(&system, "").expect("system");
        let profile = root.path().join("profile.json");
        std::fs::write(
            &profile,
            serde_json::to_vec(&serde_json::json!({"global_config":global,"system_config":system}))
                .expect("JSON"),
        )
        .expect("profile");
        let git = std::env::var_os("AURA_GIT_TEST_BINARY")
            .unwrap_or_else(|| "C:/Program Files/Git/cmd/git.exe".into());
        let runner = GitRunner::isolated(Path::new(&git), &profile).expect("isolated Git");
        let value = runner
            .run_native(
                root.path(),
                &["init".into(), "--initial-branch=main".into()],
                &AtomicBool::new(false),
                Duration::from_secs(5),
                65536,
            )
            .expect("init");
        assert_eq!(value.exit_code, Some(0));
        let reader = GitReader::with_runner(root.path(), runner, Arc::new(AtomicBool::new(false)))
            .expect("reader");
        Self { root, reader }
    }
    /// The repository opened again, as the next request opens it.
    pub fn reopen(&self) -> GitReader {
        let git = std::env::var_os("AURA_GIT_TEST_BINARY")
            .unwrap_or_else(|| "C:/Program Files/Git/cmd/git.exe".into());
        let runner = GitRunner::isolated(Path::new(&git), &self.root.path().join("profile.json"))
            .expect("isolated Git");
        GitReader::with_runner(self.root.path(), runner, Arc::new(AtomicBool::new(false)))
            .expect("reader")
    }
    pub fn git(&self, args: &[&str]) -> Vec<u8> {
        let out = self
            .reader
            .runner
            .run_native(
                self.root.path(),
                &std::iter::once("-c".to_owned())
                    .chain(std::iter::once("core.fsmonitor=false".to_owned()))
                    .chain(args.iter().map(|s| s.to_string()))
                    .collect::<Vec<_>>(),
                &AtomicBool::new(false),
                Duration::from_secs(10),
                1024 * 1024,
            )
            .expect("fixture setup argv");
        assert_eq!(
            out.exit_code,
            Some(0),
            "temporary Git setup stderr={:?}",
            String::from_utf8_lossy(&out.stderr)
        );
        out.stdout
    }
    pub fn file(&self, path: &str, bytes: &[u8]) {
        std::fs::write(self.root.path().join(path), bytes).expect("fixture file");
    }
    pub fn commit(&self, message: &str) -> String {
        self.git(&["add", "--all"]);
        self.git(&["commit", "-m", message]);
        String::from_utf8(self.git(&["rev-parse", "HEAD"])).expect("oid").trim().into()
    }
    pub fn refresh(&mut self) {
        (self.reader.config, self.reader.programs) =
            super::super::config::read_configuration(&self.reader).expect("refresh config");
    }
}
