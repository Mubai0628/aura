use crate::package_program::ProgramContext;
use aura_tool_contract::{TOOL_PACKAGE_MANIFEST_NAME, ToolPackageFileSha256V1};
use aura_tool_package::{StagedToolPackageV1, stage_directory_package_v1};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, time::Duration};

#[derive(Clone, Copy)]
pub enum Kind {
    Command,
    Powershell,
    Stdio,
    Http,
}
pub struct Fixture {
    pub root: tempfile::TempDir,
    pub staged: StagedToolPackageV1,
    pub powershell: Option<aura_backend_windows_user::compat::CompatBackend>,
}
impl Fixture {
    pub fn new(kind: Kind, edit: impl FnOnce(&mut Value)) -> Self {
        let root = tempfile::tempdir().expect("only temporary package content");
        for directory in ["source/bin", "source/scripts", "staging", "calls", "workspace"] {
            fs::create_dir_all(root.path().join(directory)).expect("owned directory");
        }
        let mut raw: Value = serde_json::from_slice(include_bytes!(
            "../../../../../fixtures/tool-package/manifest-v1/valid/aura-stdio.json"
        ))
        .expect("manifest fixture");
        let debug = std::env::current_exe()
            .expect("test image")
            .parent()
            .expect("deps")
            .parent()
            .expect("debug")
            .to_owned();
        let (path, role, bytes) = match kind {
            Kind::Powershell => {
                raw["tools"][0]["runtime"] = json!({"type":"declarative_command","command":{"type":"powershell_script","script":"scripts/fixture.ps1","arguments":[],"environment":[],"working_directory":{"type":"package_root"},"credential_slot":null,"success_exit_codes":[0]}});
                (
                    "scripts/fixture.ps1",
                    "runtime_script",
                    b"[System.Console]::Write([System.Environment]::CurrentDirectory)\n".to_vec(),
                )
            }
            Kind::Command => {
                raw["tools"][0]["runtime"] = json!({"type":"declarative_command","command":{"type":"argv","executable":"bin/fixture.exe","arguments":[],"environment":[],"working_directory":{"type":"package_root"},"credential_slot":null,"success_exit_codes":[0]}});
                (
                    "bin/fixture.exe",
                    "runtime_executable",
                    fs::read(debug.join("aura-command-fixture.exe"))
                        .expect("build current aura-tool-fixtures --bins first"),
                )
            }
            Kind::Stdio => {
                raw["tools"][0]["capability_requirements"] = json!(["process_execute"]);
                raw["tools"][0]["resource_claims"] = json!([{"type":"artifact","claim_key":"output","selector":{"type":"attempt_staging","access":"create"}}]);
                (
                    "bin/fixture.exe",
                    "runtime_executable",
                    fs::read(debug.join("aura-stdio-fixture.exe"))
                        .expect("build current aura-tool-fixtures --bins first"),
                )
            }
            Kind::Http => {
                raw = serde_json::from_slice(include_bytes!(
                    "../../../../../fixtures/tool-package/manifest-v1/valid/declarative-http.json"
                ))
                .expect("HTTP manifest");
                (
                    "bin/fixture.exe",
                    "runtime_resource",
                    b"synthetic HTTP resource never executed".to_vec(),
                )
            }
        };
        raw["files"] = json!([{"path":path,"role":role,"byte_length":bytes.len(),"sha256":ToolPackageFileSha256V1::sha256(&bytes)}]);
        if !matches!(kind, Kind::Http) {
            raw["tools"][0]["model"]["result_contract"] = json!({"type":"utf8_text"});
        }
        edit(&mut raw);
        fs::write(root.path().join("source").join(path), bytes).expect("owned payload");
        fs::write(
            root.path().join("source").join(TOOL_PACKAGE_MANIFEST_NAME),
            serde_json::to_vec(&raw).expect("JSON"),
        )
        .expect("owned manifest");
        let staged =
            stage_directory_package_v1(&root.path().join("source"), &root.path().join("staging"))
                .expect("real strict stager");
        let powershell = if matches!(kind, Kind::Powershell) {
            let pin: Value =
                serde_json::from_str(include_str!("../../../../../runtimes/powershell.json"))
                    .expect("existing runtime pin");
            let managed = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/managed-runtimes")
                .join(pin["id"].as_str().expect("runtime id"));
            Some(
                aura_backend_windows_user::compat::CompatBackend::with_environment(
                    &managed.join("payload"),
                    &managed.join("manifest.json"),
                    vec![],
                )
                .expect("verify existing Shell runtime without owner environment or downloads"),
            )
        } else {
            None
        };
        Self { root, staged, powershell }
    }
    pub fn context(&self) -> ProgramContext<'_> {
        let system = aura_windows_platform::windows_directory().expect("system path");
        ProgramContext {
            identity: crate::package_program::ProgramInvocationIdentity {
                tool_call_id: aura_domain_common::ToolCallId::new(),
                tool_execution_attempt_id: aura_domain_common::ToolExecutionAttemptId::new(),
            },
            workspace_root: Some(self.root.path()),
            directory_claims: BTreeMap::new(),
            instance_configuration: None,
            powershell: self.powershell.as_ref(),
            user_environment: vec![
                ("SystemRoot".into(), system.as_os_str().to_owned()),
                ("WINDIR".into(), system.as_os_str().to_owned()),
                ("PATH".into(), system.join("System32").into_os_string()),
                ("USERPROFILE".into(), self.root.path().as_os_str().to_owned()),
                ("HOME".into(), self.root.path().as_os_str().to_owned()),
                ("APPDATA".into(), self.root.path().as_os_str().to_owned()),
                ("LOCALAPPDATA".into(), self.root.path().as_os_str().to_owned()),
            ],
            scratch_parent: self.root.path(),
            timeout: Duration::from_secs(5),
            max_output_bytes: 1024 * 1024,
        }
    }
    pub fn assert_scratch_clean(&self) {
        assert!(
            fs::read_dir(self.root.path()).expect("root").all(|entry| !entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with("package-call-")),
            "every call root must be removed"
        );
    }
}
