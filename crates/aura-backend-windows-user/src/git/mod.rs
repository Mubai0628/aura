//! The sole native-Git process adapter. Never launches a Shell or waits for PowerShell.
mod discovery;
mod environment;
mod process;
use anyhow::Result;
use aura_windows_platform::NativeEnvironmentEntry;
use aura_windows_user_contract::ExecutionFileBinding;
pub use discovery::GitDetection;
pub use process::GitProcessResult;
use std::{path::Path, sync::atomic::AtomicBool, time::Duration};

pub struct GitRunner {
    image: ExecutionFileBinding,
    environment: Vec<NativeEnvironmentEntry>,
}
impl GitRunner {
    pub fn image_binding(&self) -> &ExecutionFileBinding {
        &self.image
    }
    /// Bind the selected executable and use the current user's fresh environment block.
    pub fn bind(path: &Path) -> Result<Self> {
        let path = discovery::real_image(path);
        let (image, _retained) = crate::file_binding::capture_git_image(&path)?;
        Ok(Self { image, environment: environment::current()? })
    }
    pub fn run(
        &self,
        directory: &Path,
        args: &[String],
        cancel: &AtomicBool,
        timeout: Duration,
        limit: usize,
    ) -> Result<GitProcessResult> {
        process::run(process::ProcessSpec {
            image: &self.image,
            directory,
            args,
            environment: &self.environment,
            cancelled: cancel,
            timeout,
            limit,
            mode: process::Mode::IsolatedRead,
            on_started: None,
        })
    }
    /// Explicit Native execution for local writes/remotes and temporary fixture preparation.
    pub fn run_native(
        &self,
        directory: &Path,
        args: &[String],
        cancel: &AtomicBool,
        timeout: Duration,
        limit: usize,
    ) -> Result<GitProcessResult> {
        process::run(process::ProcessSpec {
            image: &self.image,
            directory,
            args,
            environment: &self.environment,
            cancelled: cancel,
            timeout,
            limit,
            mode: process::Mode::Native,
            on_started: None,
        })
    }
    /// Local mutations: retain Shell's soft-offline proxy and the start lease.
    /// This is an environment hint for hooks, not an OS network-isolation claim.
    pub fn run_native_at_start(
        &self,
        directory: &Path,
        args: &[String],
        cancel: &AtomicBool,
        (timeout, limit): (Duration, usize),
        on_started: &mut dyn FnMut(),
    ) -> Result<GitProcessResult> {
        use std::os::windows::ffi::OsStringExt;
        let proxy = crate::compat::soft_proxy::SoftProxy::open()?;
        let selected = self.environment.iter().map(|entry| {
            (
                std::ffi::OsString::from(&entry.name),
                std::ffi::OsString::from_wide(&entry.value_utf16),
            )
        });
        let environment = crate::compat::environment::build(selected, Some(&proxy.endpoint))?;
        process::run(process::ProcessSpec {
            image: &self.image,
            directory,
            args,
            environment: &environment,
            cancelled: cancel,
            timeout,
            limit,
            mode: process::Mode::Native,
            on_started: Some(on_started),
        })
    }
    /// Remote Git keeps the filtered user's network configuration, without Aura's proxy.
    /// Core must authorize the remote operation before calling this primitive. The start
    /// callback releases its permission lease only after creation-time Job membership.
    pub fn run_remote_at_start(
        &self,
        directory: &Path,
        args: &[String],
        cancel: &AtomicBool,
        (timeout, limit): (Duration, usize),
        on_started: &mut dyn FnMut(),
    ) -> Result<GitProcessResult> {
        process::run(process::ProcessSpec {
            image: &self.image,
            directory,
            args,
            environment: &self.environment,
            cancelled: cancel,
            timeout,
            limit,
            mode: process::Mode::Native,
            on_started: Some(on_started),
        })
    }
    #[cfg(any(test, feature = "git-test-profile"))]
    pub fn isolated(path: &Path, profile: &Path) -> Result<Self> {
        let path = discovery::real_image(path);
        let (image, _retained) = crate::file_binding::capture_git_image(&path)?;
        // The caller supplies its own fixture. Do not consult the process-level
        // seam required by bind(): standalone unit tests must remain isolated too.
        let environment = crate::compat::environment::build(
            aura_windows_platform::current_user_environment()?,
            None,
        )?;
        let environment = environment::fixed(environment::isolated(environment, profile)?);
        Ok(Self { image, environment })
    }
}

#[cfg(test)]
mod native_tests;
#[cfg(test)]
mod tests;
