//! The execution ledger: `BrokerJournal`, the process's one V2 write authority, with its
//! background store check; the owner of every dispatch's post-execution tail; and the port a
//! Core Broker dispatch runs its steps through.
pub mod completion;
pub mod journal;
mod store_check;

pub mod port;
pub use journal::BrokerJournal;

use aura_domain_common::Revision;
use aura_domain_common::jcs::canonical_sha256;
use aura_execution_contract::*;
use aura_runtime_contract::IpcErrorEnvelope;
use aura_security_contract::*;

/// The owned pre-state reader a route hands the port.
///
/// Owned rather than borrowed because the call runs on the bounded synchronous boundary: the
/// inputs a step needs are built by the route that knows them, so nothing has to be
/// reconstructed on the blocking thread. It is a named type so every route declares the same
/// contract instead of repeating it.
pub type OwnedInspect = std::sync::Arc<dyn Fn() -> anyhow::Result<serde_json::Value> + Send + Sync>;

/// The owned post-state and evidence reader a route hands the port.
pub type OwnedEvidence<T> = std::sync::Arc<
    dyn Fn(&DispatchRequest, &Result<T, IpcErrorEnvelope>) -> anyhow::Result<BrokerEvidence>
        + Send
        + Sync,
>;

pub fn descriptor(runtime_instance_id: aura_domain_common::RuntimeInstanceId) -> BackendDescriptor {
    use ResolvedCapability::*;
    BackendDescriptor {
        known_limitations: Default::default(),
        id: BackendId("core-broker".into()),
        revision: Revision::INITIAL,
        runtime_instance_id,
        class: BackendClass::CoreBroker,
        readiness: BackendReadiness::Ready,
        capabilities: [
            WorkspaceFileRead,
            // A path the existing policy allowed directly is an external ordinary file,
            // so the backend that performs the operation has to declare that it can
            // serve it — otherwise the operation is classified, authorized, and then
            // finds no backend at all.
            ExternalFileRead,
            WorkspaceFileWrite,
            WorkspaceFileCreate,
            WorkspaceFileDelete,
            WorkspaceFileMove,
            ExternalFileWrite,
            ExternalFileCreate,
            ConfigurationRead,
            ConfigurationWrite,
            ToolRegistryRead,
            ToolEphemeralExposureWrite,
            GitRead,
            PublicNetworkRead,
            OrdinaryNetwork,
            AgentTaskRead,
        ]
        .into(),
        guarantees: [
            EnforcementGuarantee::AuthorizedReadsOnly,
            EnforcementGuarantee::AuthorizedWritesOnly,
            EnforcementGuarantee::CredentialIsolation,
            EnforcementGuarantee::ExactNetworkOrigins,
        ]
        .into(),
        compatibility: Default::default(),
        protocol_fingerprint: canonical_sha256(&"aura.core-broker.v1")
            .expect("literal protocol hash"),
    }
}
