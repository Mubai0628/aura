//! No host preparation: a duplicate of Core's own token lowered to Low integrity.
mod integrity;
mod token;
use crate::{
    JobLaunchError, JobLaunchTiming, NativeEnvironmentEntry, NativeSandboxProcess, RootJob,
};
pub use integrity::GitReadDirectory;
#[cfg(feature = "git-read-test-profile")]
pub use integrity::set_git_read_fixture_integrity;
use std::path::Path;

pub fn spawn_git_read_process(
    job: &RootJob,
    executable: &Path,
    arguments: &[Vec<u16>],
    directory: &Path,
    environment: &[NativeEnvironmentEntry],
) -> Result<(NativeSandboxProcess, JobLaunchTiming), JobLaunchError> {
    job.verify_single_process()
        .map_err(|source| JobLaunchError { source, timing: Box::default() })?;
    let token = token::low_primary_token()
        .map_err(|source| JobLaunchError { source, timing: Box::default() })?;
    crate::sandbox::launch::spawn_owned_job(
        job,
        executable,
        arguments,
        directory,
        environment,
        crate::sandbox::launch::LaunchOverrides { token: Some(&token), ..Default::default() },
    )
}
