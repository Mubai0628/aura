use aura_context_domain::{
    ContextSourceRef, ModelEgressAuthorizationBasis, ModelEgressContractError, ModelEgressItem,
    ModelEgressManifestV1, ModelEgressSourceKind, SourceAuthorizationBinding,
};
use aura_domain_common::{
    ContentHash, ContextProjectionSnapshotId, EventId, ModelProfileId, SessionId,
};
use aura_security_contract::NetworkDataClass;
use thiserror::Error;

#[derive(Clone, Debug)]
pub struct ModelEgressSourceDraft {
    pub source_ref: ContextSourceRef,
    pub source_kind: ModelEgressSourceKind,
    pub data_class: NetworkDataClass,
    pub content_hash: ContentHash,
    pub byte_length: u64,
    pub authorization_basis: ModelEgressAuthorizationBasis,
}

impl From<&crate::ContextFragment> for ModelEgressSourceDraft {
    fn from(value: &crate::ContextFragment) -> Self {
        Self {
            source_ref: value.source_ref.clone(),
            source_kind: value.source_kind,
            data_class: value.data_class,
            content_hash: ContentHash::sha256(value.content.as_bytes()),
            byte_length: u64::try_from(value.content.len()).unwrap_or(u64::MAX),
            authorization_basis: value.authorization_basis.clone(),
        }
    }
}

pub fn build_model_egress_manifest(
    context_snapshot_id: ContextProjectionSnapshotId,
    sources: Vec<ModelEgressSourceDraft>,
) -> Result<ModelEgressManifestV1, ModelEgressContractError> {
    let items = sources
        .into_iter()
        .enumerate()
        .map(|(index, source)| {
            Ok(ModelEgressItem {
                ordinal: u32::try_from(index + 1)
                    .map_err(|_| ModelEgressContractError::ItemOrderInvalid)?,
                source_ref: source.source_ref,
                source_kind: source.source_kind,
                data_class: source.data_class,
                content_hash: source.content_hash,
                byte_length: source.byte_length,
                authorization_basis: source.authorization_basis,
            })
        })
        .collect::<Result<Vec<_>, ModelEgressContractError>>()?;
    ModelEgressManifestV1::new(context_snapshot_id, items)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PersistedContextEventActor {
    User,
    ParentAgent,
    ChildAgent,
    SystemRuntime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PersistedContextEventAuthorizationFact {
    pub event_id: EventId,
    pub session_id: SessionId,
    pub actor: PersistedContextEventActor,
    pub content_hash: ContentHash,
    pub byte_length: u64,
    pub model_visible: bool,
    pub explicitly_user_authored: bool,
}

/// The authority the model-egress decision reads.
///
/// WHY `persisted_event` IS AWAITABLE
/// The fact is a real persisted-context read. It used to be synchronous, which meant the whole
/// egress verification chain - including the Agent's own authorizer - had to hold the journal on
/// whatever task it ran from, and that task is a reactor worker. The chain is now awaitable end to
/// end so each implementation can put its own synchronous critical section on its own bounded
/// owner. Every implementation declares, for that await, its own admission, deadline and receipt;
/// the one rule the trait states is that a refusal ("the fact was never read") and an unknown
/// result ("the read started and may still answer") must not be reported as each other.
pub trait ModelEgressAuthorizationFacts: Send + Sync {
    fn session_model_channel_is_authorized(
        &self,
        session_id: SessionId,
        model_profile_id: ModelProfileId,
    ) -> bool;

    fn persisted_event(
        &self,
        event_id: EventId,
    ) -> impl std::future::Future<
        Output = Result<
            Option<PersistedContextEventAuthorizationFact>,
            ModelEgressAuthorizationError,
        >,
    > + Send;

    fn source_grant_binding_is_valid(
        &self,
        session_id: SessionId,
        source_ref: &ContextSourceRef,
        source_kind: ModelEgressSourceKind,
        data_class: NetworkDataClass,
        binding: &SourceAuthorizationBinding,
    ) -> Result<bool, ModelEgressAuthorizationError>;

    /// A picture the user sent with a message of this session, by the artifact it is kept
    /// in: its SHA-256 digest and length. None when the artifact is not one.
    fn user_attachment(
        &self,
        _session_id: SessionId,
        _artifact_id: aura_domain_common::ArtifactId,
    ) -> impl std::future::Future<
        Output = Result<Option<(ContentHash, u64)>, ModelEgressAuthorizationError>,
    > + Send {
        async { Ok(None) }
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ModelEgressAuthorizationError {
    #[error("model_egress.session_model_channel_authorization_missing")]
    SessionModelChannelAuthorizationMissing,
    #[error("model_egress.contract_invalid")]
    ContractInvalid,
    #[error("model_egress.user_event_not_found")]
    UserEventNotFound,
    #[error("model_egress.user_event_identity_mismatch")]
    UserEventIdentityMismatch,
    #[error("model_egress.user_event_content_mismatch")]
    UserEventContentMismatch,
    #[error("model_egress.user_event_not_visible")]
    UserEventNotVisible,
    #[error("model_egress.source_grant_invalid")]
    SourceGrantInvalid,
    #[error("model_egress.authorization_fact_store_failed")]
    FactStore,
}

pub struct ModelEgressAuthorizer<'a, F> {
    facts: &'a F,
}

impl<'a, F> ModelEgressAuthorizer<'a, F>
where
    F: ModelEgressAuthorizationFacts,
{
    #[must_use]
    pub const fn new(facts: &'a F) -> Self {
        Self { facts }
    }

    pub async fn validate(
        &self,
        session_id: SessionId,
        model_profile_id: ModelProfileId,
        manifest: &ModelEgressManifestV1,
    ) -> Result<(), ModelEgressAuthorizationError> {
        manifest
            .validate_contract_shape()
            .map_err(|_| ModelEgressAuthorizationError::ContractInvalid)?;
        if !self.facts.session_model_channel_is_authorized(session_id, model_profile_id) {
            return Err(ModelEgressAuthorizationError::SessionModelChannelAuthorizationMissing);
        }
        for item in &manifest.items {
            match &item.authorization_basis {
                ModelEgressAuthorizationBasis::ModelChannelPolicy => {}
                ModelEgressAuthorizationBasis::UserSubmission => {
                    // A picture sent with a user message travels as an attachment, under
                    // the artifact it is kept in.
                    if let ContextSourceRef::Artifact { artifact_id } = item.source_ref {
                        let (content_hash, byte_length) = self
                            .facts
                            .user_attachment(session_id, artifact_id)
                            .await?
                            .ok_or(ModelEgressAuthorizationError::UserEventNotFound)?;
                        if content_hash != item.content_hash || byte_length != item.byte_length {
                            return Err(ModelEgressAuthorizationError::UserEventContentMismatch);
                        }
                        continue;
                    }
                    let ContextSourceRef::Event { event_id } = item.source_ref else {
                        return Err(ModelEgressAuthorizationError::UserEventIdentityMismatch);
                    };
                    let fact = self
                        .facts
                        .persisted_event(event_id)
                        .await?
                        .ok_or(ModelEgressAuthorizationError::UserEventNotFound)?;
                    if fact.event_id != event_id
                        || fact.session_id != session_id
                        || fact.actor != PersistedContextEventActor::User
                        || !fact.explicitly_user_authored
                    {
                        return Err(ModelEgressAuthorizationError::UserEventIdentityMismatch);
                    }
                    if fact.content_hash != item.content_hash
                        || fact.byte_length != item.byte_length
                    {
                        return Err(ModelEgressAuthorizationError::UserEventContentMismatch);
                    }
                    if !fact.model_visible {
                        return Err(ModelEgressAuthorizationError::UserEventNotVisible);
                    }
                }
                ModelEgressAuthorizationBasis::SourceGrants { bindings } => {
                    for binding in bindings {
                        if !self.facts.source_grant_binding_is_valid(
                            session_id,
                            &item.source_ref,
                            item.source_kind,
                            item.data_class,
                            binding,
                        )? {
                            return Err(ModelEgressAuthorizationError::SourceGrantInvalid);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use aura_domain_common::ContextProjectionSnapshotId;

    use super::*;

    struct Facts {
        channel: bool,
        events: BTreeMap<EventId, PersistedContextEventAuthorizationFact>,
    }

    impl ModelEgressAuthorizationFacts for Facts {
        fn session_model_channel_is_authorized(
            &self,
            _session_id: SessionId,
            _model_profile_id: ModelProfileId,
        ) -> bool {
            self.channel
        }

        async fn persisted_event(
            &self,
            event_id: EventId,
        ) -> Result<Option<PersistedContextEventAuthorizationFact>, ModelEgressAuthorizationError>
        {
            Ok(self.events.get(&event_id).cloned())
        }

        fn source_grant_binding_is_valid(
            &self,
            _session_id: SessionId,
            _source_ref: &ContextSourceRef,
            _source_kind: ModelEgressSourceKind,
            _data_class: NetworkDataClass,
            _binding: &SourceAuthorizationBinding,
        ) -> Result<bool, ModelEgressAuthorizationError> {
            Ok(true)
        }
    }

    #[tokio::test]
    async fn user_submission_requires_the_exact_persisted_user_event_and_channel() {
        let session_id = SessionId::new();
        let event_id = EventId::new();
        let content = "question";
        let source = ModelEgressSourceDraft {
            source_ref: ContextSourceRef::Event { event_id },
            source_kind: ModelEgressSourceKind::UserEvent,
            data_class: NetworkDataClass::PersonalData,
            content_hash: ContentHash::sha256(content.as_bytes()),
            byte_length: content.len() as u64,
            authorization_basis: ModelEgressAuthorizationBasis::UserSubmission,
        };
        let manifest =
            build_model_egress_manifest(ContextProjectionSnapshotId::new(), vec![source])
                .expect("manifest");
        let facts = Facts {
            channel: true,
            events: BTreeMap::from([(
                event_id,
                PersistedContextEventAuthorizationFact {
                    event_id,
                    session_id,
                    actor: PersistedContextEventActor::User,
                    content_hash: ContentHash::sha256(content.as_bytes()),
                    byte_length: content.len() as u64,
                    model_visible: true,
                    explicitly_user_authored: true,
                },
            )]),
        };
        ModelEgressAuthorizer::new(&facts)
            .validate(session_id, ModelProfileId::new(), &manifest)
            .await
            .expect("authorized");

        let denied = Facts { channel: false, events: facts.events };
        assert_eq!(
            ModelEgressAuthorizer::new(&denied)
                .validate(session_id, ModelProfileId::new(), &manifest)
                .await,
            Err(ModelEgressAuthorizationError::SessionModelChannelAuthorizationMissing)
        );
    }
}
