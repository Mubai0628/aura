//! Tests with a Git profile of their own. Test builds of Git refuse the user's configuration
//! (`git.test_profile_required`), and fixtures that run Git directly must not read it either: the
//! run gets an empty system configuration, no credential helper and a fixture identity, in a folder
//! of the run's TEMP named as the Git tests require.
use anyhow::{Context, Result};
use std::{ffi::OsString, fs, path::Path};

const GLOBAL: &str = concat!(
    "[credential]\n\thelper =\n",
    "[user]\n\tname = Fixture\n\temail = fixture@example.invalid\n",
    "[commit]\n\tgpgsign = false\n",
);

/// The Git profile variables, with the profile written into a new folder of `temp` (the TEMP the
/// tests will see).
pub fn environment(temp: &Path) -> Result<Vec<(&'static str, OsString)>> {
    let folder = temp.join(format!("aura-git-test-{}", std::process::id()));
    fs::create_dir_all(&folder)
        .with_context(|| format!("failed to create {}", folder.display()))?;
    let global = folder.join("global.cfg");
    let system = folder.join("system.cfg");
    let profile = folder.join("profile.json");
    fs::write(&global, GLOBAL)?;
    fs::write(&system, "")?;
    let document = serde_json::json!({ "global_config": global, "system_config": system });
    fs::write(&profile, serde_json::to_vec(&document)?)?;
    Ok(vec![
        ("AURA_GIT_TEST_PROFILE", profile.into_os_string()),
        ("GIT_CONFIG_NOSYSTEM", "1".into()),
        ("GIT_CONFIG_SYSTEM", "NUL".into()),
        ("GIT_CONFIG_GLOBAL", "NUL".into()),
        ("GIT_TERMINAL_PROMPT", "0".into()),
        ("GIT_CONFIG_COUNT", "1".into()),
        ("GIT_CONFIG_KEY_0", "credential.helper".into()),
        ("GIT_CONFIG_VALUE_0", "".into()),
    ])
}
