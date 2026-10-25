#![forbid(unsafe_code)]

mod authorization;
pub use authorization::*;
mod network_consent;
pub use network_consent::*;
mod policy;
pub use policy::*;

mod identity;
pub use identity::{InvocationActor, InvocationIdentity};
mod browser;
pub use browser::{
    browser_mode_covers, browser_navigation, browser_site, browser_tool, built_in_browser,
    network_caller_for,
};
mod git_native;
mod native_session;
mod shell_consent;
pub use git_native::{built_in_native_git, native_git_mode_covers};
mod mcp;
pub use mcp::{McpTurnConsentRef, mcp_consent_covers, mcp_server_call};
mod memory;
pub use memory::{
    MemorySessionConsentRef, MemoryVaultScope, built_in_memory, has_memory_authority,
    memory_consent_covers,
};
pub use native_session::{
    NativeSessionConsentScope, NativeShell, backend_class_can_hold_a_session_consent,
    native_session_consent_scope,
};
pub use shell_consent::{ShellConsentLifetime, built_in_shell, shell_consent_covers_capability};
mod file_consent;
pub use file_consent::{
    FileConsentProtection, FileConsentTarget, FileSessionConsentRef,
    file_capabilities_are_consentable, file_consent_capabilities, file_consent_target_covers,
    file_consent_target_of, file_entry_key, file_object_key,
};
mod permission;
pub use permission::{
    CanonicalPath, EnvironmentVariableScope, OperationRiskFlag,
    PermissionAllowAuthorizationBasisV1, PermissionGrantLifetime, PermissionMode,
    PermissionSnapshot, PrivateNetworkClass, ResolvedCapability,
};
mod approval;
pub use approval::{ApprovalDecisionKind, ApprovalState};
mod network;
pub use network::{
    MODEL_EGRESS_POLICY_VERSION_UNSUPPORTED_CODE, ModelChannelDataDisposition,
    ModelEgressPolicyVersion, ModelEgressPolicyVersionUnsupported, NetworkCallerKind,
    NetworkDataClass, NetworkDestinationClass, ResolvedNetworkDestination,
};

mod resource;
mod source;
mod trust;
pub use resource::ResourceDescriptor;
pub use source::SecuritySourceRef;
pub use trust::TrustState;
