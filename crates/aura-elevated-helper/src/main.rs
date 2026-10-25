#![forbid(unsafe_code)]

use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
    str::FromStr,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use aura_domain_common::{
    ContentHash, ContentHashAlgorithm, ContractVersion, HostPreparationOperationId,
    RuntimeInstanceId,
};
use aura_ipc::{WireFrame, read_frame, write_frame};
use aura_permission_domain::{
    ElevatedHelperChallenge, ElevatedHelperChallengeProof, ElevatedHelperCommand,
    ElevatedHelperHello, ElevatedHelperTerminal, HostPreparationBrokerResult,
    HostPreparationOwnership,
};
use aura_runtime_contract::{
    ActiveInstallState, BuildComponentKind, BuildComponentManifest, ReleaseDescriptor,
    ReleaseIdentity,
};
use aura_sandbox::{
    SandboxError, execute_approved_host_preparation_broker_plan, validate_elevated_helper_command,
};
use aura_windows_platform::{
    authenticate_active_core_identity, open_secure_byte_pipe_client, snapshot_process,
    validate_named_pipe_server, verify_active_release_trust,
};
use sha2::{Digest, Sha256};
use tokio::{net::windows::named_pipe::NamedPipeClient, time};
use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
struct HelperOptions {
    pipe_name: String,
    parent_process_id: u32,
    parent_runtime_instance_id: RuntimeInstanceId,
    helper_runtime_instance_id: RuntimeInstanceId,
    component_manifest_path: PathBuf,
}

struct ActiveReleaseCandidate {
    descriptor: ReleaseDescriptor,
    component_manifest_path: PathBuf,
    core_executable: PathBuf,
    core_content_hash: ContentHash,
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("Aura Elevated Helper failed: {error:#}");
        process::exit(1);
    }
}

async fn run() -> Result<()> {
    let options = parse_options(env::args())?;
    let startup_release = load_release_candidate(&options)?;
    validate_pipe_namespace(&options.pipe_name, &startup_release.descriptor.bundle_namespace)?;
    let expected_parent = snapshot_process(options.parent_process_id)
        .context("failed to freeze the Aura Core process identity")?;
    let mut client = time::timeout(CONNECT_TIMEOUT, connect(&options.pipe_name))
        .await
        .context("elevated broker connection timed out")??;
    validate_named_pipe_server(&client, &expected_parent)
        .context("elevated broker server identity was rejected")?;

    let hello = ElevatedHelperHello {
        protocol_version: ContractVersion::CURRENT,
        parent_process_id: options.parent_process_id,
        helper_process_id: process::id(),
        parent_runtime_instance_id: options.parent_runtime_instance_id,
        helper_runtime_instance_id: options.helper_runtime_instance_id,
    };
    write_control(&mut client, &hello).await?;

    let challenge: ElevatedHelperChallenge =
        time::timeout(COMMAND_TIMEOUT, read_control(&mut client))
            .await
            .context("elevated broker challenge timed out")??;
    write_control(
        &mut client,
        &ElevatedHelperChallengeProof { bootstrap_nonce: challenge.bootstrap_nonce },
    )
    .await?;

    let command: ElevatedHelperCommand = time::timeout(COMMAND_TIMEOUT, read_control(&mut client))
        .await
        .context("elevated broker command timed out")??;
    // Resolve and hash the active release immediately before applying the
    // elevated plan. Startup-time validation alone would leave a window in
    // which active.json or the raw manifest bytes could change while UAC and
    // the authenticated handshake are in progress.
    let release = load_release_candidate(&options)?;
    let release_trust =
        verify_active_release_trust(&release.component_manifest_path, &release.descriptor)
            .context("active Release Manifest signature trust is unavailable or invalid")?;
    let _authenticated_core = authenticate_active_core_identity(
        &release_trust,
        &release.descriptor,
        &expected_parent,
        &release.core_executable,
        &release.core_content_hash,
    )
    .context("elevated broker is not the authenticated active Aura Core")?;
    validate_elevated_helper_command(
        &command,
        &release.descriptor,
        options.parent_runtime_instance_id,
    )?;
    let outcome = execute_approved_host_preparation_broker_plan(
        &command.plan,
        &command.elevation_launch_ticket,
    );
    let result =
        terminal_result(outcome, command.plan.operation_id, &command.plan.ownership_before)?;
    let terminal = ElevatedHelperTerminal {
        protocol_version: ContractVersion::CURRENT,
        plan_hash: command.plan.plan_hash,
        result,
    };
    write_control(&mut client, &terminal).await?;
    Ok(())
}

/// Only proven pre-write refusals become terminal results. Once a write may have
/// happened, retain the error: a false marker flag must never stand in for unknown.
fn terminal_result(
    outcome: Result<HostPreparationBrokerResult, SandboxError>,
    operation_id: HostPreparationOperationId,
    ownership_before: &HostPreparationOwnership,
) -> Result<HostPreparationBrokerResult> {
    Ok(match outcome {
        Ok(result) => result,
        Err(error) => match error.pre_write_refusal_code() {
            // A refusal that provably precedes every resource write is reported as
            // a terminal result, not as a crash: closing the pipe here would leave
            // Core unable to tell "nothing was written" from "state unknown".
            Some(code) => HostPreparationBrokerResult {
                operation_id,
                succeeded: false,
                ownership_after: ownership_before.clone(),
                error_code: Some(code.to_owned()),
                // A refusal that precedes every resource write cannot have changed the
                // descriptor, so it never carries the marker difference.
                approved_auto_inherited_marker_lost: false,
            },
            None => return Err(error.into()),
        },
    })
}

async fn connect(pipe_name: &str) -> std::io::Result<NamedPipeClient> {
    loop {
        match open_secure_byte_pipe_client(pipe_name) {
            Ok(client) => return Ok(client),
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                time::sleep(Duration::from_millis(50)).await;
            }
            Err(error) => return Err(error),
        }
    }
}

async fn read_control<T: serde::de::DeserializeOwned>(client: &mut NamedPipeClient) -> Result<T> {
    let WireFrame::Control(payload) = read_frame(client).await? else {
        bail!("elevated broker accepts control frames only");
    };
    serde_json::from_slice(&payload).context("invalid elevated broker control payload")
}

async fn write_control<T: serde::Serialize>(client: &mut NamedPipeClient, value: &T) -> Result<()> {
    let payload = serde_json::to_vec(value)?;
    write_frame(client, &WireFrame::Control(payload)).await?;
    Ok(())
}

fn parse_options(arguments: impl IntoIterator<Item = String>) -> Result<HelperOptions> {
    let mut pipe_name = None;
    let mut parent_process_id = None;
    let mut parent_runtime_instance_id = None;
    let mut helper_runtime_instance_id = None;
    let mut component_manifest_path = None;
    for argument in arguments.into_iter().skip(1) {
        if let Some(value) = argument.strip_prefix("--pipe-name=") {
            pipe_name = Some(value.to_owned());
        } else if let Some(value) = argument.strip_prefix("--parent-pid=") {
            parent_process_id = Some(value.parse::<u32>().context("invalid --parent-pid")?);
        } else if let Some(value) = argument.strip_prefix("--parent-runtime-instance-id=") {
            parent_runtime_instance_id = Some(
                RuntimeInstanceId::from_str(value)
                    .context("invalid --parent-runtime-instance-id")?,
            );
        } else if let Some(value) = argument.strip_prefix("--helper-runtime-instance-id=") {
            helper_runtime_instance_id = Some(
                RuntimeInstanceId::from_str(value)
                    .context("invalid --helper-runtime-instance-id")?,
            );
        } else if let Some(value) = argument.strip_prefix("--component-manifest=") {
            component_manifest_path = Some(PathBuf::from(value));
        } else {
            bail!("unsupported elevated helper argument");
        }
    }
    let options = HelperOptions {
        pipe_name: pipe_name.context("missing --pipe-name")?,
        parent_process_id: parent_process_id.context("missing --parent-pid")?,
        parent_runtime_instance_id: parent_runtime_instance_id
            .context("missing --parent-runtime-instance-id")?,
        helper_runtime_instance_id: helper_runtime_instance_id
            .context("missing --helper-runtime-instance-id")?,
        component_manifest_path: component_manifest_path.context("missing --component-manifest")?,
    };
    let Some(scope) = options.pipe_name.strip_prefix(r"\\.\pipe\Aura\") else {
        bail!("elevated helper bootstrap arguments are invalid");
    };
    let parts = scope.split('\\').collect::<Vec<_>>();
    if options.parent_process_id == 0
        || parts.len() != 3
        || parts[0].len() != 32
        || !parts[0].bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        || parts[1] != "Elevated"
        || parts[2].len() != 32
        || !parts[2].bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        bail!("elevated helper bootstrap arguments are invalid");
    }
    Ok(options)
}

fn load_release_candidate(options: &HelperOptions) -> Result<ActiveReleaseCandidate> {
    let manifest_path = fs::canonicalize(&options.component_manifest_path)
        .context("failed to resolve component manifest")?;
    let (active_path, active_bytes_before, active) = validate_active_manifest_path(&manifest_path)?;
    let manifest_bytes = fs::read(&manifest_path).context("failed to read component manifest")?;
    let component_manifest_hash = ContentHash::new(
        ContentHashAlgorithm::Sha256,
        format!("{:x}", Sha256::digest(&manifest_bytes)),
    )?;
    let manifest: BuildComponentManifest =
        serde_json::from_slice(&manifest_bytes).context("invalid component manifest")?;
    if active.release_descriptor.bundle_namespace != manifest.bundle_namespace
        || active.release_descriptor.identity.release_bundle_id != manifest.release_bundle_id
        || active.release_descriptor.identity.component_manifest_hash != component_manifest_hash
    {
        bail!("active release identity does not match component-manifest.json");
    }
    let helper = manifest
        .components
        .iter()
        .find(|component| component.kind == BuildComponentKind::ElevatedHelper)
        .context("component manifest has no Elevated Helper component")?;
    let mut core_components =
        manifest.components.iter().filter(|component| component.kind == BuildComponentKind::Core);
    let core = core_components.next().context("component manifest has no Core component")?;
    if core_components.next().is_some() {
        bail!("component manifest contains multiple Core components");
    }
    if let Some(protocol) = helper.protocol_version
        && protocol.major != ContractVersion::CURRENT.major
    {
        bail!("Elevated Helper protocol major is incompatible");
    }
    let executable = env::current_exe().context("failed to resolve Elevated Helper executable")?;
    let manifest_root = manifest_path.parent().context("component manifest has no parent")?;
    let expected_executable =
        fs::canonicalize(safe_release_path(manifest_root, &helper.relative_path)?)
            .context("failed to resolve manifest Elevated Helper")?;
    if !expected_executable.starts_with(manifest_root) {
        bail!("Elevated Helper resolves outside the active Release Bundle");
    }
    let executable = fs::canonicalize(executable)
        .context("failed to canonicalize Elevated Helper executable")?;
    if executable != expected_executable {
        bail!("Elevated Helper is not the active manifest component");
    }
    let executable_bytes = fs::read(executable).context("failed to read Elevated Helper")?;
    let actual = match helper.content_hash.algorithm {
        ContentHashAlgorithm::Sha256 => format!("{:x}", Sha256::digest(&executable_bytes)),
        ContentHashAlgorithm::Blake3 => blake3::hash(&executable_bytes).to_hex().to_string(),
    };
    if actual != helper.content_hash.hex {
        bail!("Elevated Helper does not match its Release Bundle manifest");
    }
    let core_executable = fs::canonicalize(safe_release_path(manifest_root, &core.relative_path)?)
        .context("failed to resolve manifest Core executable")?;
    if !core_executable.starts_with(manifest_root) {
        bail!("Core executable resolves outside the active Release Bundle");
    }
    let manifest_bytes_after =
        fs::read(&manifest_path).context("failed to re-read component manifest")?;
    if manifest_bytes_after != manifest_bytes {
        bail!("component manifest changed during Elevated Helper verification");
    }
    let active_bytes_after =
        fs::read(&active_path).context("failed to re-read active release metadata")?;
    if active_bytes_after != active_bytes_before {
        bail!("active release metadata changed during Elevated Helper verification");
    }
    Ok(ActiveReleaseCandidate {
        descriptor: ReleaseDescriptor {
            bundle_namespace: manifest.bundle_namespace,
            identity: ReleaseIdentity {
                release_bundle_id: manifest.release_bundle_id,
                component_manifest_hash,
            },
        },
        component_manifest_path: manifest_path,
        core_executable,
        core_content_hash: core.content_hash.clone(),
    })
}

fn validate_pipe_namespace(
    pipe_name: &str,
    namespace: &aura_runtime_contract::BundleNamespace,
) -> Result<()> {
    let expected = format!(r"\\.\pipe\Aura\{}\Elevated\", namespace.object_name_hash());
    if !pipe_name.starts_with(&expected) {
        bail!("Elevated Helper pipe namespace does not match the signed Release Manifest");
    }
    Ok(())
}

fn validate_active_manifest_path(
    manifest_path: &Path,
) -> Result<(PathBuf, Vec<u8>, ActiveInstallState)> {
    let version_directory =
        manifest_path.parent().context("component manifest has no version directory")?;
    let versions_root =
        version_directory.parent().context("version directory has no versions root")?;
    if versions_root
        .file_name()
        .is_none_or(|name| !name.to_string_lossy().eq_ignore_ascii_case("versions"))
    {
        bail!("component manifest is not inside an Aura active versions directory");
    }
    let install_root = versions_root.parent().context("Aura versions root has no install root")?;
    let active_path = install_root.join("active.json");
    let active_bytes =
        fs::read(&active_path).context("failed to read current active release metadata")?;
    let active: ActiveInstallState =
        serde_json::from_slice(&active_bytes).context("active.json is invalid")?;
    if active.active.is_empty()
        || active.active == "."
        || active.active == ".."
        || active.active.contains('/')
        || active.active.contains('\\')
    {
        bail!("active.json contains an unsafe active version segment");
    }
    let expected_version = fs::canonicalize(versions_root.join(&active.active))
        .context("failed to resolve the current active version directory")?;
    let actual_version = fs::canonicalize(version_directory)
        .context("failed to resolve the frozen version directory")?;
    if expected_version != actual_version {
        bail!("Elevated Helper does not belong to the current active release");
    }
    Ok((active_path, active_bytes, active))
}

fn safe_release_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let candidate = Path::new(relative);
    if candidate.is_absolute()
        || candidate
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        bail!("Elevated Helper path escapes the active Release Bundle");
    }
    let path = root.join(candidate);
    if !path.starts_with(root) {
        bail!("Elevated Helper path escapes the active Release Bundle");
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_pre_write_refusals_produce_a_false_marker_terminal() {
        for error in [
            SandboxError::InvalidHostPreparationPlan("missing frozen original".to_owned()),
            SandboxError::HostPreparationCapabilityConflict("verified capability".to_owned()),
        ] {
            let code = error.pre_write_refusal_code().expect("pre-write code");
            let operation_id = HostPreparationOperationId::new();
            let ownership_before = HostPreparationOwnership {
                all_application_packages_ace_owned: true,
                ..HostPreparationOwnership::default()
            };
            let result = terminal_result(Err(error), operation_id, &ownership_before)
                .expect("proven refusal has a terminal result");
            assert!(!result.succeeded);
            assert!(!result.approved_auto_inherited_marker_lost);
            assert_eq!(result.error_code.as_deref(), Some(code));
            assert_eq!(result.operation_id, operation_id);
            assert_eq!(result.ownership_after, ownership_before);
        }
    }

    #[test]
    fn post_write_errors_cannot_be_encoded_as_no_write_terminal_results() {
        for error in [
            SandboxError::Platform(std::io::Error::other("post-write verification failed")),
            SandboxError::Platform(std::io::Error::other("NUL write failed; rollback succeeded")),
            SandboxError::CleanupFailed("NUL write and rollback failed".to_owned()),
        ] {
            let expected_message = error.to_string();
            let outcome = terminal_result(
                Err(error),
                HostPreparationOperationId::new(),
                &HostPreparationOwnership::default(),
            );
            let retained =
                outcome.expect_err("an uncertain write must not become a terminal refusal");
            assert_eq!(retained.to_string(), expected_message);
            assert_eq!(
                retained
                    .downcast_ref::<SandboxError>()
                    .expect("preserve the Sandbox error type")
                    .pre_write_refusal_code(),
                None
            );
        }
    }

    #[test]
    fn rejects_noncanonical_pipe_without_starting_a_process() {
        let result = parse_options([
            "helper.exe".to_owned(),
            "--pipe-name=not-a-pipe".to_owned(),
            "--parent-pid=42".to_owned(),
            format!("--parent-runtime-instance-id={}", RuntimeInstanceId::new()),
            format!("--helper-runtime-instance-id={}", RuntimeInstanceId::new()),
            "--component-manifest=manifest.json".to_owned(),
        ]);
        assert!(result.is_err());
    }
}
