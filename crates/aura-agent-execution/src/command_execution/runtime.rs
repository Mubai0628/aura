use aura_core_base::release::CoreReleaseContext;
use std::path::PathBuf;

pub(super) fn managed_root(release: &CoreReleaseContext) -> anyhow::Result<PathBuf> {
    // Trusted launch configuration, never a model-supplied path.
    if let Some(path) = std::env::var_os("AURA_MANAGED_SHELL_ROOT") {
        let path = PathBuf::from(path);
        anyhow::ensure!(path.is_absolute(), "compat.runtime_root_must_be_absolute");
        return Ok(path);
    }
    let pin: serde_json::Value =
        serde_json::from_str(include_str!("../../../../runtimes/powershell.json"))?;
    let id = pin["id"].as_str().ok_or_else(|| anyhow::anyhow!("compat.runtime_pin_missing"))?;
    match release.release_descriptor.bundle_namespace.trust_domain() {
        Some(
            aura_runtime_contract::ReleaseTrustDomain::Development
            | aura_runtime_contract::ReleaseTrustDomain::Test,
        ) => Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/managed-runtimes")
            .join(id)),
        Some(aura_runtime_contract::ReleaseTrustDomain::Production) => Ok(release
            .manifest_path
            .parent()
            .ok_or_else(|| anyhow::anyhow!("compat.runtime_release_root_missing"))?
            .join("managed-runtimes")
            .join(id)),
        None => anyhow::bail!("compat.runtime_namespace_invalid"),
    }
}
