//! A TEMP folder for one test run.
//!
//! Tests, and the programs they start, put their temporary roots under TEMP. A root whose store,
//! log or child process is still open when its test ends cannot be deleted then, and `tempfile`
//! drops that error, so every full Core run left gigabytes behind. Each run gets a folder of its
//! own, deleted once every test program has exited and closed its files.
use std::{ffi::OsString, fs, path::PathBuf};

use anyhow::{Context, Result};

pub(super) struct RunTemp {
    folder: PathBuf,
}

impl RunTemp {
    pub(super) fn create() -> Result<Self> {
        let folder = std::env::temp_dir().join(format!("aura-tests-{}", std::process::id()));
        // A folder with this process id is left from an earlier run that was stopped.
        let _ = fs::remove_dir_all(&folder);
        fs::create_dir_all(&folder)
            .with_context(|| format!("failed to create {}", folder.display()))?;
        Ok(Self { folder })
    }

    /// TEMP and TMP of the run, and the Git test profile inside it (the Git tests require their
    /// profile in a folder of their own TEMP).
    pub(super) fn environment(&self) -> Result<Vec<(&'static str, OsString)>> {
        let mut environment = vec![
            ("TEMP", self.folder.clone().into_os_string()),
            ("TMP", self.folder.clone().into_os_string()),
        ];
        environment.extend(crate::git_test_profile::environment(&self.folder)?);
        Ok(environment)
    }
}

impl Drop for RunTemp {
    /// What cannot be deleted is reported, not fatal: the tests have run.
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.folder) {
            eprintln!(
                "warning: the test run's TEMP folder {} was not deleted completely: {error}",
                self.folder.display()
            );
        }
    }
}
