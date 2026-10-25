use aura_domain_common::{
    AttachmentGrantId, BrowserShareId, ContentHash, ContextProjectionSnapshotId, ExternalGrantId,
    Revision, canonical_json_v1_sha256,
};
use aura_security_contract::{
    ModelChannelDataDisposition, ModelEgressPolicyVersion, NetworkDataClass,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::ContextSourceRef;

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum SourceAuthorizationRef {
    ExternalGrant { external_grant_id: ExternalGrantId },
    BrowserShare { browser_share_id: BrowserShareId },
    AttachmentGrant { attachment_grant_id: AttachmentGrantId },
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceAuthorizationBinding {
    pub authorization: SourceAuthorizationRef,
    pub authorization_revision: Revision,
    pub binding_fingerprint: ContentHash,
}

impl SourceAuthorizationBinding {
    #[must_use]
    pub fn canonical_key(&self) -> String {
        let reference = match &self.authorization {
            SourceAuthorizationRef::ExternalGrant { external_grant_id } => {
                format!("external_grant:{external_grant_id}")
            }
            SourceAuthorizationRef::BrowserShare { browser_share_id } => {
                format!("browser_share:{browser_share_id}")
            }
            SourceAuthorizationRef::AttachmentGrant { attachment_grant_id } => {
                format!("attachment_grant:{attachment_grant_id}")
            }
        };
        format!("{reference}:{}:{}", self.authorization_revision, self.binding_fingerprint)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelEgressAuthorizationBasis {
    ModelChannelPolicy,
    UserSubmission,
    SourceGrants { bindings: Vec<SourceAuthorizationBinding> },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelEgressSourceKind {
    AuraCore,
    CustomSystemAddendum,
    UserEvent,
    AssistantEvent,
    ToolResult,
    ToolSchema,
    WorkspaceFile,
    ExternalFileText,
    ExternalFileAttachment,
    ArtifactAttachment,
    BrowserText,
    BrowserAttachment,
    DiagnosticAttachment,
    Skill,
    Hook,
    TaskLineage,
    Compaction,
    /// Bounded facts read by Core from its completed turn and the two authorized
    /// Memory Vaults. Private auxiliary Model evidence, never a file/browser
    /// attachment grant, a new user message, or an instruction authority.
    MemoryMaintenance,
    /// The run-information activity fragment.
    ///
    /// It is its own source kind, not `AuraCore`, because the fact it carries comes from
    /// **other executors** - Main, the buffer store, P04, a Shell's managed background - and
    /// relabelling those as Core-owned would hide their real origin from the manifest, the
    /// Inspector and the compaction source filter. That distinction is what lets compaction
    /// exclude this fragment by name while keeping the control facts it does need.
    ActivityIndex,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEgressItem {
    pub ordinal: u32,
    pub source_ref: ContextSourceRef,
    pub source_kind: ModelEgressSourceKind,
    pub data_class: NetworkDataClass,
    pub content_hash: ContentHash,
    pub byte_length: u64,
    pub authorization_basis: ModelEgressAuthorizationBasis,
}

#[derive(Clone, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEgressManifestV1 {
    pub policy_version: ModelEgressPolicyVersion,
    pub context_snapshot_id: ContextProjectionSnapshotId,
    pub items: Vec<ModelEgressItem>,
    pub manifest_hash: ContentHash,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ModelEgressContractError {
    #[error("model_egress.policy_version_unsupported")]
    PolicyVersionUnsupported,
    #[error("model_egress.unclassified_content")]
    UnclassifiedContent,
    #[error("model_egress.always_denied")]
    AlwaysDenied,
    #[error("model_egress.item_order_invalid")]
    ItemOrderInvalid,
    #[error("model_egress.authorization_basis_invalid")]
    AuthorizationBasisInvalid,
    #[error("model_egress.source_grants_empty")]
    SourceGrantsEmpty,
    #[error("model_egress.source_grants_not_canonical")]
    SourceGrantsNotCanonical,
    #[error("model_egress.source_grants_incomplete")]
    SourceGrantsIncomplete,
    #[error("model_egress.manifest_hash_mismatch")]
    ManifestHashMismatch,
    #[error("model_egress.canonical_serialization_failed")]
    CanonicalSerialization,
}

impl ModelEgressManifestV1 {
    pub fn new(
        context_snapshot_id: ContextProjectionSnapshotId,
        items: Vec<ModelEgressItem>,
    ) -> Result<Self, ModelEgressContractError> {
        let mut manifest = Self {
            policy_version: ModelEgressPolicyVersion::CURRENT,
            context_snapshot_id,
            items,
            manifest_hash: ContentHash::sha256(b"pending"),
        };
        manifest.validate_items()?;
        manifest.manifest_hash = manifest.canonical_hash()?;
        Ok(manifest)
    }

    pub fn validate_contract_shape(&self) -> Result<(), ModelEgressContractError> {
        if !self.policy_version.is_supported() {
            return Err(ModelEgressContractError::PolicyVersionUnsupported);
        }
        self.validate_items()?;
        if self.manifest_hash != self.canonical_hash()? {
            return Err(ModelEgressContractError::ManifestHashMismatch);
        }
        Ok(())
    }

    fn validate_items(&self) -> Result<(), ModelEgressContractError> {
        for (index, item) in self.items.iter().enumerate() {
            let expected =
                u32::try_from(index + 1).map_err(|_| ModelEgressContractError::ItemOrderInvalid)?;
            if item.ordinal != expected {
                return Err(ModelEgressContractError::ItemOrderInvalid);
            }
            validate_item(self.policy_version, item)?;
        }
        Ok(())
    }

    fn canonical_hash(&self) -> Result<ContentHash, ModelEgressContractError> {
        #[derive(Serialize)]
        struct HashInput<'a> {
            policy_version: ModelEgressPolicyVersion,
            context_snapshot_id: ContextProjectionSnapshotId,
            items: &'a [ModelEgressItem],
        }
        canonical_json_v1_sha256(&HashInput {
            policy_version: self.policy_version,
            context_snapshot_id: self.context_snapshot_id,
            items: &self.items,
        })
        .map_err(|_| ModelEgressContractError::CanonicalSerialization)
    }
}

fn validate_item(
    policy_version: ModelEgressPolicyVersion,
    item: &ModelEgressItem,
) -> Result<(), ModelEgressContractError> {
    let disposition = item
        .data_class
        .model_channel_disposition(policy_version)
        .map_err(|_| ModelEgressContractError::PolicyVersionUnsupported)?;
    if disposition == ModelChannelDataDisposition::AlwaysDenied {
        return Err(ModelEgressContractError::AlwaysDenied);
    }
    // Source access is independent of egress classification. In V2, a file or
    // browser attachment cannot lose its read/share binding by being classified
    // as ordinary model content. Historical V1 manifests retain their meaning.
    if policy_version == ModelEgressPolicyVersion::V2
        && !required_binding_kinds(item.source_kind, disposition).is_empty()
        && !matches!(item.authorization_basis, ModelEgressAuthorizationBasis::SourceGrants { .. })
    {
        return Err(ModelEgressContractError::SourceGrantsIncomplete);
    }
    match &item.authorization_basis {
        ModelEgressAuthorizationBasis::ModelChannelPolicy => {
            if disposition != ModelChannelDataDisposition::AllowedByModelChannel
                || item.source_kind == ModelEgressSourceKind::UserEvent
            {
                return Err(ModelEgressContractError::AuthorizationBasisInvalid);
            }
        }
        ModelEgressAuthorizationBasis::UserSubmission => {
            if item.source_kind != ModelEgressSourceKind::UserEvent {
                return Err(ModelEgressContractError::AuthorizationBasisInvalid);
            }
        }
        ModelEgressAuthorizationBasis::SourceGrants { bindings } => {
            validate_bindings(bindings)?;
            let required = required_binding_kinds(item.source_kind, disposition);
            if required
                .iter()
                .any(|required| !bindings.iter().any(|binding| binding_kind(binding) == *required))
            {
                return Err(ModelEgressContractError::SourceGrantsIncomplete);
            }
        }
    }
    Ok(())
}

fn validate_bindings(
    bindings: &[SourceAuthorizationBinding],
) -> Result<(), ModelEgressContractError> {
    if bindings.is_empty() {
        return Err(ModelEgressContractError::SourceGrantsEmpty);
    }
    if !bindings.windows(2).all(|pair| pair[0].canonical_key() < pair[1].canonical_key()) {
        return Err(ModelEgressContractError::SourceGrantsNotCanonical);
    }
    Ok(())
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum BindingKind {
    External,
    Browser,
    Attachment,
}

fn binding_kind(binding: &SourceAuthorizationBinding) -> BindingKind {
    match binding.authorization {
        SourceAuthorizationRef::ExternalGrant { .. } => BindingKind::External,
        SourceAuthorizationRef::BrowserShare { .. } => BindingKind::Browser,
        SourceAuthorizationRef::AttachmentGrant { .. } => BindingKind::Attachment,
    }
}

fn required_binding_kinds(
    source_kind: ModelEgressSourceKind,
    disposition: ModelChannelDataDisposition,
) -> &'static [BindingKind] {
    use BindingKind::{Attachment, Browser, External};
    match source_kind {
        ModelEgressSourceKind::ExternalFileText => &[External],
        ModelEgressSourceKind::ExternalFileAttachment => &[External, Attachment],
        ModelEgressSourceKind::ArtifactAttachment | ModelEgressSourceKind::DiagnosticAttachment => {
            &[Attachment]
        }
        ModelEgressSourceKind::BrowserText => &[Browser],
        ModelEgressSourceKind::BrowserAttachment => &[Browser, Attachment],
        _ if disposition == ModelChannelDataDisposition::RequiresSourceEgressGrant => &[Attachment],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use aura_domain_common::{
        AttachmentGrantId, ContentHash, ContextProjectionSnapshotId, EventId, ExternalGrantId,
        Revision,
    };
    use aura_security_contract::{ModelEgressPolicyVersion, NetworkDataClass};

    use super::{
        ContextSourceRef, ModelEgressAuthorizationBasis, ModelEgressContractError, ModelEgressItem,
        ModelEgressManifestV1, ModelEgressSourceKind, SourceAuthorizationBinding,
        SourceAuthorizationRef,
    };
    #[test]
    fn maintenance_facts_use_model_channel_without_borrowing_attachment_or_secret_authority() {
        let draft = item(
            ModelEgressSourceKind::MemoryMaintenance,
            NetworkDataClass::WorkspaceSource,
            ModelEgressAuthorizationBasis::ModelChannelPolicy,
        );
        assert!(
            ModelEgressManifestV1::new(ContextProjectionSnapshotId::new(), vec![draft.clone()])
                .is_ok()
        );
        let mut attachment = draft.clone();
        attachment.source_kind = ModelEgressSourceKind::ArtifactAttachment;
        assert_eq!(
            ModelEgressManifestV1::new(ContextProjectionSnapshotId::new(), vec![attachment])
                .expect_err("attachments require their own grant"),
            ModelEgressContractError::SourceGrantsIncomplete
        );
        let mut secret = draft;
        secret.data_class = NetworkDataClass::CredentialMaterial;
        assert!(
            ModelEgressManifestV1::new(ContextProjectionSnapshotId::new(), vec![secret]).is_err()
        );
    }

    fn source_event() -> ContextSourceRef {
        ContextSourceRef::Event { event_id: EventId::new() }
    }

    fn binding(authorization: SourceAuthorizationRef) -> SourceAuthorizationBinding {
        SourceAuthorizationBinding {
            authorization,
            authorization_revision: Revision::INITIAL,
            binding_fingerprint: ContentHash::sha256(b"binding"),
        }
    }

    fn item(
        source_kind: ModelEgressSourceKind,
        data_class: NetworkDataClass,
        authorization_basis: ModelEgressAuthorizationBasis,
    ) -> ModelEgressItem {
        ModelEgressItem {
            ordinal: 1,
            source_ref: source_event(),
            source_kind,
            data_class,
            content_hash: ContentHash::sha256(b"content"),
            byte_length: 7,
            authorization_basis,
        }
    }

    fn manifest(item: ModelEgressItem) -> Result<ModelEgressManifestV1, ModelEgressContractError> {
        ModelEgressManifestV1::new(ContextProjectionSnapshotId::new(), vec![item])
    }

    #[test]
    fn model_channel_policy_is_explicit_and_does_not_authorize_user_events() {
        assert!(
            manifest(item(
                ModelEgressSourceKind::AuraCore,
                NetworkDataClass::PublicContent,
                ModelEgressAuthorizationBasis::ModelChannelPolicy,
            ))
            .is_ok()
        );
        assert_eq!(
            manifest(item(
                ModelEgressSourceKind::UserEvent,
                NetworkDataClass::PublicQuery,
                ModelEgressAuthorizationBasis::ModelChannelPolicy,
            )),
            Err(ModelEgressContractError::AuthorizationBasisInvalid)
        );
    }

    #[test]
    fn v2_normal_context_has_no_content_origin_approval_but_preserves_source_access() {
        for class in [
            NetworkDataClass::WorkspaceSource,
            NetworkDataClass::UserFileContent,
            NetworkDataClass::PersonalData,
            NetworkDataClass::DiagnosticData,
            NetworkDataClass::Unknown,
            NetworkDataClass::SuspectedSecret,
        ] {
            let current = manifest(item(
                ModelEgressSourceKind::WorkspaceFile,
                class,
                ModelEgressAuthorizationBasis::ModelChannelPolicy,
            ))
            .expect("normal model context");
            assert_eq!(current.policy_version, ModelEgressPolicyVersion::V2);
            for kind in [
                ModelEgressSourceKind::ExternalFileText,
                ModelEgressSourceKind::ExternalFileAttachment,
                ModelEgressSourceKind::BrowserText,
                ModelEgressSourceKind::BrowserAttachment,
                ModelEgressSourceKind::ArtifactAttachment,
            ] {
                assert_eq!(
                    manifest(item(kind, class, ModelEgressAuthorizationBasis::ModelChannelPolicy)),
                    Err(ModelEgressContractError::SourceGrantsIncomplete)
                );
            }
        }
        for class in [
            NetworkDataClass::CredentialMaterial,
            NetworkDataClass::SecurityInput,
            NetworkDataClass::Cookie,
            NetworkDataClass::PrivateKey,
        ] {
            assert_eq!(
                manifest(item(
                    ModelEgressSourceKind::AuraCore,
                    class,
                    ModelEgressAuthorizationBasis::ModelChannelPolicy
                )),
                Err(ModelEgressContractError::AlwaysDenied)
            );
        }
        let mut historical = manifest(item(
            ModelEgressSourceKind::AuraCore,
            NetworkDataClass::PublicContent,
            ModelEgressAuthorizationBasis::ModelChannelPolicy,
        ))
        .expect("base");
        historical.policy_version = ModelEgressPolicyVersion::V1;
        historical.manifest_hash = historical.canonical_hash().expect("historical hash");
        historical.validate_contract_shape().expect("V1 remains interpretable");
        historical.items[0].data_class = NetworkDataClass::UserFileContent;
        assert_eq!(
            historical.validate_contract_shape(),
            Err(ModelEgressContractError::AuthorizationBasisInvalid)
        );
    }

    #[test]
    fn user_submission_is_reserved_for_user_events_and_never_bypasses_always_denied() {
        assert!(
            manifest(item(
                ModelEgressSourceKind::UserEvent,
                NetworkDataClass::PersonalData,
                ModelEgressAuthorizationBasis::UserSubmission,
            ))
            .is_ok()
        );
        assert_eq!(
            manifest(item(
                ModelEgressSourceKind::AssistantEvent,
                NetworkDataClass::PublicContent,
                ModelEgressAuthorizationBasis::UserSubmission,
            )),
            Err(ModelEgressContractError::AuthorizationBasisInvalid)
        );
        assert_eq!(
            manifest(item(
                ModelEgressSourceKind::UserEvent,
                NetworkDataClass::CredentialMaterial,
                ModelEgressAuthorizationBasis::UserSubmission,
            )),
            Err(ModelEgressContractError::AlwaysDenied)
        );
    }

    #[test]
    fn source_grants_require_non_empty_canonical_complete_bindings() {
        let attachment = binding(SourceAuthorizationRef::AttachmentGrant {
            attachment_grant_id: AttachmentGrantId::new(),
        });
        let external = binding(SourceAuthorizationRef::ExternalGrant {
            external_grant_id: ExternalGrantId::new(),
        });

        assert_eq!(
            manifest(item(
                ModelEgressSourceKind::ExternalFileAttachment,
                NetworkDataClass::UserFileContent,
                ModelEgressAuthorizationBasis::SourceGrants { bindings: Vec::new() },
            )),
            Err(ModelEgressContractError::SourceGrantsEmpty)
        );
        assert_eq!(
            manifest(item(
                ModelEgressSourceKind::ExternalFileAttachment,
                NetworkDataClass::UserFileContent,
                ModelEgressAuthorizationBasis::SourceGrants {
                    bindings: vec![external.clone(), attachment.clone()],
                },
            )),
            Err(ModelEgressContractError::SourceGrantsNotCanonical)
        );
        assert_eq!(
            manifest(item(
                ModelEgressSourceKind::ExternalFileAttachment,
                NetworkDataClass::UserFileContent,
                ModelEgressAuthorizationBasis::SourceGrants {
                    bindings: vec![attachment.clone(), attachment.clone()],
                },
            )),
            Err(ModelEgressContractError::SourceGrantsNotCanonical)
        );
        assert_eq!(
            manifest(item(
                ModelEgressSourceKind::ExternalFileAttachment,
                NetworkDataClass::UserFileContent,
                ModelEgressAuthorizationBasis::SourceGrants { bindings: vec![attachment.clone()] },
            )),
            Err(ModelEgressContractError::SourceGrantsIncomplete)
        );
        assert!(
            manifest(item(
                ModelEgressSourceKind::ExternalFileAttachment,
                NetworkDataClass::UserFileContent,
                ModelEgressAuthorizationBasis::SourceGrants {
                    bindings: vec![attachment, external],
                },
            ))
            .is_ok()
        );
    }

    #[test]
    fn policy_version_and_manifest_hash_are_frozen() {
        let mut value = manifest(item(
            ModelEgressSourceKind::AuraCore,
            NetworkDataClass::PublicContent,
            ModelEgressAuthorizationBasis::ModelChannelPolicy,
        ))
        .expect("manifest");
        value.policy_version = ModelEgressPolicyVersion::new(NonZeroU32::new(3).expect("non-zero"));
        assert_eq!(
            value.validate_contract_shape(),
            Err(ModelEgressContractError::PolicyVersionUnsupported)
        );

        value.policy_version = ModelEgressPolicyVersion::V1;
        value.items[0].byte_length += 1;
        assert_eq!(
            value.validate_contract_shape(),
            Err(ModelEgressContractError::ManifestHashMismatch)
        );
    }
}
