use super::ControlResources;
use aura_core_base::release::{CoreReleaseContext, SidecarReleaseIdentity};
use aura_domain_common::{ContentHash, ContentHashAlgorithm, ReleaseBundleId};
use aura_runtime_contract::{ReleaseDescriptor, ReleaseIdentity, RuntimeProcessKind};
use std::path::{Path, PathBuf};

fn release(root: &Path) -> CoreReleaseContext {
    let hash = ContentHash::new(ContentHashAlgorithm::Sha256, "0".repeat(64)).expect("hash");
    let sidecar = |process_kind, name: &str| SidecarReleaseIdentity {
        process_kind,
        component_version: "0.0.0".into(),
        executable: root.join(name),
        content_hash: hash.clone(),
        protocol_version: None,
    };
    CoreReleaseContext {
        release_descriptor: ReleaseDescriptor {
            bundle_namespace: "aura.desktop.test.control-resources".parse().expect("namespace"),
            identity: ReleaseIdentity {
                release_bundle_id: ReleaseBundleId::new(),
                component_manifest_hash: hash.clone(),
            },
        },
        component_version: "0.0.0".into(),
        build_commit: "test".into(),
        manifest_path: root.join("component-manifest.json"),
        elevated_helper_executable: root.join("aura-elevated-helper.exe"),
        elevated_helper_hash: hash.clone(),
        sandbox_host: sidecar(RuntimeProcessKind::SandboxHost, "aura-sandbox-host.exe"),
        egress_proxy: sidecar(RuntimeProcessKind::EgressProxy, "aura-egress-proxy.exe"),
    }
}

/// The data moved to `D\Aura`: the data, its logs, the window profile (the built-in browser's
/// sign-ins) and the settings ask every time a model reaches in; global memory beside them does
/// not.
#[test]
fn a_moved_installations_data_logs_and_window_profile_ask_but_global_memory_does_not() {
    let temp = tempfile::tempdir().expect("TEMP");
    let moved = temp.path().join("D").join("Aura");
    let settings = temp.path().join("Roaming").join("Aura").join("namespaces").join("h");
    let mut paths = aura_config::AuraPaths::from_roots(settings.clone(), moved.join("data"));
    paths.locations.ui_profile = moved.join("ui-profile");
    paths.locations.core_log = moved.join("logs").join("core.jsonl");
    paths.locations.location = Some(moved.clone());
    paths.locations.global_memory = Some(moved.join("memories"));
    for folder in ["data", "ui-profile", "logs", "memories"] {
        std::fs::create_dir_all(moved.join(folder)).expect("moved folder");
    }
    std::fs::create_dir_all(&settings).expect("settings");
    let registry = ControlResources::default();
    registry.register_runtime(&paths, &release(temp.path()));
    let asks: [PathBuf; 4] = [
        moved.join("data").join("aura.db"),
        moved.join("ui-profile").join("Local State"),
        moved.join("logs").join("core.jsonl"),
        settings.join("models.json"),
    ];
    for path in asks {
        assert!(!registry.viewable_outside(&path), "{} must ask", path.display());
    }
    assert!(
        registry.viewable_outside(&moved.join("memories").join("note.md")),
        "global memory stays an ordinary folder"
    );
}
