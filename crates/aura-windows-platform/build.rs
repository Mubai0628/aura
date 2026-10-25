use std::{
    env, fs,
    path::{Path, PathBuf},
};

#[path = "src/release_root_profile.rs"]
mod release_root_profile;

const PRODUCTION_BUILD_ENV: &str = "AURA_PRODUCTION_RELEASE_BUILD";
const ROOT_RELATIVE_PATH: &str =
    "../../resources/release-trust/production/aura-production-release-root-v1.der";

fn main() {
    println!("cargo:rerun-if-env-changed={PRODUCTION_BUILD_ENV}");
    println!("cargo:rustc-check-cfg=cfg(aura_production_release_trust)");
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let root_path = manifest_dir.join(ROOT_RELATIVE_PATH);
    println!("cargo:rerun-if-changed={}", watchable(&root_path).display());

    match env::var(PRODUCTION_BUILD_ENV).as_deref() {
        Ok("1") => {
            let der = fs::read(&root_path).unwrap_or_else(|error| {
                panic!("Production Release build requires {}: {error}", root_path.display())
            });
            release_root_profile::validate_production_root(&der).unwrap_or_else(|error| {
                panic!("Aura Production Release Root profile is invalid: {error}")
            });
            println!("cargo:rustc-cfg=aura_production_release_trust");
        }
        Ok(value) => panic!("{PRODUCTION_BUILD_ENV} must be exactly 1 when set, got {value:?}"),
        Err(env::VarError::NotPresent) => {}
        Err(error) => panic!("failed to read {PRODUCTION_BUILD_ENV}: {error}"),
    }
}

/// The nearest existing path that can actually be watched for a change to `path`.
///
/// Cargo treats a `rerun-if-changed` path that does not exist as changed on every build,
/// so naming a file this checkout does not carry makes the build script permanently
/// stale: every dependent unit recompiles and every execution image is relinked on each
/// build. The publication binds exact SHA-256 digests, so a relink is a version change
/// whenever the link is not reproducible, and it costs a full rebuild for no source
/// change. Watching the nearest existing ancestor keeps the original intent — the
/// production trust root appearing later is still a change — without the permanent
/// staleness.
fn watchable(path: &Path) -> PathBuf {
    let mut candidate = path.to_path_buf();
    while !candidate.exists() {
        match candidate.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => candidate = parent.to_path_buf(),
            _ => break,
        }
    }
    candidate
}
