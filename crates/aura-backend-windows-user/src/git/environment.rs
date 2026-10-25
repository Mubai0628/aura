//! Git obtains the same filtered user environment as Shell, independently of PS readiness.
use anyhow::Result;
#[cfg(any(test, feature = "git-test-profile"))]
use anyhow::ensure;
use aura_windows_platform::NativeEnvironmentEntry;

pub(super) fn current() -> Result<Vec<NativeEnvironmentEntry>> {
    let environment = crate::compat::environment::build(
        aura_windows_platform::current_user_environment()?,
        None,
    )?;
    #[cfg(feature = "git-test-profile")]
    let environment = if let Some(profile) = std::env::var_os("AURA_GIT_TEST_PROFILE") {
        isolated(environment, std::path::Path::new(&profile))?
    } else {
        anyhow::bail!(
            "git.test_profile_required: test-feature Git never falls back to owner config"
        )
    };
    Ok(fixed(environment))
}

pub(super) fn fixed(mut entries: Vec<NativeEnvironmentEntry>) -> Vec<NativeEnvironmentEntry> {
    for (name, value) in [
        ("GIT_TERMINAL_PROMPT", "0"),
        ("LC_ALL", "C"),
        ("LANG", "C"),
        ("GIT_TRACE2", ""),
        ("GIT_TRACE2_EVENT", ""),
        ("GIT_TRACE2_PERF", ""),
    ] {
        put(&mut entries, name, value);
    }
    entries
}

pub(super) fn put(entries: &mut Vec<NativeEnvironmentEntry>, name: &str, value: &str) {
    entries.retain(|entry| !entry.name.eq_ignore_ascii_case(name));
    entries.push(NativeEnvironmentEntry {
        name: name.into(),
        value_utf16: value.encode_utf16().collect(),
    });
}

#[cfg(any(test, feature = "git-test-profile"))]
pub(super) fn isolated(
    mut entries: Vec<NativeEnvironmentEntry>,
    profile: &std::path::Path,
) -> Result<Vec<NativeEnvironmentEntry>> {
    // Only the explicit, compile-time absent production seam can replace this environment.
    // The marker path is selected by the test launcher, never by a model or IPC argument.
    let profile = std::fs::canonicalize(profile)?;
    let parent = profile.parent().ok_or_else(|| anyhow::anyhow!("git.test_profile_root"))?;
    let temporary = std::fs::canonicalize(std::env::temp_dir())?;
    ensure!(
        parent.starts_with(&temporary)
            && parent
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("aura-git-test-")),
        "git.test_profile_must_be_owned_temporary_directory"
    );
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&profile)?)?;
    let config = |key| -> Result<String> {
        let path = std::path::PathBuf::from(
            value[key].as_str().ok_or_else(|| anyhow::anyhow!("git.test_profile_field"))?,
        );
        let path = std::fs::canonicalize(path)?;
        ensure!(path.starts_with(parent), "git.test_profile_config_outside_fixture");
        Ok(aura_windows_platform::dos_drive_path(&path)?.to_string_lossy().into_owned())
    };
    let global = config("global_config")?;
    let system = config("system_config")?;
    // Remove inherited Git redirects and command configuration, including config counters.
    entries.retain(|entry| !entry.name.to_ascii_uppercase().starts_with("GIT_"));
    let home = aura_windows_platform::dos_drive_path(parent)?;
    let home = home.to_str().ok_or_else(|| anyhow::anyhow!("git.test_profile_utf8"))?;
    for (name, value) in [
        ("GIT_CONFIG_GLOBAL", global.as_str()),
        ("GIT_CONFIG_SYSTEM", system.as_str()),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_TERMINAL_PROMPT", "0"),
        ("HOME", home),
        ("XDG_CONFIG_HOME", home),
        ("HTTP_PROXY", ""),
        ("HTTPS_PROXY", ""),
        ("ALL_PROXY", ""),
        ("NO_PROXY", "*"),
    ] {
        put(&mut entries, name, value);
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn trace_destinations_are_overridden_after_the_user_environment_is_filtered() {
        let input = ["GIT_TRACE2", "git_trace2_event", "GIT_TRACE2_PERF"]
            .into_iter()
            .map(|name| NativeEnvironmentEntry {
                name: name.into(),
                value_utf16: "MUST_NOT_OPEN".encode_utf16().collect(),
            })
            .collect();
        let values = fixed(input);
        for name in ["GIT_TRACE2", "GIT_TRACE2_EVENT", "GIT_TRACE2_PERF"] {
            let selected = values
                .iter()
                .filter(|entry| entry.name.eq_ignore_ascii_case(name))
                .collect::<Vec<_>>();
            assert_eq!(selected.len(), 1);
            assert!(selected[0].value_utf16.is_empty());
        }
    }
}
