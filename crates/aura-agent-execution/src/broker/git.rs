//! The backend descriptor of authenticated Git reads.
use aura_security_contract::ResolvedCapability;

pub fn descriptor(
    runtime: aura_domain_common::RuntimeInstanceId,
) -> aura_execution_contract::BackendDescriptor {
    let mut descriptor = super::descriptor(runtime);
    descriptor.id = aura_execution_contract::BackendId("native-git-broker-v1".into());
    descriptor.capabilities = [ResolvedCapability::GitRead].into();
    // AuthorizedWritesOnly + ReadOnlyUserFiles: duplicated Core token at Low IL;
    // worktree/git-dir/common-dir must be above Low and enforce no-write-up.
    // ChildProcessControl: creation-time membership in a kill-on-close Job with
    // ActiveProcessLimit=1. BoundedOutput: the adapter's combined stream quota.
    // AuthorizedReadsOnly + CredentialIsolation are construction guarantees, as
    // with the retired Core reader: Core fixes the read argv after repository
    // identification/authorization, protocols are disabled and no child (including
    // helpers, hooks, textconv or filters) can start. No credential is materialized.
    descriptor.guarantees = [
        aura_security_contract::EnforcementGuarantee::AuthorizedReadsOnly,
        aura_security_contract::EnforcementGuarantee::AuthorizedWritesOnly,
        aura_security_contract::EnforcementGuarantee::ReadOnlyUserFiles,
        aura_security_contract::EnforcementGuarantee::CredentialIsolation,
        aura_security_contract::EnforcementGuarantee::ChildProcessControl,
        aura_security_contract::EnforcementGuarantee::BoundedOutput,
    ]
    .into();
    descriptor.protocol_fingerprint =
        aura_domain_common::jcs::canonical_sha256(&"aura.native-git-broker.v1").expect("literal");
    descriptor
}
