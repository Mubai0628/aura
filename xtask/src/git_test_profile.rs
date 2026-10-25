//! Tests with a Git profile of their own. Test builds of Git refuse the user's configuration
//! (`git.test_profile_required`), and fixtures that run Git directly must not read it either: the
//! run gets an empty system configuration, no credential helper and a fixture identity, in a folder
//! of the temporary directory named as the Git tests require.
use anyhow::{Context, Result};
use std::{ffi::OsStr, fs, path::Path};

const GLOBAL: &str = concat!(
    "[credential]\n\thelper =\n",
    "[user]\n\tname = Fixture\n\temail = fixture@example.invalid\n",
    "[commit]\n\tgpgsign = false\n",
);

/// `cargo test <arguments>` with a fresh profile, removed afterwards.
pub fn run_tests(root: &Path, arguments: &[&str]) -> Result<()> {
    let folder = std::env::temp_dir().join(format!("aura-git-test-{}", std::process::id()));
    fs::create_dir_all(&folder)
        .with_context(|| format!("failed to create {}", folder.display()))?;
    let result = (|| {
        let global = folder.join("global.cfg");
        let system = folder.join("system.cfg");
        let profile = folder.join("profile.json");
        fs::write(&global, GLOBAL)?;
        fs::write(&system, "")?;
        let document = serde_json::json!({ "global_config": global, "system_config": system });
        fs::write(&profile, serde_json::to_vec(&document)?)?;
        let environment: [(&str, &OsStr); 8] = [
            ("AURA_GIT_TEST_PROFILE", profile.as_os_str()),
            ("GIT_CONFIG_NOSYSTEM", OsStr::new("1")),
            ("GIT_CONFIG_SYSTEM", OsStr::new("NUL")),
            ("GIT_CONFIG_GLOBAL", OsStr::new("NUL")),
            ("GIT_TERMINAL_PROMPT", OsStr::new("0")),
            ("GIT_CONFIG_COUNT", OsStr::new("1")),
            ("GIT_CONFIG_KEY_0", OsStr::new("credential.helper")),
            ("GIT_CONFIG_VALUE_0", OsStr::new("")),
        ];
        let arguments = [&["test"], arguments].concat();
        super::run_with_env(root, "cargo", &arguments, &environment)
    })();
    let _ = fs::remove_dir_all(&folder);
    result
}
