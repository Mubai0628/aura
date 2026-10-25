use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::BundleNamespace;
use aura_domain_common::{ContentHash, ContractVersion, ReleaseBundleId};

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
pub enum ReleaseChannel {
    Stable,
    Preview,
    Nightly,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuildComponentKind {
    Launcher,
    Setup,
    Desktop,
    MainPipeBridge,
    Core,
    SandboxHost,
    EgressProxy,
    ElevatedHelper,
    WebRuntime,
    Chromium,
    PluginHost,
    ChromeNativeHost,
    PromptBundle,
    DatabaseSchema,
    IpcProtocol,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildComponent {
    pub component_id: String,
    pub kind: BuildComponentKind,
    pub version: String,
    pub relative_path: String,
    pub content_hash: ContentHash,
    pub protocol_version: Option<ContractVersion>,
}

#[derive(Clone, Debug, Deserialize, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BuildComponentManifest {
    pub manifest_version: ContractVersion,
    pub bundle_namespace: BundleNamespace,
    pub release_bundle_id: ReleaseBundleId,
    pub release_channel: ReleaseChannel,
    pub product_version: String,
    pub build_commit: String,
    pub target_triple: String,
    pub components: Vec<BuildComponent>,
}
