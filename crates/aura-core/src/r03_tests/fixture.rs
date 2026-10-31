use super::*;

pub(crate) struct Fixture {
    pub(crate) root: TempDir,
    pub(crate) paths: AuraPaths,
    options: CoreOptions,
    release: CoreReleaseContext,
    starts: std::sync::atomic::AtomicUsize,
}

impl Fixture {
    pub(crate) fn new() -> Self {
        Self::with_backend(None)
    }

    pub(super) fn with_backend(image: Option<PathBuf>) -> Self {
        let root = tempfile::tempdir().expect("R03 isolated fixture operation must succeed");
        let manifest_path = root.path().join("manifest.json");
        let mut manifest: Value = serde_json::from_slice(include_bytes!(
            "../../../../resources/component-manifest.example.json"
        ))
        .expect("R03 isolated fixture operation must succeed");
        let hash = format!(
            "{:x}",
            Sha256::digest(
                fs::read(env::current_exe().expect("R03 isolated fixture operation must succeed"))
                    .expect("R03 isolated fixture operation must succeed")
            )
        );
        for component in manifest["components"]
            .as_array_mut()
            .expect("R03 isolated fixture operation must succeed")
        {
            if component["kind"] == "core" {
                component["content_hash"]["hex"] = json!(hash);
            }
            if component["kind"] == "sandbox_host"
                && let Some(image) = &image
            {
                let target = root.path().join("aura-sandbox-host.exe");
                fs::copy(image, &target).expect("R03 isolated fixture operation must succeed");
                component["relative_path"] = json!("aura-sandbox-host.exe");
                component["content_hash"]["hex"] = json!(format!(
                    "{:x}",
                    Sha256::digest(
                        fs::read(target).expect("R03 isolated fixture operation must succeed")
                    )
                ));
            }
        }
        fs::write(
            &manifest_path,
            serde_json::to_vec(&manifest).expect("R03 isolated fixture operation must succeed"),
        )
        .expect("R03 isolated fixture operation must succeed");
        let options = CoreOptions {
            pipe_name: "r03-isolated-tests".into(),
            bootstrap_stdin: true,
            parent_process_id: process::id(),
            runtime_instance_id: RuntimeInstanceId::new(),
            component_manifest_path: manifest_path,
            allow_dev_component_mismatch: false,
            safe_mode: false,
            test_mode: CoreTestMode::Normal,
            continue_after: None,
        };
        // Optional executables deliberately do not exist in this release directory.
        let release =
            load_release_identity(&options).expect("R03 isolated fixture operation must succeed");
        let paths = AuraPaths::from_roots(root.path().join("config"), root.path().join("local"));
        Self { root, paths, options, release, starts: std::sync::atomic::AtomicUsize::new(0) }
    }

    /// A fixture whose Release Bundle uses the layout the Host Preparation planner
    /// validates before it freezes a plan: `<install>/versions/<version>/` with
    /// `component-manifest.json`, `<install>/active.json`, and a real Elevated
    /// Helper file at the path the manifest names.
    ///
    /// [`Fixture::new`] keeps its flat layout; this one exists so a test can drive
    /// the real planner instead of a stand-in.
    pub(crate) fn with_active_release_layout() -> Self {
        let root = tempfile::tempdir().expect("R03 isolated fixture operation must succeed");
        let version = "1.0.0";
        let version_directory = root.path().join("versions").join(version);
        fs::create_dir_all(&version_directory)
            .expect("R03 isolated fixture operation must succeed");
        let executable = env::current_exe().expect("R03 isolated fixture operation must succeed");
        let hash = format!(
            "{:x}",
            Sha256::digest(
                fs::read(&executable).expect("R03 isolated fixture operation must succeed")
            )
        );
        let helper_name = "aura-elevated-helper.exe";
        fs::copy(&executable, version_directory.join(helper_name))
            .expect("R03 isolated fixture operation must succeed");
        let mut manifest: Value = serde_json::from_slice(include_bytes!(
            "../../../../resources/component-manifest.example.json"
        ))
        .expect("R03 isolated fixture operation must succeed");
        for component in manifest["components"]
            .as_array_mut()
            .expect("R03 isolated fixture operation must succeed")
        {
            match component["kind"].as_str() {
                // The running test executable stands in for both components, so
                // every hash the planner re-derives matches a real file.
                Some("core") => component["content_hash"]["hex"] = json!(hash),
                Some("elevated_helper") => {
                    component["relative_path"] = json!(helper_name);
                    component["content_hash"]["hex"] = json!(hash);
                }
                _ => {}
            }
        }
        let manifest_path = version_directory.join("component-manifest.json");
        fs::write(
            &manifest_path,
            serde_json::to_vec(&manifest).expect("R03 isolated fixture operation must succeed"),
        )
        .expect("R03 isolated fixture operation must succeed");
        let options = CoreOptions {
            pipe_name: "r03-isolated-tests".into(),
            bootstrap_stdin: true,
            parent_process_id: process::id(),
            runtime_instance_id: RuntimeInstanceId::new(),
            component_manifest_path: manifest_path,
            allow_dev_component_mismatch: false,
            safe_mode: false,
            test_mode: CoreTestMode::Normal,
            continue_after: None,
        };
        let release =
            load_release_identity(&options).expect("R03 isolated fixture operation must succeed");
        let active = aura_runtime_contract::ActiveInstallState {
            active: version.to_owned(),
            release_descriptor: release.release_descriptor.clone(),
            previous: None,
            pending: None,
            last_known_good: None,
        };
        fs::write(
            root.path().join("active.json"),
            serde_json::to_vec(&active).expect("R03 isolated fixture operation must succeed"),
        )
        .expect("R03 isolated fixture operation must succeed");
        let paths = AuraPaths::from_roots(root.path().join("config"), root.path().join("local"));
        Self { root, paths, options, release, starts: std::sync::atomic::AtomicUsize::new(0) }
    }

    pub(crate) fn renew_runtime_instance(&mut self) {
        self.options.runtime_instance_id = RuntimeInstanceId::new();
    }

    pub(super) fn start_count(&self) -> usize {
        self.starts.load(Ordering::SeqCst)
    }

    pub(crate) async fn start(&self) -> RuntimeState {
        self.starts.fetch_add(1, Ordering::SeqCst);
        let startup = classify_startup(
            initialize_services_at(&self.options, &self.release, self.paths.clone()).await,
        );
        let state = new_runtime_state(
            self.options.clone(),
            self.release.clone(),
            retain_process(process::id()).expect("R03 isolated fixture operation must succeed"),
            Zeroizing::new([0; 32]),
            startup,
        );
        // This fixture starts after channel authentication, not through an E2E route.
        let mut channel = state
            .authenticated_channel
            .lock()
            .expect("R03 isolated fixture operation must succeed");
        channel.active_channel_id = Some(IpcChannelId::new());
        channel.peer_handshake = Some(RuntimeHandshake {
            protocol_version: ContractVersion::CURRENT,
            process_kind: RuntimeProcessKind::ElectronMain,
            component_version: self.release.component_version.clone(),
            build_commit: self.release.build_commit.clone(),
            release_descriptor: self.release.release_descriptor.clone(),
            runtime_instance_id: RuntimeInstanceId::new(),
            parent_runtime_instance_id: None,
            supported_features: vec![],
        });
        drop(channel);
        state.ready.store(true, Ordering::Release);
        state
    }
}
