use std::{collections::BTreeMap, str::FromStr, sync::Arc};

use aura_domain_common::{EndpointId, NormalizedIssueCode, Revision, SessionId};
use aura_model_config::{
    CredentialAvailability, CredentialSlotStatusFacts, ModelConfigError, ModelConfigService,
    ModelConfigSnapshot, ModelCredentialStatusError, derive_endpoint_credential_status,
    endpoint_fingerprint, normalize_base_url,
};
use aura_model_domain::{
    DefaultModelSelection, DormantEndpointCredentialSummary, EndpointAuthScheme,
    EndpointAuthenticationKind, EndpointAuthenticationProjection, EndpointCredentialState,
    EndpointCredentialStatus, EndpointDetailProjection, InitialSessionModelBinding,
    ModelAvailabilityTestView, ModelCredentialSlot, ModelDiscoveryRunRecord, ModelDiscoveryRunView,
    ModelExecutionReadinessView, ModelProfileConfig, ModelProfileProjection,
    ModelSettingsProjection, SessionModelBinding, SessionModelBindingSeed,
    SessionModelEndpointView, SessionModelOptionView, SessionModelProviderView,
    SessionModelSelectorProjection,
};
use aura_permission_domain::NormalizedOrigin;
use aura_persistence::{PersistenceCoordinator, PersistenceError};
use thiserror::Error;

use crate::p05_credential::credential_record_is_available;

#[derive(Debug, Error)]
pub enum P05ProjectionError {
    #[error(transparent)]
    ModelConfig(#[from] ModelConfigError),
    #[error(transparent)]
    Url(#[from] aura_model_config::ModelUrlError),
    #[error(transparent)]
    Persistence(#[from] PersistenceError),
    #[error(transparent)]
    CredentialStatus(#[from] ModelCredentialStatusError),
    #[error("model projection is unavailable: {0}")]
    Unavailable(String),
    #[error("model endpoint origin is invalid while constructing a projection")]
    InvalidEndpointOrigin,
}

#[derive(Clone)]
pub struct P05ProjectionBuilder {
    persistence: Arc<PersistenceCoordinator>,
    model_config: Arc<ModelConfigService>,
}

impl P05ProjectionBuilder {
    #[must_use]
    pub fn new(
        persistence: Arc<PersistenceCoordinator>,
        model_config: Arc<ModelConfigService>,
    ) -> Self {
        Self { persistence, model_config }
    }

    pub fn current_model_settings(&self) -> Result<ModelSettingsProjection, P05ProjectionError> {
        let snapshot = self.model_config.snapshot()?;
        self.model_settings(&snapshot, &BTreeMap::new())
    }

    pub fn model_settings(
        &self,
        snapshot: &ModelConfigSnapshot,
        status_overrides: &BTreeMap<EndpointId, EndpointCredentialStatus>,
    ) -> Result<ModelSettingsProjection, P05ProjectionError> {
        let default = self.model_config.default_model_selection()?;
        self.model_settings_with_default(snapshot, status_overrides, &default)
    }

    pub fn model_settings_with_default(
        &self,
        snapshot: &ModelConfigSnapshot,
        status_overrides: &BTreeMap<EndpointId, EndpointCredentialStatus>,
        default: &DefaultModelSelection,
    ) -> Result<ModelSettingsProjection, P05ProjectionError> {
        let mut active_credentials = Vec::new();
        let mut dormant_credentials = Vec::new();
        for endpoint in snapshot.registry.endpoints.values() {
            let authentication = self.authentication_projection(endpoint, status_overrides)?;
            if let Some(status) = authentication.active_credential {
                active_credentials.push(status);
            }
            if let Some(summary) = authentication.dormant_credential {
                dormant_credentials.push(summary);
            }
        }
        active_credentials.sort_by(|left, right| {
            (left.endpoint_id, left.slot_name, left.credential_id).cmp(&(
                right.endpoint_id,
                right.slot_name,
                right.credential_id,
            ))
        });
        dormant_credentials.sort_by(|left, right| {
            (left.endpoint_id, left.slot_name, left.credential_id).cmp(&(
                right.endpoint_id,
                right.slot_name,
                right.credential_id,
            ))
        });
        Ok(ModelSettingsProjection {
            models_revision: Revision::new(snapshot.revision),
            aura_revision: Revision::new(default.aura_config_revision),
            registry: (*snapshot.registry).clone(),
            default_model_profile_id: default.model_profile_id,
            active_credentials,
            dormant_credentials,
            reasoning_efforts: aura_model_domain::registry_reasoning_efforts(&snapshot.registry),
        })
    }

    pub fn endpoint_detail(
        &self,
        snapshot: &ModelConfigSnapshot,
        endpoint_id: EndpointId,
        status_overrides: &BTreeMap<EndpointId, EndpointCredentialStatus>,
    ) -> Result<EndpointDetailProjection, P05ProjectionError> {
        let latest_discovery_run = self
            .persistence
            .latest_model_discovery_run_for_endpoint(endpoint_id)?
            .as_ref()
            .map(|record| self.model_discovery_run_view(record))
            .transpose()?;
        self.endpoint_detail_with_latest(
            snapshot,
            endpoint_id,
            status_overrides,
            latest_discovery_run,
        )
    }

    /// Builds the Renderer-safe discovery view without exposing the
    /// InternalOnly `ModelFailureId` or provider error artifact.
    pub(crate) fn model_discovery_run_view(
        &self,
        record: &ModelDiscoveryRunRecord,
    ) -> Result<ModelDiscoveryRunView, P05ProjectionError> {
        Ok(ModelDiscoveryRunView::from_record(record))
    }

    pub(crate) fn endpoint_detail_with_latest(
        &self,
        snapshot: &ModelConfigSnapshot,
        endpoint_id: EndpointId,
        status_overrides: &BTreeMap<EndpointId, EndpointCredentialStatus>,
        latest_discovery_run: Option<ModelDiscoveryRunView>,
    ) -> Result<EndpointDetailProjection, P05ProjectionError> {
        let endpoint = snapshot
            .registry
            .endpoints
            .get(&endpoint_id)
            .cloned()
            .ok_or(ModelConfigError::EndpointIdNotFound(endpoint_id))?;
        let provider = snapshot
            .registry
            .providers
            .get(&endpoint.provider_id)
            .cloned()
            .ok_or(ModelConfigError::ProviderForEndpointNotFound(endpoint_id))?;
        let models = endpoint
            .model_order
            .iter()
            .map(|model_profile_id| {
                snapshot
                    .registry
                    .models
                    .get(model_profile_id)
                    .cloned()
                    .ok_or(ModelConfigError::ProfileNotFound(*model_profile_id))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let authentication = self.authentication_projection(&endpoint, status_overrides)?;
        Ok(EndpointDetailProjection {
            endpoint_id,
            models_revision: Revision::new(snapshot.revision),
            provider,
            endpoint,
            authentication,
            models,
            latest_discovery_run,
        })
    }

    pub fn model_profile(
        &self,
        snapshot: &ModelConfigSnapshot,
        model_profile_id: aura_domain_common::ModelProfileId,
        status_overrides: &BTreeMap<EndpointId, EndpointCredentialStatus>,
    ) -> Result<ModelProfileProjection, P05ProjectionError> {
        let latest_availability_test = self
            .persistence
            .latest_model_availability_test_for_profile(model_profile_id)?
            .as_ref()
            .map(ModelAvailabilityTestView::from_record);
        self.model_profile_with_latest_availability(
            snapshot,
            model_profile_id,
            status_overrides,
            latest_availability_test,
        )
    }

    pub(crate) fn model_profile_with_latest_availability(
        &self,
        snapshot: &ModelConfigSnapshot,
        model_profile_id: aura_domain_common::ModelProfileId,
        status_overrides: &BTreeMap<EndpointId, EndpointCredentialStatus>,
        latest_availability_test: Option<ModelAvailabilityTestView>,
    ) -> Result<ModelProfileProjection, P05ProjectionError> {
        let profile = snapshot
            .registry
            .models
            .get(&model_profile_id)
            .cloned()
            .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
        let endpoint = snapshot
            .registry
            .endpoints
            .get(&profile.endpoint_id)
            .cloned()
            .ok_or(ModelConfigError::EndpointNotFound(model_profile_id))?;
        let provider = snapshot
            .registry
            .providers
            .get(&endpoint.provider_id)
            .cloned()
            .ok_or(ModelConfigError::ProviderNotFound(model_profile_id))?;
        let credential_status = match status_overrides.get(&endpoint.id) {
            Some(status) => Some(status.clone()),
            None => self.active_credential_status(&endpoint)?,
        };
        let execution_readiness =
            model_execution_readiness(&profile, &endpoint, credential_status.as_ref());
        let active_availability_test =
            latest_availability_test.as_ref().filter(|test| !test.state.is_terminal()).cloned();
        Ok(ModelProfileProjection {
            model_profile_id,
            models_revision: Revision::new(snapshot.revision),
            provider,
            endpoint,
            execution_readiness,
            profile,
            active_availability_test,
            latest_availability_test,
        })
    }

    pub fn session_model_selector(
        &self,
        snapshot: &ModelConfigSnapshot,
        session_id: SessionId,
        binding: Option<SessionModelBinding>,
    ) -> Result<SessionModelSelectorProjection, P05ProjectionError> {
        let default = self.model_config.default_model_selection()?;
        self.session_model_selector_with_default(snapshot, session_id, binding, &default)
    }

    /// Resolves the default binding from one immutable model-config snapshot.
    /// The Session owner persists this seed together with the Session and its
    /// N-A authorization; P05 never creates a synthetic Session itself.
    pub fn initial_session_model_binding(
        &self,
        snapshot: &ModelConfigSnapshot,
        default: &DefaultModelSelection,
    ) -> Result<InitialSessionModelBinding, P05ProjectionError> {
        let Some(model_profile_id) = default.model_profile_id else {
            return Ok(InitialSessionModelBinding::Unbound {
                issue_code: NormalizedIssueCode::new("configuration.model_profile_not_selected")
                    .expect("frozen initial binding issue code"),
            });
        };
        let profile = snapshot
            .registry
            .models
            .get(&model_profile_id)
            .ok_or(ModelConfigError::ProfileNotFound(model_profile_id))?;
        let endpoint = snapshot
            .registry
            .endpoints
            .get(&profile.endpoint_id)
            .ok_or(ModelConfigError::EndpointNotFound(model_profile_id))?;
        let provider = snapshot
            .registry
            .providers
            .get(&endpoint.provider_id)
            .ok_or(ModelConfigError::ProviderNotFound(model_profile_id))?;
        let credential_status = self.active_credential_status(endpoint)?;
        let readiness = model_execution_readiness(profile, endpoint, credential_status.as_ref());
        if let Some(issue_code) = readiness.issue_code {
            return Ok(InitialSessionModelBinding::Unbound { issue_code });
        }
        Ok(InitialSessionModelBinding::Bound {
            seed: SessionModelBindingSeed {
                provider_id: provider.id,
                endpoint_id: endpoint.id,
                model_profile_id: profile.id,
                readable_ref_at_binding: format!(
                    "{}/{}/{}",
                    provider.slug, endpoint.slug, profile.provider_model_name
                ),
                models_revision_at_binding: Revision::new(snapshot.revision),
                endpoint_revision_at_binding: endpoint.revision,
                model_profile_revision_at_binding: profile.revision,
                endpoint_security_fingerprint_at_binding: endpoint_fingerprint(endpoint)?,
                api_format_at_binding: endpoint.api_format,
            },
        })
    }

    pub fn session_model_selector_with_default(
        &self,
        snapshot: &ModelConfigSnapshot,
        session_id: SessionId,
        binding: Option<SessionModelBinding>,
        default: &DefaultModelSelection,
    ) -> Result<SessionModelSelectorProjection, P05ProjectionError> {
        self.session_model_selector_with_default_and_overrides(
            snapshot,
            session_id,
            binding,
            default,
            &BTreeMap::new(),
        )
    }

    fn session_model_selector_with_default_and_overrides(
        &self,
        snapshot: &ModelConfigSnapshot,
        session_id: SessionId,
        binding: Option<SessionModelBinding>,
        default: &DefaultModelSelection,
        status_overrides: &BTreeMap<EndpointId, EndpointCredentialStatus>,
    ) -> Result<SessionModelSelectorProjection, P05ProjectionError> {
        let providers = snapshot
            .registry
            .provider_order
            .iter()
            .enumerate()
            .map(|(order, provider_id)| {
                let provider = snapshot
                    .registry
                    .providers
                    .get(provider_id)
                    .cloned()
                    .ok_or(ModelConfigError::ProviderNotFoundById(*provider_id))?;
                Ok(SessionModelProviderView {
                    provider_id: provider.id,
                    display_name: provider.display_name,
                    order: u32::try_from(order).unwrap_or(u32::MAX),
                })
            })
            .collect::<Result<Vec<_>, ModelConfigError>>()?;
        let mut endpoints = Vec::new();
        let mut models = Vec::new();
        let mut readiness_by_profile = BTreeMap::new();
        for provider_id in &snapshot.registry.provider_order {
            let provider = snapshot
                .registry
                .providers
                .get(provider_id)
                .ok_or(ModelConfigError::ProviderNotFoundById(*provider_id))?;
            for (endpoint_order, endpoint_id) in provider.endpoint_order.iter().enumerate() {
                let endpoint = snapshot
                    .registry
                    .endpoints
                    .get(endpoint_id)
                    .cloned()
                    .ok_or(ModelConfigError::EndpointIdNotFound(*endpoint_id))?;
                let credential_status = match status_overrides.get(&endpoint.id) {
                    Some(status) => Some(status.clone()),
                    None => self.active_credential_status(&endpoint)?,
                };
                for (model_order, model_profile_id) in endpoint.model_order.iter().enumerate() {
                    let profile = snapshot
                        .registry
                        .models
                        .get(model_profile_id)
                        .cloned()
                        .ok_or(ModelConfigError::ProfileNotFound(*model_profile_id))?;
                    let readiness =
                        model_execution_readiness(&profile, &endpoint, credential_status.as_ref());
                    let effective_max_output_tokens = profile.effective_max_output_tokens();
                    readiness_by_profile.insert(profile.id, readiness.clone());
                    // The levels the profile lists, else the catalog's and the format's.
                    let reasoning_efforts = aura_model_domain::offered_reasoning_efforts(
                        &profile.request_defaults,
                        endpoint.api_format,
                        profile.catalog.as_ref(),
                    );
                    let reasoning_effort = profile.request_defaults.reasoning_effort;
                    models.push(SessionModelOptionView {
                        model_profile_id: profile.id,
                        endpoint_id: endpoint.id,
                        display_name: profile.display_name,
                        provider_model_name: profile.provider_model_name,
                        effective_context_window_tokens: profile
                            .context
                            .runtime_effective_window_tokens,
                        effective_max_output_tokens,
                        execution_readiness: readiness,
                        order: u32::try_from(model_order).unwrap_or(u32::MAX),
                        reasoning_efforts,
                        reasoning_effort,
                    });
                }
                endpoints.push(SessionModelEndpointView {
                    endpoint_id: endpoint.id,
                    provider_id: endpoint.provider_id,
                    display_name: endpoint.display_name,
                    api_format: endpoint.api_format,
                    order: u32::try_from(endpoint_order).unwrap_or(u32::MAX),
                });
            }
        }
        let selected_model_profile_id = binding.as_ref().map(|binding| binding.model_profile_id);
        let selected_model_execution_readiness = selected_model_profile_id
            .and_then(|model_profile_id| readiness_by_profile.get(&model_profile_id).cloned())
            .unwrap_or_else(|| ModelExecutionReadinessView {
                is_executable: false,
                issue_code: Some(
                    NormalizedIssueCode::new("configuration.model_profile_not_selected")
                        .expect("frozen selector issue code"),
                ),
            });
        Ok(SessionModelSelectorProjection {
            session_id,
            // An absent binding may be either revision zero or a cleared
            // binding tombstone. Both must use the Session owner's CAS value.
            binding_revision: match &binding {
                Some(binding) => binding.binding_revision,
                None => self.persistence.session_model_binding_state(session_id)?.revision,
            },
            selected_model_profile_id,
            default_model_profile_id: default.model_profile_id,
            providers,
            endpoints,
            models,
            selected_model_execution_readiness,
            projection_revision: Revision::INITIAL,
        })
    }

    /// Completes a model-config transaction's projection set from the candidate
    /// `ModelSettingsProjection` already present in that same transaction.
    /// Existing scoped drafts win so deletion-specific binding clears and
    /// operation-specific endpoint views remain exact.
    pub(crate) fn complete_model_config_projection_drafts(
        &self,
        drafts: &mut Vec<aura_persistence::ModelProjectionDraft>,
    ) -> Result<(), P05ProjectionError> {
        let Some(settings) = drafts.iter().find_map(|draft| match draft {
            aura_persistence::ModelProjectionDraft::ModelSettings(settings) => {
                Some(settings.clone())
            }
            _ => None,
        }) else {
            return Ok(());
        };
        let mut snapshot = self.model_config.snapshot()?;
        snapshot.revision = settings.models_revision.value();
        snapshot.registry = Arc::new(settings.registry.clone());
        let default = DefaultModelSelection {
            model_profile_id: settings.default_model_profile_id,
            aura_config_revision: settings.aura_revision.value(),
        };
        let status_overrides = settings
            .active_credentials
            .iter()
            .cloned()
            .map(|status| (status.endpoint_id, status))
            .collect::<BTreeMap<_, _>>();

        for endpoint_id in snapshot.registry.endpoints.keys().copied() {
            let already_present = drafts.iter().any(|draft| {
                matches!(
                    draft,
                    aura_persistence::ModelProjectionDraft::EndpointDetail(detail)
                        if detail.endpoint_id == endpoint_id
                )
            });
            if !already_present {
                drafts.push(aura_persistence::ModelProjectionDraft::EndpointDetail(Box::new(
                    self.endpoint_detail(&snapshot, endpoint_id, &status_overrides)?,
                )));
            }
        }
        for model_profile_id in snapshot.registry.models.keys().copied() {
            let already_present = drafts.iter().any(|draft| {
                matches!(
                    draft,
                    aura_persistence::ModelProjectionDraft::ModelProfile(profile)
                        if profile.model_profile_id == model_profile_id
                )
            });
            if !already_present {
                drafts.push(aura_persistence::ModelProjectionDraft::ModelProfile(Box::new(
                    self.model_profile(&snapshot, model_profile_id, &status_overrides)?,
                )));
            }
        }
        for session in self.persistence.workspace_session_projection()?.sessions {
            let session_id = session.session_id;
            let already_present = drafts.iter().any(|draft| {
                matches!(
                    draft,
                    aura_persistence::ModelProjectionDraft::SessionModelSelector(selector)
                        if selector.session_id == session_id
                )
            });
            if !already_present {
                let binding = self.persistence.session_model_binding(session_id)?;
                drafts.push(aura_persistence::ModelProjectionDraft::SessionModelSelector(
                    self.session_model_selector_with_default_and_overrides(
                        &snapshot,
                        session_id,
                        binding,
                        &default,
                        &status_overrides,
                    )?,
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn active_credential_status(
        &self,
        endpoint: &aura_model_domain::ModelEndpointConfig,
    ) -> Result<Option<EndpointCredentialStatus>, P05ProjectionError> {
        let record = self.persistence.model_credential_record_for_endpoint(
            endpoint.id,
            aura_model_domain::ModelCredentialSlot::ApiKey,
        )?;
        if endpoint.auth_scheme == EndpointAuthScheme::None {
            // Retained records are represented by DormantEndpointCredentialSummary,
            // not by the active credential map. Its exact projection placement is
            // intentionally handled by the dormant projection contract.
            return Ok(None);
        }

        let facts = record.as_ref().and_then(|record| {
            let state = record.state.renderer_state()?;
            let mut error_code = record.error_code.clone();
            let binding_matches = endpoint_binding_matches(endpoint, record).unwrap_or(false);
            let available = binding_matches && credential_record_is_available(record);
            if !binding_matches {
                error_code = Some(
                    NormalizedIssueCode::from_str("credential.binding_mismatch")
                        .expect("frozen credential issue code"),
                );
            } else if !available && error_code.is_none() {
                error_code = Some(
                    NormalizedIssueCode::from_str("credential.unavailable")
                        .expect("registered credential issue prefix"),
                );
            }
            Some((
                CredentialSlotStatusFacts {
                    endpoint_id: record.endpoint_id,
                    slot_name: record.credential_slot,
                    credential_id: record.credential_id,
                    state,
                    last_committed_credential_revision: record.last_committed_credential_revision,
                    row_revision: record.row_revision,
                    error_code,
                },
                if available {
                    CredentialAvailability::Available
                } else {
                    CredentialAvailability::Unavailable
                },
            ))
        });
        let (facts, availability) = facts
            .as_ref()
            .map_or((None, CredentialAvailability::Unavailable), |(facts, availability)| {
                (Some(facts), *availability)
            });
        derive_endpoint_credential_status(endpoint.auth_scheme, facts, availability)
            .map_err(P05ProjectionError::from)
    }

    fn authentication_projection(
        &self,
        endpoint: &aura_model_domain::ModelEndpointConfig,
        status_overrides: &BTreeMap<EndpointId, EndpointCredentialStatus>,
    ) -> Result<EndpointAuthenticationProjection, P05ProjectionError> {
        let projection = if endpoint.auth_scheme == EndpointAuthScheme::None {
            EndpointAuthenticationProjection {
                auth_kind: EndpointAuthenticationKind::None,
                active_credential: None,
                dormant_credential: self.dormant_credential_summary(endpoint)?,
            }
        } else {
            let active_credential = match status_overrides.get(&endpoint.id) {
                Some(status) => Some(status.clone()),
                None => self.active_credential_status(endpoint)?,
            };
            EndpointAuthenticationProjection {
                auth_kind: EndpointAuthenticationKind::Secret,
                active_credential,
                dormant_credential: None,
            }
        };
        if projection.is_valid() {
            Ok(projection)
        } else {
            Err(PersistenceError::InvalidRequest(
                "endpoint authentication projection violates the active/dormant invariant"
                    .to_owned(),
            )
            .into())
        }
    }

    fn dormant_credential_summary(
        &self,
        endpoint: &aura_model_domain::ModelEndpointConfig,
    ) -> Result<Option<DormantEndpointCredentialSummary>, P05ProjectionError> {
        let record = self
            .persistence
            .model_credential_record_for_endpoint(endpoint.id, ModelCredentialSlot::ApiKey)?;
        Ok(record.and_then(|record| {
            let retained_state = record.state.renderer_state()?;
            Some(DormantEndpointCredentialSummary {
                endpoint_id: record.endpoint_id,
                slot_name: record.credential_slot,
                credential_id: record.credential_id,
                retained_state,
                last_committed_credential_revision: record.last_committed_credential_revision,
                row_revision: record.row_revision,
                error_code: record.error_code,
            })
        }))
    }

    /// Builds every Renderer-visible projection affected by a committed
    /// credential mutation. The status override is required because the
    /// WinCred/SQLite journal constructs the new Renderer-safe state before
    /// the metadata transaction makes that state queryable from SQLite.
    pub(crate) fn credential_mutation_drafts(
        &self,
        snapshot: &ModelConfigSnapshot,
        endpoint_id: EndpointId,
        status: EndpointCredentialStatus,
    ) -> Result<Vec<aura_persistence::ModelProjectionDraft>, P05ProjectionError> {
        let overrides = BTreeMap::from([(endpoint_id, status)]);
        let mut drafts = vec![
            aura_persistence::ModelProjectionDraft::ModelSettings(
                self.model_settings(snapshot, &overrides)?,
            ),
            aura_persistence::ModelProjectionDraft::EndpointDetail(Box::new(
                self.endpoint_detail(snapshot, endpoint_id, &overrides)?,
            )),
        ];
        let endpoint = snapshot
            .registry
            .endpoints
            .get(&endpoint_id)
            .ok_or(ModelConfigError::EndpointIdNotFound(endpoint_id))?;
        for model_profile_id in &endpoint.model_order {
            drafts.push(aura_persistence::ModelProjectionDraft::ModelProfile(Box::new(
                self.model_profile(snapshot, *model_profile_id, &overrides)?,
            )));
        }
        self.complete_model_config_projection_drafts(&mut drafts)?;
        Ok(drafts)
    }
}

fn model_execution_readiness(
    profile: &ModelProfileConfig,
    endpoint: &aura_model_domain::ModelEndpointConfig,
    credential_status: Option<&EndpointCredentialStatus>,
) -> ModelExecutionReadinessView {
    let issue_code = if !profile.enabled {
        Some("configuration.model_profile_disabled")
    } else if !profile.effective_max_output_tokens().is_some_and(|tokens| tokens > 0) {
        Some("configuration.output_limit_missing")
    } else if endpoint.auth_scheme == EndpointAuthScheme::None {
        None
    } else {
        match credential_status {
            Some(status) if status.state == EndpointCredentialState::Configured => None,
            Some(status) => status.error_code.as_ref().map(NormalizedIssueCode::as_str).or(Some(
                match status.state {
                    EndpointCredentialState::Unconfigured => "credential.unconfigured",
                    EndpointCredentialState::Configured => unreachable!(
                        "configured credential status is handled before issue selection"
                    ),
                    EndpointCredentialState::Cleared => "credential.cleared",
                    EndpointCredentialState::Unavailable => "credential.unavailable",
                    EndpointCredentialState::ReconciliationRequired => {
                        "credential.reconciliation_required"
                    }
                },
            )),
            None => Some("credential.unconfigured"),
        }
    };
    ModelExecutionReadinessView {
        is_executable: issue_code.is_none(),
        issue_code: issue_code.map(|code| {
            NormalizedIssueCode::from_str(code).expect("frozen readiness issue code is valid")
        }),
    }
}

fn endpoint_binding_matches(
    endpoint: &aura_model_domain::ModelEndpointConfig,
    record: &aura_persistence::ModelCredentialRecord,
) -> Result<bool, P05ProjectionError> {
    let url = normalize_base_url(&endpoint.base_url)?;
    let host = url.host_str().ok_or(P05ProjectionError::InvalidEndpointOrigin)?;
    let port = url.port_or_known_default().ok_or(P05ProjectionError::InvalidEndpointOrigin)?;
    let current_origin =
        NormalizedOrigin { scheme: url.scheme().to_owned(), host: host.to_ascii_lowercase(), port };
    Ok(record.binding.auth_scheme == endpoint.auth_scheme
        && record.binding.normalized_origin == current_origin)
}
