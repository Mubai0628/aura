use super::{GitRunner, environment};
use anyhow::{Result, ensure};
use aura_windows_user_contract::ExecutionFileBinding;
use std::{
    path::{Path, PathBuf},
    sync::{Mutex, PoisonError, atomic::AtomicBool},
    time::Duration,
};

pub struct GitDetection {
    pub executable: Option<String>,
    pub version: Option<String>,
    pub state: &'static str,
    pub runner: Option<GitRunner>,
}

impl GitRunner {
    /// Registry first, then absolute entries from the user's Windows environment PATH.
    /// Called for each request, so replacement/upgrades are rebound without restarting Core. The
    /// program found is checked once: while it is as it was, neither its
    /// bytes nor `--version` are read again.
    pub fn detect() -> Result<GitDetection> {
        let environment = environment::current()?;
        #[cfg(feature = "git-test-profile")]
        if let Some(profile) = std::env::var_os("AURA_GIT_TEST_PROFILE") {
            let value: serde_json::Value = serde_json::from_slice(&std::fs::read(profile)?)?;
            if let Some(path) = value.get("git_path").and_then(serde_json::Value::as_str) {
                return Self::detect_at(Path::new(path), environment);
            }
        }
        let mut candidates = vec![];
        if let Some(installation) = aura_windows_platform::machine_git_installation()? {
            candidates.push(installation.join("cmd/git.exe"));
            candidates.push(installation.join("bin/git.exe"));
        }
        if let Some(path) = environment.iter().find(|entry| entry.name.eq_ignore_ascii_case("PATH"))
        {
            let path = String::from_utf16(&path.value_utf16)?;
            candidates.extend(path.split(';').filter_map(|part| {
                let path = PathBuf::from(part.trim_matches('"'));
                path.is_absolute().then(|| path.join("git.exe"))
            }));
        }
        match candidates.into_iter().find(|path| path.is_file()) {
            Some(path) => Self::detect_at(&path, environment),
            None => {
                Ok(GitDetection { executable: None, version: None, state: "missing", runner: None })
            }
        }
    }

    /// What `detect()` finds under a test profile, for this profile's `git_path` and isolated
    /// configuration, without the process-wide variable.
    #[cfg(any(test, feature = "git-test-profile"))]
    pub fn detect_with_profile(profile: &Path) -> Result<GitDetection> {
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(profile)?)?;
        let path = value
            .get("git_path")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("git.test_profile_field"))?;
        let environment = crate::compat::environment::build(
            aura_windows_platform::current_user_environment()?,
            None,
        )?;
        let environment = environment::fixed(environment::isolated(environment, profile)?);
        Self::detect_at(Path::new(path), environment)
    }

    fn detect_at(
        path: &Path,
        environment: Vec<aura_windows_platform::NativeEnvironmentEntry>,
    ) -> Result<GitDetection> {
        if !path.is_file() {
            return Ok(GitDetection {
                executable: None,
                version: None,
                state: "missing",
                runner: None,
            });
        }
        let path = real_image(path);
        let (image, _retained) = crate::file_binding::capture_git_image(&path)?;
        if let Some((version, state)) = known(&image) {
            return Ok(GitDetection {
                executable: Some(image.path.clone()),
                version: Some(version),
                state,
                runner: (state == "available").then_some(GitRunner { image, environment }),
            });
        }
        let detection = GitRunner { image: image.clone(), environment }.inspect_version()?;
        if let Some(version) = &detection.version {
            remember(image, version.clone(), detection.state);
        }
        Ok(detection)
    }

    /// Inspect this bound image. Also used by isolated native executable fixtures.
    pub fn inspect_version(self) -> Result<GitDetection> {
        let executable = self.image.path.clone();
        let scratch = tempfile::Builder::new().prefix("aura-git-detect-").tempdir()?;
        let value = self.run(
            scratch.path(),
            &["--version".into()],
            &AtomicBool::new(false),
            Duration::from_secs(5),
            65536,
        )?;
        ensure!(value.stopped && value.reason == "exited", "git.detection_incomplete");
        let version = String::from_utf8(value.stdout)?.trim().to_owned();
        let state = if value.exit_code != Some(0) { "invalid" } else { version_state(&version) };
        Ok(GitDetection {
            executable: Some(executable),
            version: Some(version),
            state,
            runner: (state == "available").then_some(self),
        })
    }
}

/// The Git program last detected, the version it answered and what that meant.
static DETECTED: Mutex<Option<(ExecutionFileBinding, String, &'static str)>> = Mutex::new(None);

fn known(image: &ExecutionFileBinding) -> Option<(String, &'static str)> {
    let detected = DETECTED.lock().unwrap_or_else(PoisonError::into_inner);
    detected
        .as_ref()
        .filter(|(known, _, _)| known == image)
        .map(|(_, version, state)| (version.clone(), *state))
}

fn remember(image: ExecutionFileBinding, version: String, state: &'static str) {
    let mut detected = DETECTED.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some((before, previous, _)) = detected.as_ref()
        && before != &image
    {
        // Another program, or the same one changed: the user upgraded or replaced Git.
        crate::checked_program::note(
            "program.detected_again",
            serde_json::json!({"program": image.path, "previous_program": before.path,
                "previous_version": previous, "version": version, "state": state}),
        );
    }
    *detected = Some((image, version, state));
}

/// Git for Windows cmd/bin launchers need a second process. Locate the actual
/// image in that same installation; all candidates are still identity/hash bound.
pub(super) fn real_image(path: &Path) -> PathBuf {
    let Some(parent) = path.parent() else {
        return path.into();
    };
    if parent
        .file_name()
        .is_some_and(|name| name.eq_ignore_ascii_case("cmd") || name.eq_ignore_ascii_case("bin"))
        && let Some(root) = parent.parent()
    {
        for suffix in
            ["mingw64/bin/git.exe", "libexec/git-core/git.exe", "mingw64/libexec/git-core/git.exe"]
        {
            let candidate = root.join(suffix);
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    path.into()
}

pub(super) fn version_state(text: &str) -> &'static str {
    let Some(number) = text.strip_prefix("git version ") else {
        return "invalid";
    };
    let mut parts = number.split('.');
    match (
        parts.next().and_then(|n| n.parse::<u32>().ok()),
        parts.next().and_then(|n| n.parse::<u32>().ok()),
    ) {
        (Some(major), Some(minor)) if major > 2 || (major == 2 && minor >= 40) => "available",
        (Some(_), Some(_)) => "too_old",
        _ => "invalid",
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn minimum_git_240_version_gate() {
        for (version, expected) in [
            ("git version 2.40.0.windows.1", "available"),
            ("git version 2.55.0.windows.2", "available"),
            ("git version 2.39.9.windows.1", "too_old"),
            ("git version 1.99.0", "too_old"),
            ("not git", "invalid"),
        ] {
            assert_eq!(super::version_state(version), expected);
        }
    }
}
