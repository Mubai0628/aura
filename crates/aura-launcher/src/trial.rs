//! A version on trial and what it reports. The desktop writes `launcher/ready.v1.json` in the data
//! folder once Core answers and its window shows; the launcher takes that as the trial's success.
//! A trial that ends before it is ready is recorded in `launcher/trial-failed.v1.json`, which the
//! desktop shows and the updater reads so that version is not offered again by itself.
use std::{fs, path::Path, time::SystemTime};

use anyhow::Result;
use aura_domain_common::{ReleaseBundleId, UtcTimestamp};
use serde::{Deserialize, Serialize};

pub const READY_FILE: &str = "launcher/ready.v1.json";
pub const FAILED_FILE: &str = "launcher/trial-failed.v1.json";

#[derive(Deserialize)]
struct Ready {
    release_bundle_id: ReleaseBundleId,
}

/// Whether the desktop of release `id` reported ready since `since`.
pub fn reported_ready(data_root: &Path, id: ReleaseBundleId, since: SystemTime) -> bool {
    let path = data_root.join(READY_FILE);
    let fresh = fs::metadata(&path)
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| modified >= since);
    fresh
        && fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Ready>(&bytes).ok())
            .is_some_and(|ready| ready.release_bundle_id == id)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrialFailure {
    /// The version folder or its manifest did not check out, so it never started.
    Invalid,
    /// The desktop exited with an error before it was ready.
    Crashed,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FailedTrial {
    pub version: String,
    pub kept_version: String,
    pub failure: TrialFailure,
    pub exit_code: Option<u32>,
    pub detail: String,
    pub at: UtcTimestamp,
}

pub fn record_failed(data_root: &Path, failed: &FailedTrial) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(failed)?;
    bytes.push(b'\n');
    crate::install_state::replace(&data_root.join(FAILED_FILE), &bytes)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn ready(data_root: &Path, id: ReleaseBundleId) {
        let path = data_root.join(READY_FILE);
        fs::create_dir_all(path.parent().expect("launcher folder")).expect("folder");
        fs::write(path, serde_json::json!({ "release_bundle_id": id }).to_string()).expect("ready");
    }

    #[test]
    fn only_a_fresh_report_from_the_trial_release_counts() {
        let data = tempfile::tempdir().expect("data root");
        let trial = ReleaseBundleId::new();
        let started = SystemTime::now() - Duration::from_secs(1);
        assert!(!reported_ready(data.path(), trial, started), "nothing reported yet");
        ready(data.path(), ReleaseBundleId::new());
        assert!(!reported_ready(data.path(), trial, started), "another release reported");
        ready(data.path(), trial);
        assert!(reported_ready(data.path(), trial, started));
        let later = SystemTime::now() + Duration::from_secs(60);
        assert!(!reported_ready(data.path(), trial, later), "a report older than the trial");
    }

    #[test]
    fn a_failed_trial_is_recorded_for_the_desktop_and_the_updater() {
        let data = tempfile::tempdir().expect("data root");
        let failed = FailedTrial {
            version: "0.1.1".into(),
            kept_version: "0.1.0".into(),
            failure: TrialFailure::Crashed,
            exit_code: Some(3),
            detail: "exit code 3".into(),
            at: UtcTimestamp::now(),
        };
        record_failed(data.path(), &failed).expect("record");
        let read: FailedTrial =
            serde_json::from_slice(&fs::read(data.path().join(FAILED_FILE)).expect("file"))
                .expect("record json");
        assert_eq!(read, failed);
    }
}
