//! How Main starts Core: the command line Core is given and what it must say.
use std::{io::Write, path::PathBuf, str::FromStr};

use anyhow::{Context, Result, bail};
use aura_domain_common::RuntimeInstanceId;
use aura_runtime_contract::BundleNamespace;

/// `--version` and `--help`, answered without starting Core; `None` for any other command line.
pub fn version_or_help(arguments: &[String], out: &mut dyn Write) -> Option<u8> {
    let given = |names: [&str; 2]| {
        arguments.iter().skip(1).any(|argument| names.contains(&argument.as_str()))
    };
    let text = if given(["--version", "-V"]) {
        format!("aura-core {}", env!("CARGO_PKG_VERSION"))
    } else if given(["--help", "-h"]) {
        "Aura Core P01 Sidecar\n\nAura Core is started by Electron Main with a private bootstrap contract."
            .to_owned()
    } else {
        return None;
    };
    let _ = writeln!(out, "{text}");
    Some(0)
}

#[derive(Clone, Debug)]
pub struct CoreOptions {
    pub pipe_name: String,
    pub bootstrap_stdin: bool,
    pub parent_process_id: u32,
    pub runtime_instance_id: RuntimeInstanceId,
    pub component_manifest_path: PathBuf,
    /// The data folder Main was given; Core works it out again and refuses a different one
    /// (`locations`).
    pub data_root: PathBuf,
    pub allow_dev_component_mismatch: bool,
    pub safe_mode: bool,
    pub test_mode: CoreTestMode,
    /// Set by Main when a Core it had serving ended while Aura stayed open: why, so this Core
    /// continues the turns that cut off (`agent_application::continuation`).
    pub continue_after: Option<aura_agent_domain::CoreRestartCause>,
}

impl CoreOptions {
    pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut pipe_name = None;
        let mut bootstrap_stdin = false;
        let mut parent_process_id = None;
        let mut runtime_instance_id = None;
        let mut component_manifest_path = None;
        let mut data_root = None;
        let mut allow_dev_component_mismatch = false;
        let mut safe_mode = false;
        let mut test_mode = CoreTestMode::Normal;
        let mut continue_after = None;

        for argument in arguments.into_iter().skip(1) {
            if let Some(value) = argument.strip_prefix("--pipe-name=") {
                pipe_name = Some(value.to_owned());
            } else if argument == "--bootstrap-stdin" {
                bootstrap_stdin = true;
            } else if let Some(value) = argument.strip_prefix("--parent-pid=") {
                parent_process_id = Some(value.parse::<u32>().context("invalid --parent-pid")?);
            } else if let Some(value) = argument.strip_prefix("--runtime-instance-id=") {
                runtime_instance_id = Some(
                    RuntimeInstanceId::from_str(value).context("invalid --runtime-instance-id")?,
                );
            } else if let Some(value) = argument.strip_prefix("--component-manifest=") {
                component_manifest_path = Some(PathBuf::from(value));
            } else if let Some(value) = argument.strip_prefix(aura_data_location::DATA_ROOT) {
                data_root = Some(PathBuf::from(value));
            } else if argument == "--allow-dev-component-mismatch" {
                allow_dev_component_mismatch = true;
            } else if argument == "--safe-mode" {
                safe_mode = true;
            } else if let Some(value) = argument.strip_prefix("--test-mode=") {
                test_mode = CoreTestMode::parse(value)?;
            } else if let Some(value) = argument.strip_prefix("--continue-after=") {
                continue_after = Some(
                    aura_agent_domain::CoreRestartCause::parse_argument(value)
                        .context("invalid --continue-after")?,
                );
            } else {
                bail!("unsupported aura-core argument: {argument}");
            }
        }

        let options = Self {
            pipe_name: pipe_name.context("missing --pipe-name")?,
            bootstrap_stdin,
            parent_process_id: parent_process_id.context("missing --parent-pid")?,
            runtime_instance_id: runtime_instance_id.context("missing --runtime-instance-id")?,
            component_manifest_path: component_manifest_path
                .context("missing --component-manifest")?,
            data_root: data_root.context("missing --data-root")?,
            allow_dev_component_mismatch,
            safe_mode,
            test_mode,
            continue_after,
        };
        options.validate()?;
        Ok(options)
    }

    fn validate(&self) -> Result<()> {
        if self.parent_process_id == 0 {
            bail!("--parent-pid must not be zero");
        }
        if !self.bootstrap_stdin {
            bail!("Aura Core requires the inherited bootstrap input channel");
        }
        if !self.data_root.is_absolute() {
            bail!("--data-root must be an absolute path");
        }
        let scope = self
            .pipe_name
            .strip_prefix(r"\\.\pipe\Aura\")
            .context("pipe name is outside the Aura namespace")?;
        let parts = scope.split('\\').collect::<Vec<_>>();
        if parts.len() != 3
            || !is_lower_hex_128(parts[0])
            || parts[1] != self.runtime_instance_id.to_string()
            || !is_lower_hex_128(parts[2])
        {
            bail!("pipe name must contain namespace hash, runtime identity, and 128-bit nonce");
        }
        Ok(())
    }

    pub fn validate_release_namespace(&self, namespace: &BundleNamespace) -> Result<()> {
        let expected = format!(
            r"\\.\pipe\Aura\{}\{}\",
            namespace.object_name_hash(),
            self.runtime_instance_id
        );
        self.pipe_name
            .strip_prefix(&expected)
            .context("pipe name does not match the active Release namespace")?;
        Ok(())
    }
}

fn is_lower_hex_128(value: &str) -> bool {
    value.len() == 32
        && value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

/// What a test asks Core to do instead of starting normally (`--test-mode=`).
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CoreTestMode {
    #[default]
    Normal,
    Incompatible,
    CrashAfterHandshake,
    DisconnectAfterReady,
    ProjectionRevisionGap,
    StartupSequenceGap,
    StallAfterHandshake,
    MigrationRecoveryRequired,
}

impl CoreTestMode {
    fn parse(value: &str) -> Result<Self> {
        Ok(match value {
            "incompatible" => Self::Incompatible,
            "crash-after-handshake" => Self::CrashAfterHandshake,
            "disconnect-after-ready" => Self::DisconnectAfterReady,
            "projection-revision-gap" => Self::ProjectionRevisionGap,
            "startup-sequence-gap" => Self::StartupSequenceGap,
            "stall-after-handshake" => Self::StallAfterHandshake,
            "migration-recovery-required" => Self::MigrationRecoveryRequired,
            _ => bail!("unknown Core test mode: {value}"),
        })
    }
}

#[cfg(test)]
mod tests;
