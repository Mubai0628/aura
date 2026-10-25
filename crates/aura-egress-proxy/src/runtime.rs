use std::{collections::BTreeMap, env, fs, path::PathBuf, str::FromStr};

use anyhow::{Context, Result, bail};
use aura_domain_common::{ContentHash, ContentHashAlgorithm, ContractVersion, RuntimeInstanceId};
use aura_ipc::{SidecarBootstrapConfig, connect_authenticated_sidecar};
use aura_runtime_contract::{
    BuildComponent, BuildComponentKind, BuildComponentManifest, ReleaseDescriptor, ReleaseIdentity,
    RuntimeHandshake, RuntimeProcessKind,
};
use aura_windows_platform::{
    read_inherited_bootstrap_secret, snapshot_process, validate_named_pipe_server,
};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

struct Options {
    pipe_name: String,
    parent_process_id: u32,
    parent_runtime_instance_id: RuntimeInstanceId,
    runtime_instance_id: RuntimeInstanceId,
    component_manifest_path: PathBuf,
    allow_dev_component_mismatch: bool,
    bootstrap_handle: usize,
}

struct SidecarReleaseContext {
    release_descriptor: ReleaseDescriptor,
    component_version: String,
    build_commit: String,
}

pub async fn run() -> Result<()> {
    let options = Options::parse(env::args())?;
    let release = load_release_identity(&options)?;
    options.validate_pipe_scope(&release.release_descriptor.bundle_namespace)?;
    let expected_parent = snapshot_process(options.parent_process_id)
        .context("failed to snapshot expected Core process")?;
    let current =
        snapshot_process(std::process::id()).context("failed to snapshot Egress Proxy process")?;
    let mut bootstrap_secret = read_inherited_bootstrap_secret(options.bootstrap_handle)
        .context("failed to consume inherited bootstrap secret")?;
    let handshake = RuntimeHandshake {
        protocol_version: ContractVersion::CURRENT,
        process_kind: RuntimeProcessKind::EgressProxy,
        component_version: release.component_version,
        build_commit: release.build_commit,
        release_descriptor: release.release_descriptor,
        runtime_instance_id: options.runtime_instance_id,
        parent_runtime_instance_id: Some(options.parent_runtime_instance_id),
        supported_features: vec![
            "aura.egress-proxy.v1".to_owned(),
            "aura.egress.logical-credential.v1".to_owned(),
            "aura.egress.proxy-dns-ttl.v1".to_owned(),
        ],
    };
    let config = SidecarBootstrapConfig {
        pipe_name: options.pipe_name,
        handshake,
        process_id: current.process_id,
        process_creation_time_100ns: current.creation_time_100ns,
        bootstrap_secret,
    };
    bootstrap_secret.zeroize();
    let (pipe, _) = connect_authenticated_sidecar(config, |client| {
        validate_named_pipe_server(client, &expected_parent).map(|_| ())
    })
    .await
    .context("Egress Proxy authentication failed")?;
    super::runtime_commands::serve(pipe).await
}

impl Options {
    fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut values = BTreeMap::<String, String>::new();
        let mut allow_dev_component_mismatch = false;
        for argument in arguments.into_iter().skip(1) {
            if argument == "--allow-dev-component-mismatch" {
                allow_dev_component_mismatch = true;
                continue;
            }
            let Some((name, value)) =
                argument.strip_prefix("--").and_then(|value| value.split_once('='))
            else {
                bail!("unsupported aura-egress-proxy argument")
            };
            if values.insert(name.to_owned(), value.to_owned()).is_some() {
                bail!("duplicate aura-egress-proxy argument: {name}");
            }
        }
        let take =
            |name: &str| values.get(name).cloned().with_context(|| format!("missing --{name}"));
        let options = Self {
            pipe_name: take("pipe-name")?,
            parent_process_id: take("parent-pid")?.parse().context("invalid --parent-pid")?,
            parent_runtime_instance_id: RuntimeInstanceId::from_str(&take(
                "parent-runtime-instance-id",
            )?)
            .context("invalid --parent-runtime-instance-id")?,
            runtime_instance_id: RuntimeInstanceId::from_str(&take("runtime-instance-id")?)
                .context("invalid --runtime-instance-id")?,
            component_manifest_path: PathBuf::from(take("component-manifest")?),
            allow_dev_component_mismatch,
            bootstrap_handle: take("bootstrap-handle")?
                .parse()
                .context("invalid --bootstrap-handle")?,
        };
        options.validate()?;
        Ok(options)
    }

    fn validate(&self) -> Result<()> {
        if self.parent_process_id == 0 || self.bootstrap_handle == 0 {
            bail!("parent PID and bootstrap handle must be non-zero");
        }
        Ok(())
    }

    fn validate_pipe_scope(
        &self,
        namespace: &aura_runtime_contract::BundleNamespace,
    ) -> Result<()> {
        let prefix = format!(
            r"\\.\pipe\Aura\{}\{}\sidecar\{}\",
            namespace.object_name_hash(),
            self.parent_runtime_instance_id,
            self.runtime_instance_id
        );
        let suffix = self.pipe_name.strip_prefix(&prefix).context("sidecar pipe scope mismatch")?;
        if suffix.len() != 32
            || !suffix.bytes().all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
        {
            bail!("sidecar pipe must end with 128 bits of lowercase hex randomness");
        }
        Ok(())
    }
}

fn load_release_identity(options: &Options) -> Result<SidecarReleaseContext> {
    let manifest_path = fs::canonicalize(&options.component_manifest_path)
        .context("failed to canonicalize component manifest")?;
    let bytes = fs::read(&manifest_path).context("failed to read component manifest")?;
    let manifest: BuildComponentManifest =
        serde_json::from_slice(&bytes).context("invalid component manifest")?;
    let component = manifest
        .components
        .iter()
        .find(|component| component.kind == BuildComponentKind::EgressProxy)
        .context("component manifest has no Egress Proxy")?;
    verify_current_component(component, options.allow_dev_component_mismatch)?;
    if manifest.bundle_namespace.trust_domain().is_none() {
        bail!("unsupported Aura release bundle namespace");
    }
    Ok(SidecarReleaseContext {
        release_descriptor: ReleaseDescriptor {
            bundle_namespace: manifest.bundle_namespace,
            identity: ReleaseIdentity {
                release_bundle_id: manifest.release_bundle_id,
                component_manifest_hash: ContentHash::new(
                    ContentHashAlgorithm::Sha256,
                    format!("{:x}", Sha256::digest(&bytes)),
                )?,
            },
        },
        component_version: component.version.clone(),
        build_commit: manifest.build_commit,
    })
}

fn verify_current_component(component: &BuildComponent, allow_mismatch: bool) -> Result<()> {
    if component.protocol_version.is_some_and(|version| {
        version.major != ContractVersion::CURRENT.major
            || version.minor > ContractVersion::CURRENT.minor
    }) {
        bail!("Egress Proxy manifest protocol is incompatible");
    }
    let executable = env::current_exe().context("failed to resolve aura-egress-proxy.exe")?;
    let bytes = fs::read(&executable).context("failed to read aura-egress-proxy.exe")?;
    let actual = match component.content_hash.algorithm {
        ContentHashAlgorithm::Sha256 => format!("{:x}", Sha256::digest(&bytes)),
        ContentHashAlgorithm::Blake3 => blake3::hash(&bytes).to_hex().to_string(),
    };
    if actual != component.content_hash.hex && !allow_mismatch {
        bail!("aura-egress-proxy.exe does not match the Release Bundle manifest");
    }
    Ok(())
}
