use std::{
    fs, io,
    path::{Path, PathBuf},
    process,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use aura_domain_common::{
    ContentHash, ContentHashAlgorithm, ContractVersion, RuntimeInstanceId, SecretNonce,
};
use aura_ipc::{WireFrame, read_frame, write_frame};
use aura_permission_domain::{
    ElevatedHelperChallenge, ElevatedHelperChallengeProof, ElevatedHelperCommand,
    ElevatedHelperHello, ElevatedHelperTerminal, ElevationLaunchTicket, HostPreparationBrokerPlan,
    HostPreparationBrokerResult, host_preparation_pre_write_refusal_code,
};
use aura_runtime_contract::{
    ActiveInstallState, BuildComponentKind, BuildComponentManifest, ReleaseDescriptor,
};
use aura_sandbox::ElevatedBrokerGuard;
use aura_windows_platform::{
    ElevatedHelperLaunch, create_secure_byte_pipe_server, launch_verified_aura_elevated_helper,
    validate_named_pipe_client, verify_active_release_trust,
};
use serde::{Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{net::windows::named_pipe::NamedPipeServer, time};

const HELPER_CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
const HELPER_PROTOCOL_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Debug)]
pub struct ElevatedBrokerOptions {
    pub helper_executable: PathBuf,
    pub helper_content_hash: ContentHash,
    pub component_manifest_path: PathBuf,
    pub release_descriptor: ReleaseDescriptor,
    pub runtime_instance_id: RuntimeInstanceId,
}

#[derive(Debug, Error)]
#[error(
    "sandbox.host_preparation_release_trust_failed: 这个 Aura 安装包的签名校验没有通过或无法进行，没有请求管理员权限，系统没有被修改；请从官方发布页重新下载安装。"
)]
pub(crate) struct ActiveReleaseTrustError {
    #[source]
    source: io::Error,
}

impl ActiveReleaseTrustError {
    pub(crate) fn new(source: io::Error) -> Self {
        Self { source }
    }
}

/// The Elevated Helper refused the plan, and proved it wrote nothing.
///
/// Kept distinct from a transport failure so the operation is not recorded as
/// "state unknown" when the refusal itself establishes that no resource changed.
#[derive(Debug, Error)]
#[error(
    "sandbox.host_preparation_refused: 管理员助手在改动系统之前拒绝了这份兼容环境方案（{code}），系统没有被修改；请看“兼容环境”一项的状态，核对后再决定是否重试。"
)]
pub(crate) struct ElevatedHelperRefusal {
    code: &'static str,
}

impl ElevatedHelperRefusal {
    /// Built by the tests that assert the refusal keeps its own code instead of
    /// being closed out as an unknown state.
    #[cfg(test)]
    pub(crate) fn new(code: &'static str) -> Self {
        Self { code }
    }

    pub(crate) fn code(&self) -> &'static str {
        self.code
    }
}

pub async fn execute_elevated_host_preparation(
    options: &ElevatedBrokerOptions,
    plan: HostPreparationBrokerPlan,
    plan_nonce: SecretNonce,
    elevation_launch_ticket: ElevationLaunchTicket,
    on_authenticated: impl FnOnce() -> Result<()>,
) -> Result<HostPreparationBrokerResult> {
    validate_active_release_identity(options, &plan)?;
    let _release_trust =
        verify_active_release_trust(&options.component_manifest_path, &options.release_descriptor)
            .map_err(ActiveReleaseTrustError::new)?;
    let mut pipe_random = [0_u8; 16];
    getrandom::fill(&mut pipe_random).context("sandbox.elevated_helper_random_failed: 没能为管理员助手生成随机管道名，没有请求管理员权限，系统没有被修改；请重试。")?;
    let pipe_name = format!(
        r"\\.\pipe\Aura\{}\Elevated\{}",
        options.release_descriptor.bundle_namespace.object_name_hash(),
        lower_hex(&pipe_random)
    );
    let mut nonce = [0_u8; 32];
    getrandom::fill(&mut nonce).context("sandbox.elevated_helper_random_failed: 没能为管理员助手生成随机校验码，没有请求管理员权限，系统没有被修改；请重试。")?;
    let challenge_nonce = SecretNonce::from_bytes(nonce);
    let helper_runtime_instance_id = RuntimeInstanceId::new();
    let mut guard = ElevatedBrokerGuard::new(
        challenge_nonce.clone(),
        process::id(),
        ContractVersion::CURRENT,
        options.runtime_instance_id,
        helper_runtime_instance_id,
    );
    let mut server = create_secure_byte_pipe_server(&pipe_name, true)
        .context("sandbox.elevated_helper_pipe_failed: 没能建立与管理员助手通信的管道，没有请求管理员权限，系统没有被修改；请重试。")?;
    let helper = launch_verified_aura_elevated_helper(&ElevatedHelperLaunch {
        executable: options.helper_executable.clone(),
        component_manifest_path: options.component_manifest_path.clone(),
        expected_content_hash: options.helper_content_hash.clone(),
        pipe_name,
        parent_process_id: process::id(),
        parent_runtime_instance_id: options.runtime_instance_id,
        helper_runtime_instance_id,
        allow_dev_component_mismatch: false,
    })
    .context("sandbox.elevated_helper_launch_failed: 没能启动经过校验的 Aura 管理员助手；请看“兼容环境”一项的状态，核对后再决定是否重试。")?;
    time::timeout(HELPER_CONNECT_TIMEOUT, server.connect())
        .await
        .context("sandbox.elevated_helper_connect_timeout: 管理员助手没有在限定时间内连上 Aura（管理员权限窗口可能还没确认）；请看“兼容环境”一项的状态，核对后再决定是否重试。")??;
    let helper_identity = validate_named_pipe_client(&server, helper.identity())
        .context("sandbox.elevated_helper_identity_rejected: 连上管道的程序不是 Aura 启动的管理员助手，已拒绝它；请看“兼容环境”一项的状态，核对后再决定是否重试。")?;

    let hello: ElevatedHelperHello = read_control_timeout(&mut server).await?;
    if hello.helper_process_id != helper_identity.process_id {
        bail!(
            "sandbox.elevated_helper_identity_rejected: 管理员助手报的进程号与连上管道的进程不一致，已拒绝它；请看“兼容环境”一项的状态，核对后再决定是否重试。"
        );
    }
    guard.accept_hello(&hello)?;
    write_control(&mut server, &ElevatedHelperChallenge { bootstrap_nonce: challenge_nonce })
        .await?;
    let proof: ElevatedHelperChallengeProof = read_control_timeout(&mut server).await?;
    guard.accept_challenge_proof(&proof)?;
    on_authenticated().context("sandbox.host_preparation_state_unsaved: 管理员助手已通过校验，但没能把操作记成正在应用，没有让它改动系统；请看“兼容环境”一项的状态，核对后再决定是否重试。")?;

    let command = ElevatedHelperCommand {
        protocol_version: ContractVersion::CURRENT,
        release_descriptor: options.release_descriptor.clone(),
        runtime_instance_id: options.runtime_instance_id,
        elevation_launch_ticket,
        plan_nonce,
        plan: plan.clone(),
    };
    write_control(&mut server, &command).await?;
    let terminal: ElevatedHelperTerminal = read_control_timeout(&mut server).await?;
    if terminal.protocol_version.major != ContractVersion::CURRENT.major
        || terminal.plan_hash != plan.plan_hash
        || terminal.result.operation_id != plan.operation_id
    {
        bail!(
            "sandbox.elevated_helper_result_mismatch: 管理员助手返回的结果与这次的方案对不上，不能确认系统是否已被修改；请点“修复兼容环境”重新检查。"
        );
    }
    if !terminal.result.succeeded {
        // Only a code from the closed pre-write set may downgrade "unknown" to
        // "refused, nothing written"; anything else stays a helper failure.
        if let Some(code) =
            terminal.result.error_code.as_deref().and_then(host_preparation_pre_write_refusal_code)
        {
            return Err(ElevatedHelperRefusal { code }.into());
        }
        bail!(
            "sandbox.elevated_helper_failed: 管理员助手报告失败，但不能证明它没有改动系统；请点“修复兼容环境”重新检查。"
        );
    }
    drop(helper);
    Ok(terminal.result)
}

/// Records the verified write outcome after the in-process/helper paths converge.
/// This is diagnostic evidence of the write, not proof that the operation's
/// persistence transition has committed. Failed or unobserved writes cannot supply it.
pub(crate) fn record_host_preparation_write_evidence(
    result: &HostPreparationBrokerResult,
    emit: impl FnOnce(&str, serde_json::Value),
) {
    if result.succeeded && result.approved_auto_inherited_marker_lost {
        emit(
            "host_preparation_approved_dacl_difference",
            serde_json::json!({
                "operation_id": result.operation_id,
                "difference": "se_dacl_auto_inherited_lost",
            }),
        );
    }
}

pub fn validate_active_release_identity(
    options: &ElevatedBrokerOptions,
    plan: &HostPreparationBrokerPlan,
) -> Result<()> {
    let manifest_path = fs::canonicalize(&options.component_manifest_path)
        .context("sandbox.release_manifest_unreadable: 找不到当前安装的 Aura 组件清单，没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。")?;
    let (active_path, active_bytes_before, active) = validate_active_manifest_path(&manifest_path)?;
    let manifest_bytes =
        fs::read(&manifest_path).context("sandbox.release_manifest_unreadable: 读不出当前安装的 Aura 组件清单，没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。")?;
    let actual_manifest_hash = ContentHash::new(
        ContentHashAlgorithm::Sha256,
        format!("{:x}", Sha256::digest(&manifest_bytes)),
    )?;
    if actual_manifest_hash != options.release_descriptor.identity.component_manifest_hash
        || actual_manifest_hash != plan.release_descriptor.identity.component_manifest_hash
        || actual_manifest_hash != active.release_descriptor.identity.component_manifest_hash
    {
        bail!(
            "sandbox.host_preparation_release_changed: 准备方案之后 Aura 的组件清单变了（可能刚更新过），这份方案作废，没有请求管理员权限，系统没有被修改；请重新开始这次操作。"
        );
    }
    let manifest: BuildComponentManifest =
        serde_json::from_slice(&manifest_bytes).context("sandbox.release_manifest_invalid: 当前安装的 Aura 组件清单格式不对，没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。")?;
    if manifest.bundle_namespace != options.release_descriptor.bundle_namespace
        || manifest.bundle_namespace != plan.release_descriptor.bundle_namespace
        || manifest.bundle_namespace != active.release_descriptor.bundle_namespace
        || manifest.release_bundle_id != options.release_descriptor.identity.release_bundle_id
        || manifest.release_bundle_id != plan.release_descriptor.identity.release_bundle_id
        || manifest.release_bundle_id != active.release_descriptor.identity.release_bundle_id
    {
        bail!(
            "sandbox.host_preparation_release_changed: 准备方案之后安装的 Aura 变了（可能刚更新过），这份方案作废，没有请求管理员权限，系统没有被修改；请重新开始这次操作。"
        );
    }
    let helper = manifest
        .components
        .iter()
        .find(|component| component.kind == BuildComponentKind::ElevatedHelper)
        .context("sandbox.elevated_helper_missing: 当前安装的 Aura 组件清单里没有管理员助手，没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。")?;
    if helper.content_hash != options.helper_content_hash {
        bail!(
            "sandbox.host_preparation_release_changed: Aura 启动时记下的管理员助手与当前安装的不一致（可能刚更新过），没有请求管理员权限，系统没有被修改；请重启 Aura 后再试。"
        );
    }
    let manifest_root =
        manifest_path.parent().context("sandbox.release_layout_invalid: Aura 组件清单的位置不对，没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。")?;
    let expected_helper_path = safe_release_path(manifest_root, &helper.relative_path)?;
    let expected_helper_path = fs::canonicalize(expected_helper_path)
        .context("sandbox.elevated_helper_missing: 找不到当前安装的管理员助手程序，没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。")?;
    if !expected_helper_path.starts_with(manifest_root) {
        bail!(
            "sandbox.elevated_helper_outside_release: 管理员助手程序不在当前安装的 Aura 目录里，没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。"
        );
    }
    let configured_helper_path = fs::canonicalize(&options.helper_executable)
        .context("sandbox.elevated_helper_missing: 找不到 Aura 启动时记下的管理员助手程序，没有请求管理员权限，系统没有被修改；请重启 Aura 后再试。")?;
    if expected_helper_path != configured_helper_path {
        bail!(
            "sandbox.host_preparation_release_changed: Aura 启动时记下的管理员助手不属于当前安装的版本（可能刚更新过），没有请求管理员权限，系统没有被修改；请重启 Aura 后再试。"
        );
    }
    let helper_bytes =
        fs::read(&configured_helper_path).context("sandbox.elevated_helper_unreadable: 读不出当前安装的管理员助手程序，没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。")?;
    let helper_hash = match helper.content_hash.algorithm {
        ContentHashAlgorithm::Sha256 => format!("{:x}", Sha256::digest(&helper_bytes)),
        ContentHashAlgorithm::Blake3 => blake3::hash(&helper_bytes).to_hex().to_string(),
    };
    if helper_hash != helper.content_hash.hex {
        bail!(
            "sandbox.elevated_helper_tampered: 管理员助手程序与组件清单记录的不一致（可能被改动或损坏），没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。"
        );
    }
    let manifest_bytes_after =
        fs::read(&manifest_path).context("sandbox.release_manifest_unreadable: 核对时没能再次读出 Aura 组件清单，没有请求管理员权限，系统没有被修改；请重试。")?;
    if manifest_bytes_after != manifest_bytes {
        bail!(
            "sandbox.host_preparation_release_changed: 核对期间 Aura 的组件清单变了（可能正在更新），没有请求管理员权限，系统没有被修改；等更新完成后重新开始这次操作。"
        );
    }
    let active_bytes_after =
        fs::read(&active_path).context("sandbox.release_metadata_unreadable: 核对时没能再次读出 Aura 的当前版本记录（active.json），没有请求管理员权限，系统没有被修改；请重试。")?;
    if active_bytes_after != active_bytes_before {
        bail!(
            "sandbox.host_preparation_release_changed: 核对期间 Aura 的当前版本记录变了（可能正在更新），没有请求管理员权限，系统没有被修改；等更新完成后重新开始这次操作。"
        );
    }
    Ok(())
}

fn validate_active_manifest_path(
    manifest_path: &Path,
) -> Result<(PathBuf, Vec<u8>, ActiveInstallState)> {
    let version_directory =
        manifest_path.parent().context("sandbox.release_layout_invalid: Aura 组件清单不在版本目录里，没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。")?;
    let versions_root =
        version_directory.parent().context("sandbox.release_layout_invalid: Aura 的版本目录不在 versions 文件夹里，没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。")?;
    if versions_root
        .file_name()
        .is_none_or(|name| !name.to_string_lossy().eq_ignore_ascii_case("versions"))
    {
        bail!(
            "sandbox.release_layout_invalid: Aura 组件清单不在安装目录的 versions 文件夹里（开发版或安装不完整），没有请求管理员权限，系统没有被修改；请用正式安装的 Aura 再试。"
        );
    }
    let install_root = versions_root.parent().context("sandbox.release_layout_invalid: Aura 的 versions 文件夹没有上级安装目录，没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。")?;
    let active_path = install_root.join("active.json");
    let active_bytes =
        fs::read(&active_path).context("sandbox.release_metadata_unreadable: 读不出 Aura 的当前版本记录（active.json），没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。")?;
    let active: ActiveInstallState =
        serde_json::from_slice(&active_bytes).context("sandbox.release_metadata_invalid: Aura 的当前版本记录（active.json）格式不对，没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。")?;
    if active.active.is_empty()
        || active.active == "."
        || active.active == ".."
        || active.active.contains('/')
        || active.active.contains('\\')
    {
        bail!(
            "sandbox.release_metadata_invalid: Aura 的当前版本记录（active.json）里的版本名不安全，没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。"
        );
    }
    let expected_version = fs::canonicalize(versions_root.join(&active.active))
        .context("sandbox.release_layout_invalid: 找不到当前版本记录指向的 Aura 版本目录，没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。")?;
    let actual_version = fs::canonicalize(version_directory)
        .context("sandbox.release_layout_invalid: 找不到 Aura 启动时所在的版本目录（可能已被更新清理），没有请求管理员权限，系统没有被修改；请重启 Aura 后再试。")?;
    if expected_version != actual_version {
        bail!(
            "sandbox.host_preparation_release_changed: 正在运行的 Aura 不是当前安装的版本（可能刚更新过），没有请求管理员权限，系统没有被修改；请重启 Aura 后再试。"
        );
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
        bail!(
            "sandbox.elevated_helper_outside_release: 组件清单里管理员助手的路径指向了 Aura 安装目录之外，没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。"
        );
    }
    let path = root.join(candidate);
    if !path.starts_with(root) {
        bail!(
            "sandbox.elevated_helper_outside_release: 组件清单里管理员助手的路径指向了 Aura 安装目录之外，没有请求管理员权限，系统没有被修改；请重新安装 Aura 后再试。"
        );
    }
    Ok(path)
}

async fn read_control_timeout<T: DeserializeOwned>(server: &mut NamedPipeServer) -> Result<T> {
    let frame = time::timeout(HELPER_PROTOCOL_TIMEOUT, read_frame(server))
        .await
        .context("sandbox.elevated_helper_timeout: 管理员助手没有在限定时间内答复；请看“兼容环境”一项的状态，核对后再决定是否重试。")??;
    let WireFrame::Control(payload) = frame else {
        bail!(
            "sandbox.elevated_helper_protocol_invalid: 管理员助手发来的消息格式不对，已停止与它通信；请看“兼容环境”一项的状态，核对后再决定是否重试。"
        );
    };
    serde_json::from_slice(&payload).context("sandbox.elevated_helper_protocol_invalid: 管理员助手发来的消息内容无法解析，已停止与它通信；请看“兼容环境”一项的状态，核对后再决定是否重试。")
}

async fn write_control<T: Serialize>(server: &mut NamedPipeServer, value: &T) -> Result<()> {
    let payload = serde_json::to_vec(value)?;
    write_frame(server, &WireFrame::Control(payload)).await?;
    Ok(())
}

fn lower_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(&mut value, "{byte:02x}").expect("writing to String is infallible");
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_domain_common::{HostPreparationOperationId, ReleaseBundleId, UtcTimestamp};
    use aura_permission_domain::{HostPreparationAction, HostPreparationOwnership};
    use aura_runtime_contract::{BundleNamespace, ReleaseIdentity};
    use aura_security_contract::CanonicalPath;
    use tempfile::tempdir;

    #[test]
    fn in_process_and_helper_results_preserve_the_same_write_evidence_once() {
        // Exercise the common consumer with a direct result and with the actual
        // helper terminal serde shape. No helper process or host ACL is involved.
        for via_helper in [false, true] {
            let expected = HostPreparationBrokerResult {
                operation_id: HostPreparationOperationId::new(),
                succeeded: true,
                ownership_after: HostPreparationOwnership::default(),
                error_code: None,
                approved_auto_inherited_marker_lost: true,
            };
            let result = if via_helper {
                let terminal = ElevatedHelperTerminal {
                    protocol_version: ContractVersion::CURRENT,
                    plan_hash: ContentHash::blake3(b"evidence plan"),
                    result: expected.clone(),
                };
                let bytes = serde_json::to_vec(&terminal).expect("helper terminal bytes");
                serde_json::from_slice::<ElevatedHelperTerminal>(&bytes)
                    .expect("Core reads helper terminal")
                    .result
            } else {
                expected.clone()
            };
            let mut events = Vec::new();
            record_host_preparation_write_evidence(&result, |event, fields| {
                events.push((event.to_owned(), fields));
            });
            assert_eq!(events.len(), 1, "helper={via_helper}");
            assert_eq!(events[0].0, "host_preparation_approved_dacl_difference");
            assert_eq!(
                events[0].1,
                serde_json::json!({
                    "operation_id": expected.operation_id,
                    "difference": "se_dacl_auto_inherited_lost",
                })
            );
            assert_eq!(result, expected, "recording does not change the execution outcome");
        }
    }

    #[test]
    fn unchanged_or_refused_results_do_not_claim_an_approved_write_difference() {
        for (succeeded, marker_lost) in [(true, false), (false, false), (false, true)] {
            let result = HostPreparationBrokerResult {
                operation_id: HostPreparationOperationId::new(),
                succeeded,
                ownership_after: HostPreparationOwnership::default(),
                error_code: (!succeeded)
                    .then(|| "sandbox.host_preparation_plan_invalid".to_owned()),
                approved_auto_inherited_marker_lost: marker_lost,
            };
            record_host_preparation_write_evidence(&result, |_, _| {
                panic!("a false/failed outcome must not emit successful write evidence");
            });
        }
    }

    #[test]
    fn elevated_pipe_randomness_is_exactly_128_bits_of_lowercase_hex() {
        let value = lower_hex(&[0x0a; 16]);
        assert_eq!(value.len(), 32);
        assert!(value.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f')));
    }

    #[test]
    fn active_release_validation_hashes_original_manifest_bytes_and_rejects_release_changes() {
        let install = tempdir().expect("temporary install root");
        let version_directory = install.path().join("versions").join("1.0.0");
        fs::create_dir_all(&version_directory).expect("active version directory");
        let helper_path = version_directory.join("aura-elevated-helper.exe");
        let helper_bytes = b"fixture elevated helper";
        fs::write(&helper_path, helper_bytes).expect("fixture helper");
        let helper_hash = ContentHash::new(
            ContentHashAlgorithm::Sha256,
            format!("{:x}", Sha256::digest(helper_bytes)),
        )
        .expect("helper hash");
        let release_bundle_id = ReleaseBundleId::new();
        let bundle_namespace: BundleNamespace =
            "aura.desktop.test.p04-broker".parse().expect("test namespace");
        let manifest_bytes = serde_json::to_vec_pretty(&serde_json::json!({
            "manifest_version": ContractVersion::CURRENT,
            "bundle_namespace": bundle_namespace,
            "release_bundle_id": release_bundle_id,
            "release_channel": "Nightly",
            "product_version": "1.0.0",
            "build_commit": "fixture",
            "target_triple": "x86_64-pc-windows-msvc",
            "components": [{
                "component_id": "aura-elevated-helper",
                "kind": "elevated_helper",
                "version": "1.0.0",
                "relative_path": "aura-elevated-helper.exe",
                "content_hash": helper_hash,
                "protocol_version": ContractVersion::CURRENT,
            }],
        }))
        .expect("fixture manifest");
        let manifest_path = version_directory.join("component-manifest.json");
        fs::write(&manifest_path, &manifest_bytes).expect("component manifest");
        let manifest_hash = ContentHash::new(
            ContentHashAlgorithm::Sha256,
            format!("{:x}", Sha256::digest(&manifest_bytes)),
        )
        .expect("manifest hash");
        let release_descriptor = ReleaseDescriptor {
            bundle_namespace: bundle_namespace.clone(),
            identity: ReleaseIdentity {
                release_bundle_id,
                component_manifest_hash: manifest_hash.clone(),
            },
        };
        fs::write(
            install.path().join("active.json"),
            serde_json::to_vec(&ActiveInstallState {
                active: "1.0.0".to_owned(),
                release_descriptor: release_descriptor.clone(),
                previous: None,
                pending: None,
                last_known_good: None,
                data_location: None,
            })
            .expect("active metadata"),
        )
        .expect("active metadata");
        let runtime_instance_id = RuntimeInstanceId::new();
        let options = ElevatedBrokerOptions {
            helper_executable: helper_path,
            helper_content_hash: helper_hash,
            component_manifest_path: manifest_path.clone(),
            release_descriptor: release_descriptor.clone(),
            runtime_instance_id,
        };
        let plan = HostPreparationBrokerPlan {
            operation_id: HostPreparationOperationId::new(),
            action: HostPreparationAction::Prepare,
            host_preparation_revision: 1,
            runtime_instance_id,
            release_descriptor: release_descriptor.clone(),
            plan_nonce_hash: ContentHash::blake3(b"fixture nonce"),
            expires_at: UtcTimestamp::now(),
            system_drive_root: CanonicalPath {
                display: r"C:\".to_owned(),
                utf16: r"C:\".encode_utf16().collect(),
            },
            ownership_before: HostPreparationOwnership {
                system_drive_root: None,
                all_application_packages_ace_owned: false,
                all_restricted_application_packages_ace_owned: false,
                original_null_device_sddl: None,
            },
            add_all_application_packages_ace: true,
            add_all_restricted_application_packages_ace: true,
            update_null_device_security: true,
            plan_hash: ContentHash::blake3(b"fixture plan"),
        };

        validate_active_release_identity(&options, &plan).expect("valid active release");

        let mut reformatted = manifest_bytes.clone();
        reformatted.push(b'\n');
        fs::write(&manifest_path, reformatted).expect("reformatted manifest");
        assert!(validate_active_release_identity(&options, &plan).is_err());

        fs::write(&manifest_path, manifest_bytes).expect("restore manifest");
        fs::create_dir_all(install.path().join("versions").join("2.0.0"))
            .expect("replacement version");
        fs::write(
            install.path().join("active.json"),
            serde_json::to_vec(&ActiveInstallState {
                active: "2.0.0".to_owned(),
                release_descriptor,
                previous: Some("1.0.0".to_owned()),
                pending: None,
                last_known_good: Some("1.0.0".to_owned()),
                data_location: None,
            })
            .expect("replacement active metadata"),
        )
        .expect("replacement active metadata");
        assert!(validate_active_release_identity(&options, &plan).is_err());
    }
}
