//! The documented example (`docs/examples/hooks/protect-env`) runs as the owner will run it:
//! trusted from its folder, through Aura's managed PowerShell.
use super::support::Fixture;
use crate::hooks::HookCall;
use aura_hooks::HookEvent;
use serde_json::json;
use std::sync::atomic::AtomicBool;

fn managed_powershell() -> aura_backend_windows_user::compat::CompatBackend {
    let pin: serde_json::Value =
        serde_json::from_str(include_str!("../../../../../runtimes/powershell.json"))
            .expect("existing runtime pin");
    let managed = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/managed-runtimes")
        .join(pin["id"].as_str().expect("runtime id"));
    aura_backend_windows_user::compat::CompatBackend::with_environment(
        &managed.join("payload"),
        &managed.join("manifest.json"),
        vec![],
    )
    .expect("verify existing Shell runtime without owner environment or downloads")
}

fn copy_example(target: &std::path::Path) {
    let example = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/examples/hooks/protect-env");
    std::fs::create_dir_all(target).expect("package folder");
    for file in ["hook.toml", "protect-env.ps1"] {
        std::fs::copy(example.join(file), target.join(file)).expect("example file");
    }
}

#[test]
fn the_protect_env_example_blocks_env_writes_and_lets_others_through() {
    let f = Fixture::new();
    let package = f.root.path().join("hooks").join("protect-env");
    copy_example(&package);
    f.trust(&package);
    let powershell = managed_powershell();
    let run = |path: &str| {
        let call = HookCall {
            payload: json!({"tool": "fs.write", "arguments": {"path": path, "text": "x"}}),
            ..f.call(HookEvent::ToolBeforeCall, Some("fs.write"), json!({}))
        };
        f.runner.run(call, Some(&powershell), &AtomicBool::new(false)).expect("runs")
    };
    let blocked = run("config/.env");
    let block = blocked.block.expect("an .env write is blocked");
    assert_eq!(block.hook_name, "protect-env");
    assert!(block.reason.contains("不允许修改 .env"), "{}", block.reason);
    let other = run("src/main.rs");
    assert!(other.block.is_none(), "other files pass");
    let records = f.records();
    assert_eq!(records.len(), 2);
    assert!(records.iter().all(|r| r.failure.is_none()), "{records:?}");
}
