pub use commands::P05CommandDispatch;
use std::{
    collections::BTreeMap,
    env,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result};
use aura_config::SettingsFacade;
use aura_domain_common::{ModelAvailabilityTestId, ModelDiscoveryRunId, Revision};
use aura_model_config::ModelConfigService;
use aura_model_domain::{
    CancellationSource, ModelAvailabilityTestState, ModelDiscoveryState,
    ModelOperationCancellationReason, TransportAttemptState, TransportOperationContext,
};
use aura_network_control::{
    ConfigurationTransportPolicyV1, CoreHttpClientGateway, NetworkControlOptions,
    PersistenceNetworkGrantStore,
};
use aura_persistence::ModelDiscoveryTerminalEventContext;
use aura_persistence::PersistenceCoordinator;
use aura_persistence::{
    ModelConfigCredentialChildPhase, ModelConfigMutationJournalState, ModelProjectionDraft,
    PersistenceError,
};
use aura_projection_contract::{ProjectionChange, ProjectionKind, ProjectionScope};
use aura_runtime_contract::{BundleNamespace, ReleaseTrustDomain};
use tokio::sync::Notify;

use crate::p05_credential::PersistentProviderCredentialResolver;
use crate::p05_credential::{P05CredentialJournal, P05CredentialJournalError};

pub(crate) mod availability;
pub mod catalog;
pub(crate) mod commands;
pub(crate) mod config_journal;
pub(crate) mod configuration_transport;
pub(crate) mod credential_preflight;
pub(crate) mod discovery;
pub(crate) mod projection;
pub mod provider_error;
pub mod provider_icon;
mod session_selector;
pub(crate) mod transport_store;

use catalog::{PersistentProviderCatalog, open_catalog, project_catalog, spawn_catalog_refresh};
use config_journal::ModelConfigJournalCoordinator;
use configuration_transport::ConfigurationTransport;
use projection::P05ProjectionBuilder;
use provider_icon::ProviderIconService;
use transport_store::{PersistentTransportStore, UnavailableAgentModelArtifactOwnerResolver};

const fn model_config_recovery_phase(state: ModelConfigMutationJournalState) -> &'static str {
    match state {
        ModelConfigMutationJournalState::Prepared => "prepared",
        ModelConfigMutationJournalState::CredentialStaged => "credential_staged",
        ModelConfigMutationJournalState::ConfigPublished => "config_published",
        ModelConfigMutationJournalState::CredentialMutationCompleted => {
            "credential_mutation_completed"
        }
        ModelConfigMutationJournalState::MetadataCommitted => "metadata_committed",
        ModelConfigMutationJournalState::CleanupPending => "cleanup_pending",
        ModelConfigMutationJournalState::Completed => "completed",
        ModelConfigMutationJournalState::Aborted => "aborted",
        ModelConfigMutationJournalState::ReconciliationRequired => "reconciliation_required",
    }
}

fn model_config_recovery_error_code(
    error: &config_journal::ModelConfigJournalError,
) -> &'static str {
    use config_journal::ModelConfigJournalError;
    match error {
        ModelConfigJournalError::ReconciliationRequired(_) => {
            "persistence.model_config_reconciliation_required"
        }
        ModelConfigJournalError::Aborted(_) => "persistence.model_config_recovery_aborted",
        ModelConfigJournalError::Persistence(PersistenceError::InvalidRequest(_)) => {
            "persistence.model_config_recovery_invalid_payload"
        }
        ModelConfigJournalError::Persistence(_) => {
            "persistence.model_config_recovery_persistence_failed"
        }
        ModelConfigJournalError::Settings(_) | ModelConfigJournalError::ModelConfig(_) => {
            "persistence.model_config_recovery_config_failed"
        }
        ModelConfigJournalError::Projection(_) => {
            "persistence.model_config_recovery_projection_failed"
        }
        ModelConfigJournalError::InvalidPayload(_) => {
            "persistence.model_config_recovery_decode_failed"
        }
        ModelConfigJournalError::CredentialChildRequiresDedicatedCoordinator => {
            "persistence.model_config_recovery_child_phase_conflict"
        }
        ModelConfigJournalError::Credential(_) => {
            "persistence.model_config_recovery_credential_failed"
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct P05OperationControl {
    cancellation: aura_model_transport::ModelCancellationToken,
    cancellation_reason: Arc<Mutex<Option<ModelOperationCancellationReason>>>,
    terminal_projection: Arc<P05TerminalProjectionSignal>,
}

impl P05OperationControl {
    fn cancel(&self, reason: ModelOperationCancellationReason) {
        let mut current = self
            .cancellation_reason
            .lock()
            .expect("P05 operation cancellation reason lock is available");
        if current.is_none() {
            *current = Some(reason);
        }
        drop(current);
        self.cancellation.cancel();
    }

    #[must_use]
    fn cancellation_reason(&self) -> Option<ModelOperationCancellationReason> {
        *self
            .cancellation_reason
            .lock()
            .expect("P05 operation cancellation reason lock is available")
    }

    #[must_use]
    fn cancellation(&self) -> aura_model_transport::ModelCancellationToken {
        self.cancellation.clone()
    }

    fn mark_terminal_projection_published(&self) {
        self.terminal_projection.mark_published();
    }

    async fn wait_for_terminal_projection(&self) {
        self.terminal_projection.wait().await;
    }
}

#[derive(Default)]
struct P05TerminalProjectionSignal {
    published: AtomicBool,
    notify: Notify,
}

impl P05TerminalProjectionSignal {
    fn mark_published(&self) {
        if !self.published.swap(true, Ordering::AcqRel) {
            self.notify.notify_waiters();
        }
    }

    async fn wait(&self) {
        loop {
            let notified = self.notify.notified();
            if self.published.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }
}

#[derive(Default)]
struct P05OperationCancellationRegistry {
    discoveries: Mutex<BTreeMap<ModelDiscoveryRunId, P05OperationControl>>,
    availability_tests: Mutex<BTreeMap<ModelAvailabilityTestId, P05OperationControl>>,
}

impl P05OperationCancellationRegistry {
    fn register_discovery(&self, id: ModelDiscoveryRunId) -> P05OperationControl {
        let control = P05OperationControl::default();
        self.discoveries
            .lock()
            .expect("P05 discovery cancellation registry lock is available")
            .insert(id, control.clone());
        control
    }

    fn discovery(&self, id: ModelDiscoveryRunId) -> Option<P05OperationControl> {
        self.discoveries
            .lock()
            .expect("P05 discovery cancellation registry lock is available")
            .get(&id)
            .cloned()
    }

    fn remove_discovery(&self, id: ModelDiscoveryRunId) {
        self.discoveries
            .lock()
            .expect("P05 discovery cancellation registry lock is available")
            .remove(&id);
    }

    fn register_availability(&self, id: ModelAvailabilityTestId) -> P05OperationControl {
        let control = P05OperationControl::default();
        self.availability_tests
            .lock()
            .expect("P05 availability cancellation registry lock is available")
            .insert(id, control.clone());
        control
    }

    fn availability(&self, id: ModelAvailabilityTestId) -> Option<P05OperationControl> {
        self.availability_tests
            .lock()
            .expect("P05 availability cancellation registry lock is available")
            .get(&id)
            .cloned()
    }

    fn remove_availability(&self, id: ModelAvailabilityTestId) {
        self.availability_tests
            .lock()
            .expect("P05 availability cancellation registry lock is available")
            .remove(&id);
    }
}

/// process-lifetime services constructed once by the Core composition root.
///
/// The transport store deliberately has no fixed Session identity. Discovery,
/// availability tests and later Agent requests receive the same store and derive
/// their direct Artifact owner from each persisted operation context.
#[derive(Clone)]
pub struct P05Services {
    pub model_gateway: Arc<CoreHttpClientGateway>,
    pub model_config: Arc<ModelConfigService>,
    pub provider_catalog: Arc<PersistentProviderCatalog>,
    pub configuration_transport: ConfigurationTransport,
    pub persistence: Arc<PersistenceCoordinator>,
    pub bundle_namespace: BundleNamespace,
    pub config_journal: ModelConfigJournalCoordinator,
    pub credential_journal: P05CredentialJournal,
    pub projections: P05ProjectionBuilder,
    pub(crate) provider_icons: ProviderIconService,
    /// The process's one bounded synchronous boundary, shared by cloning.
    ///
    /// The catalog refresh lives here because every step of it after the fetch is synchronous
    /// storage work that reaches the one persistence writer: it runs as a
    /// [`aura_core_base::sync_boundary::SyncSection::Projection`] section on this owner, never on the
    /// reactor worker that drove the fetch.
    pub(crate) sync: aura_core_base::sync_boundary::SyncBoundary,
    operation_cancellations: Arc<P05OperationCancellationRegistry>,
}

impl crate::InitialSessionModelBindingResolver for P05Services {
    fn resolve_initial_session_model_binding(
        &self,
    ) -> Result<aura_model_domain::InitialSessionModelBinding> {
        let snapshot = self
            .model_config
            .snapshot()
            .context("failed to read the model config for initial Session binding")?;
        let default = self
            .model_config
            .default_model_selection()
            .context("failed to read the default model for initial Session binding")?;
        self.projections
            .initial_session_model_binding(&snapshot, &default)
            .context("failed to resolve the initial Session model binding")
    }
}

impl P05Services {
    /// The **process-startup** constructor.
    ///
    /// It performs the one-time responsibilities this service owns: resuming every incomplete
    /// model-configuration mutation journal, recovering an incomplete credential journal, and
    /// cancelling model operations a previous process left interrupted. These reconcile state
    /// a *previous* process left behind, so they must run exactly once, at startup.
    ///
    /// The startup half is named here and the construction half is
    /// [`Self::open_live`], so a live runtime that lost its model authority can be given one
    /// back **without** repeating any of the above against state this process already
    /// reconciled.
    pub fn open(
        persistence: Arc<PersistenceCoordinator>,
        settings: Arc<SettingsFacade>,
        bundle_namespace: BundleNamespace,
        sync: aura_core_base::sync_boundary::SyncBoundary,
    ) -> Result<Self> {
        let network_grants =
            Arc::new(PersistenceNetworkGrantStore::new(persistence.as_ref().clone()));
        let provider_credentials =
            Arc::new(PersistentProviderCredentialResolver::new(Arc::clone(&persistence)));
        let gateway = Arc::new(
            CoreHttpClientGateway::new(network_grants, None, NetworkControlOptions::default())
                .with_provider_credentials(provider_credentials),
        );
        let absolute_raw_stream_ceiling =
            ConfigurationTransportPolicyV1::agent_model_response_limit_bytes(u64::MAX)
                .context("P05 Agent response ceiling must be non-zero")?;
        let max_raw_stream_bytes = usize::try_from(absolute_raw_stream_ceiling)
            .context("P05 Agent response ceiling does not fit this platform")?;
        let transport_store = Arc::new(
            PersistentTransportStore::new(
                Arc::clone(&persistence),
                Arc::new(UnavailableAgentModelArtifactOwnerResolver),
                max_raw_stream_bytes,
            )
            .context("failed to initialize the persistent model transport store")?,
        );
        let model_config = Arc::new(ModelConfigService::new(Arc::clone(&settings)));
        let projections =
            P05ProjectionBuilder::new(Arc::clone(&persistence), Arc::clone(&model_config));
        let config_journal =
            ModelConfigJournalCoordinator::new(Arc::clone(&persistence), Arc::clone(&settings))
                .with_projection_builder(projections.clone());
        let credential_journal = P05CredentialJournal::new(Arc::clone(&persistence));
        // ---------------------------------------------------------------------------------
        // STARTUP-ONLY: reconcile what a previous process left behind.
        //
        // The work is extracted into
        // `reconcile_leftover_journals` so the **live** supply path can perform it through the very
        // same production entry when the startup path never reached it. A damaged `models.json`
        // means this constructor never ran, so the reconciliation was never done — and a live
        // supply that skipped it would publish an authority whose first responsibility was still
        // outstanding. The ledger in `service_rebuild` decides which of the two paths owns it, so it
        // happens exactly once either way.
        // ---------------------------------------------------------------------------------
        Self::reconcile_leftover_journals(
            &persistence,
            &config_journal,
            &credential_journal,
            &model_config,
            &projections,
        )?;
        // The clean-acceptance profile is validated **only** at startup: it asserts properties of a
        // run's starting state, so re-running it mid-run would assert something that is no longer
        // the claim. It is deliberately not part of the shareable reconciliation.
        if env::var_os("AURA_P05_CLEAN_ACCEPTANCE").is_some() {
            if env::var("AURA_P05_CLEAN_ACCEPTANCE").as_deref() != Ok("1") {
                anyhow::bail!("AURA_P05_CLEAN_ACCEPTANCE must be exactly 1 when present");
            }
            if bundle_namespace.trust_domain() != Some(ReleaseTrustDomain::Development) {
                anyhow::bail!(
                    "the clean P05 acceptance profile is restricted to a Development namespace"
                );
            }
            persistence
                .validate_clean_p05_acceptance_profile()
                .context("p05.clean_acceptance.integrity_check_failed")?;
            aura_core_base::append_core_log(
                "p05_clean_acceptance_profile_verified",
                serde_json::json!({
                    "bundle_namespace": bundle_namespace,
                    "legacy_ledger_count": 0,
                    "pending_journal_count": 0,
                    "nonterminal_attempt_count": 0,
                }),
            );
        }
        let provider_catalog = Arc::new(
            open_catalog(Arc::clone(&gateway), Arc::clone(&persistence))
                .context("failed to initialize the P05 provider catalog")?,
        );
        let provider_icons = ProviderIconService::new(
            Arc::clone(&gateway),
            Arc::clone(&provider_catalog),
            Arc::clone(&model_config),
            Arc::clone(&persistence),
        );
        // ---------------------------------------------------------------------------------
        // END STARTUP-ONLY. Everything below is construction from the process's existing
        // owners, and `open_live` performs exactly the same work.
        // ---------------------------------------------------------------------------------
        Self::assemble(
            persistence,
            settings,
            bundle_namespace,
            sync,
            gateway,
            transport_store,
            model_config,
            config_journal,
            credential_journal,
            projections,
            provider_catalog,
            provider_icons,
        )
    }

    /// Reconciles the state a **previous** process left behind, through the production entries.
    ///
    /// This is the startup-only half of `open`, extracted so
    /// there is exactly one implementation of it: the startup path calls it when it really opens the
    /// authority, and a live supply calls it when the startup path never got that far. Two copies
    /// would be two chances to drift, and the whole point is that the live path performs the *same*
    /// reconciliation rather than a weaker imitation.
    ///
    /// # What it does, and what it deliberately does not
    ///
    /// * it resumes every incomplete model-configuration mutation journal, including the credential
    ///   child phases, which is the existing production recovery;
    /// * it recovers an incomplete **credential** journal;
    /// * it cancels model operations a previous process left interrupted — a restart fact, not a
    ///   per-call one, which is why it belongs here.
    ///
    /// It never re-dispatches a call and never replays an `Unknown` result: the journal resume
    /// settles the *records* a previous process left, which is the bounded convergence the brief
    /// requires, not a re-execution.
    fn reconcile_leftover_journals(
        persistence: &Arc<PersistenceCoordinator>,
        config_journal: &ModelConfigJournalCoordinator,
        credential_journal: &P05CredentialJournal,
        model_config: &Arc<ModelConfigService>,
        projections: &P05ProjectionBuilder,
    ) -> Result<()> {
        for journal in persistence
            .incomplete_model_config_mutation_journals()
            .context("failed to load incomplete P05 model-config journals")?
        {
            let journal_id = journal.journal_id;
            let schema_version = journal.payload.schema_version;
            let recovery_phase = model_config_recovery_phase(journal.state);
            let recovery = match journal.payload.credential_child_phase() {
                Some(ModelConfigCredentialChildPhase::BeforeConfigPublish) => {
                    config_journal.resume_with_credential(journal, credential_journal, None)
                }
                Some(ModelConfigCredentialChildPhase::AfterConfigPublish) => {
                    config_journal.resume_with_credential_deletes(journal, credential_journal)
                }
                None => config_journal.resume(journal),
            };
            if let Err(error) = recovery
                && !matches!(&error, config_journal::ModelConfigJournalError::Aborted(_))
            {
                let error_code = model_config_recovery_error_code(&error);
                aura_core_base::append_core_log(
                    "model_config_journal_recovery_failed",
                    serde_json::json!({
                        "journal_id": journal_id,
                        "schema_version": schema_version,
                        "recovery_phase": recovery_phase,
                        "error_code": error_code,
                    }),
                );
                return Err(error).with_context(|| {
                    format!(
                        "failed to recover P05 model-config journal {journal_id} \
                             at {recovery_phase} with schema {schema_version} ({error_code})"
                    )
                });
            }
        }
        cancel_interrupted_model_operations(persistence)
            .context("failed to cancel P05 operations interrupted by application restart")?;
        let mut credential_projection_factory =
            |journal: &aura_persistence::CredentialMutationJournalRecord,
             status: aura_model_domain::EndpointCredentialStatus| {
                let record = persistence
                    .model_credential_record(journal.credential_id)
                    .map_err(P05CredentialJournalError::from)?
                    .ok_or_else(|| {
                        P05CredentialJournalError::Projection(
                            "credential recovery cannot resolve its owning endpoint".to_owned(),
                        )
                    })?;
                let snapshot = model_config
                    .snapshot()
                    .map_err(|error| P05CredentialJournalError::Projection(error.to_string()))?;
                projections
                    .credential_mutation_drafts(&snapshot, record.endpoint_id, status)
                    .map_err(|error| P05CredentialJournalError::Projection(error.to_string()))
            };
        credential_journal
            .recover(&mut credential_projection_factory)
            .context("failed to recover an incomplete P05 credential journal")?;
        Ok(())
    }

    /// Performs the **live supply's** share of the one-time startup responsibility.
    ///
    /// This is the same production reconciliation as
    /// [`Self::reconcile_leftover_journals`], reached through the already-constructed service, so a
    /// process that refused the model authority at startup can still discharge the responsibility it
    /// never met. It is called only by the supply path, and only when the process's ledger says the
    /// step really is outstanding.
    pub fn reconcile_at_live_supply(&self) -> Result<()> {
        Self::reconcile_leftover_journals(
            &self.persistence,
            &self.config_journal,
            &self.credential_journal,
            &self.model_config,
            &self.projections,
        )
    }

    /// The **live** constructor: every owner is the process's existing one, and no startup
    /// reconciliation is repeated.
    ///
    /// This is what a live configuration recovery calls, so a
    /// model authority refused because `models.json` was unreadable can be supplied **after**
    /// the domain is repaired, without a restart.
    ///
    /// # What it deliberately does not do
    ///
    /// * it does not resume incomplete model-configuration journals — the startup half settles
    ///   the rows a previous process left, and this process has no new ones to reconcile;
    /// * it does not cancel interrupted model operations — likewise a restart fact;
    /// * it does not validate the clean-acceptance profile — that check describes a run's
    ///   starting state, and re-running it mid-run would assert something that is no longer
    ///   the claim;
    /// * it opens **no** second database, journal or bounded boundary: it borrows the
    ///   process's own persistence, settings and boundary.
    ///
    /// What it does do is the service's own read-through of current configuration — the initial
    /// model projections — which is idempotent: it publishes the projection the current
    /// configuration already implies.
    pub fn open_live(
        persistence: Arc<PersistenceCoordinator>,
        settings: Arc<SettingsFacade>,
        bundle_namespace: BundleNamespace,
        sync: aura_core_base::sync_boundary::SyncBoundary,
    ) -> Result<Self> {
        let network_grants =
            Arc::new(PersistenceNetworkGrantStore::new(persistence.as_ref().clone()));
        let provider_credentials =
            Arc::new(PersistentProviderCredentialResolver::new(Arc::clone(&persistence)));
        let gateway = Arc::new(
            CoreHttpClientGateway::new(network_grants, None, NetworkControlOptions::default())
                .with_provider_credentials(provider_credentials),
        );
        let absolute_raw_stream_ceiling =
            ConfigurationTransportPolicyV1::agent_model_response_limit_bytes(u64::MAX)
                .context("P05 Agent response ceiling must be non-zero")?;
        let max_raw_stream_bytes = usize::try_from(absolute_raw_stream_ceiling)
            .context("P05 Agent response ceiling does not fit this platform")?;
        let transport_store = Arc::new(
            PersistentTransportStore::new(
                Arc::clone(&persistence),
                Arc::new(UnavailableAgentModelArtifactOwnerResolver),
                max_raw_stream_bytes,
            )
            .context("failed to initialize the persistent model transport store")?,
        );
        let model_config = Arc::new(ModelConfigService::new(Arc::clone(&settings)));
        let projections =
            P05ProjectionBuilder::new(Arc::clone(&persistence), Arc::clone(&model_config));
        let config_journal =
            ModelConfigJournalCoordinator::new(Arc::clone(&persistence), Arc::clone(&settings))
                .with_projection_builder(projections.clone());
        let credential_journal = P05CredentialJournal::new(Arc::clone(&persistence));
        let provider_catalog = Arc::new(
            open_catalog(Arc::clone(&gateway), Arc::clone(&persistence))
                .context("failed to initialize the P05 provider catalog")?,
        );
        let provider_icons = ProviderIconService::new(
            Arc::clone(&gateway),
            Arc::clone(&provider_catalog),
            Arc::clone(&model_config),
            Arc::clone(&persistence),
        );
        Self::assemble(
            persistence,
            settings,
            bundle_namespace,
            sync,
            gateway,
            transport_store,
            model_config,
            config_journal,
            credential_journal,
            projections,
            provider_catalog,
            provider_icons,
        )
    }

    /// Builds the service from parts that already exist.
    ///
    /// Both constructors end here, so a live supply cannot drift from the startup path: the
    /// service a recovery publishes is assembled by the same code that assembles the one a
    /// restart would produce.
    #[allow(clippy::too_many_arguments)]
    fn assemble(
        persistence: Arc<PersistenceCoordinator>,
        _settings: Arc<SettingsFacade>,
        bundle_namespace: BundleNamespace,
        sync: aura_core_base::sync_boundary::SyncBoundary,
        gateway: Arc<CoreHttpClientGateway>,
        transport_store: Arc<PersistentTransportStore>,
        model_config: Arc<ModelConfigService>,
        config_journal: ModelConfigJournalCoordinator,
        credential_journal: P05CredentialJournal,
        projections: P05ProjectionBuilder,
        provider_catalog: Arc<PersistentProviderCatalog>,
        provider_icons: ProviderIconService,
    ) -> Result<Self> {
        let initial_model_settings = projections
            .current_model_settings()
            .context("failed to build initial P05 ModelSettings projection")?;
        let initial_catalog = project_catalog(
            &provider_catalog
                .status()
                .context("failed to read initial P05 provider catalog status")?,
            &model_config.snapshot().context("failed to read initial P05 model config snapshot")?,
            &persistence,
        )
        .context("failed to build initial P05 ProviderCatalog projection")?;
        let mut initial_projection_drafts = vec![
            ModelProjectionDraft::ModelSettings(initial_model_settings),
            ModelProjectionDraft::ProviderCatalog(initial_catalog),
        ];
        projections
            .complete_model_config_projection_drafts(&mut initial_projection_drafts)
            .context("failed to complete initial P05 model projections")?;
        persistence
            .commit_model_projection_snapshots(initial_projection_drafts)
            .context("failed to initialize P05 projection state")?;
        provider_icons.schedule_missing();
        if tokio::runtime::Handle::try_current().is_ok()
            && provider_catalog.needs_refresh(aura_domain_common::UtcTimestamp::now())
            && let Some(ownership) = provider_catalog
                .begin_refresh()
                .context("failed to enter P05 provider-catalog refresh state")?
        {
            // The occupancy's one owner exists before the first fallible step below: the loading
            // projection build and its commit both return `?`, and dropping the handle on either
            // of those exits is what gives the slot back instead of leaving the catalog
            // "refreshing" for the life of the process.
            let loading_catalog = project_catalog(
                &provider_catalog
                    .status()
                    .context("failed to read P05 provider-catalog loading state")?,
                &model_config
                    .snapshot()
                    .context("failed to read P05 model config for catalog refresh")?,
                &persistence,
            )
            .context("failed to build P05 provider-catalog loading projection")?;
            persistence
                .commit_model_projection_snapshots(vec![ModelProjectionDraft::ProviderCatalog(
                    loading_catalog,
                )])
                .context("failed to publish P05 provider-catalog loading projection")?;
            // The refresh now belongs to the task this starts, which releases the slot when its
            // own storage half really ends.
            drop(spawn_catalog_refresh(
                sync.clone(),
                Arc::clone(&provider_catalog),
                Arc::clone(&model_config),
                Arc::clone(&persistence),
                provider_icons.clone(),
                ownership,
            ));
        }
        let configuration_transport = ConfigurationTransport::new(
            Arc::clone(&gateway),
            Arc::clone(&persistence),
            Arc::clone(&transport_store),
        );
        Ok(Self {
            model_config: Arc::clone(&model_config),
            model_gateway: gateway,
            provider_catalog,
            configuration_transport,
            persistence: Arc::clone(&persistence),
            bundle_namespace,
            config_journal,
            credential_journal,
            projections,
            provider_icons: provider_icons.clone(),
            sync,
            operation_cancellations: Arc::new(P05OperationCancellationRegistry::default()),
        })
    }

    pub fn rebuild_model_projections_if_stale(&self) -> Result<Vec<ProjectionChange>> {
        let current_settings = self
            .projections
            .current_model_settings()
            .context("failed to build current P05 ModelSettings projection")?;
        let already_current = self
            .persistence
            .latest_model_projection(ProjectionKind::ModelSettings, ProjectionScope::Global)
            .context("failed to read the latest P05 ModelSettings projection")?
            .is_some_and(|change| {
                matches!(
                    change,
                    ProjectionChange::ModelSettings { payload, .. }
                        if *payload == current_settings
                )
            });
        if already_current {
            return Ok(Vec::new());
        }

        let mut drafts = vec![ModelProjectionDraft::ModelSettings(current_settings)];
        self.projections
            .complete_model_config_projection_drafts(&mut drafts)
            .context("failed to rebuild all P05 model projection drafts")?;
        let changes = self
            .persistence
            .commit_model_projection_snapshots(drafts)
            .context("failed to commit rebuilt P05 model projections")?;
        self.persistence
            .publish_model_projection_changes(&changes)
            .context("failed to publish rebuilt P05 model projections")?;
        Ok(changes)
    }

    /// Starts a scheduled catalog refresh when one is due, and says whether one really started.
    ///
    /// The path itself lives in [`catalog::refresh_catalog_if_due_for`], which is the one
    /// implementation of the scheduled announcement and its handover; this method only fills in
    /// the process's own services.
    pub async fn refresh_catalog_if_due(&self) -> Result<bool> {
        catalog::refresh_catalog_if_due_for(catalog::CatalogRefreshPath {
            sync: self.sync.clone(),
            catalog: Arc::clone(&self.provider_catalog),
            model_config: Arc::clone(&self.model_config),
            persistence: Arc::clone(&self.persistence),
            provider_icons: self.provider_icons.clone(),
        })
        .await
    }
}

fn cancel_interrupted_model_operations(
    persistence: &PersistenceCoordinator,
) -> Result<(), aura_persistence::PersistenceError> {
    let recovery = persistence.configuration_transport_recovery_snapshot()?;
    for degradation in &recovery.historical_integrity_degradations {
        aura_core_base::append_core_log(
            "p05_historical_integrity_degradation",
            serde_json::json!({
                "migration_key": degradation.migration_key,
                "resolution": "legacy_indeterminate",
                "transport_attempt_id": degradation.transport_attempt_id,
                "discovery_run_id": degradation.discovery_run_id,
                "failure_id": degradation.failure_id,
                "discovery_failed_event_id": degradation.discovery_failed_event_id,
                "original_state": degradation.original_state,
                "source_schema_version": degradation.source_schema_version,
                "canonical_evidence_hash": degradation.canonical_evidence_hash,
                "reconciled_at": degradation.reconciled_at,
            }),
        );
    }
    let interrupted_attempts = recovery.runtime_safety_blockers;

    // Only an already-cancelled owner supplies enough durable evidence to
    // forward-cancel its legacy non-terminal Attempt. Every current active or
    // otherwise-terminal owner remains a RuntimeSafetyBlocker; startup must
    // not infer whether a DispatchPrepared-or-later request reached Provider.
    let mut safely_cancelled_attempts = Vec::new();
    for attempt in &interrupted_attempts {
        match &attempt.request_evidence.operation_context {
            TransportOperationContext::ModelDiscovery { discovery_run_id } => {
                let record = persistence.model_discovery_run(*discovery_run_id)?.ok_or_else(|| {
                    aura_persistence::PersistenceError::InvalidRequest(format!(
                        "non-terminal configuration transport attempt {} has no discovery owner",
                        attempt.id
                    ))
                })?;
                if record.endpoint_id != attempt.endpoint_id {
                    return Err(aura_persistence::PersistenceError::InvalidRequest(format!(
                        "non-terminal discovery transport attempt {} contradicts its owner",
                        attempt.id
                    )));
                }
                if record.state == ModelDiscoveryState::Cancelled {
                    safely_cancelled_attempts.push((attempt.clone(), record.correlation_id));
                } else {
                    return Err(aura_persistence::PersistenceError::InvalidRequest(format!(
                        "current discovery {} owns non-terminal runtime safety blocker {}",
                        record.id, attempt.id
                    )));
                }
            }
            TransportOperationContext::ModelAvailabilityTest {
                model_availability_test_id,
                model_profile_id,
            } => {
                let record = persistence
                    .model_availability_test(*model_availability_test_id)?
                    .ok_or_else(|| {
                        aura_persistence::PersistenceError::InvalidRequest(format!(
                            "non-terminal configuration transport attempt {} has no availability-test owner",
                            attempt.id
                        ))
                    })?;
                if record.endpoint_id != attempt.endpoint_id
                    || record.model_profile_id != *model_profile_id
                {
                    return Err(aura_persistence::PersistenceError::InvalidRequest(format!(
                        "non-terminal availability transport attempt {} contradicts its owner",
                        attempt.id
                    )));
                }
                if record.state == ModelAvailabilityTestState::Cancelled {
                    safely_cancelled_attempts.push((attempt.clone(), record.correlation_id));
                } else {
                    return Err(aura_persistence::PersistenceError::InvalidRequest(format!(
                        "current availability test {} owns non-terminal runtime safety blocker {}",
                        record.id, attempt.id
                    )));
                }
            }
            TransportOperationContext::AgentModelAttempt { .. } => {
                unreachable!("the P05 recovery query excludes Agent Model transport attempts")
            }
        }
    }

    // The owner is already durably Cancelled, so this write does not infer a
    // Provider result and only closes an incomplete cancellation sequence.
    for (interrupted, correlation_id) in &safely_cancelled_attempts {
        let mut cancelled = interrupted.clone();
        cancelled.state = TransportAttemptState::Cancelled;
        cancelled.finished_at = Some(aura_domain_common::UtcTimestamp::now());
        persistence.cancel_transport_attempt(
            *correlation_id,
            &cancelled,
            CancellationSource::CoreShutdown,
        )?;
    }

    for mut record in persistence.active_model_discovery_runs()? {
        for attempt in &safely_cancelled_attempts {
            if matches!(
                &attempt.0.request_evidence.operation_context,
                TransportOperationContext::ModelDiscovery { discovery_run_id }
                    if *discovery_run_id == record.id
            ) && !record.transport_attempt_ids.contains(&attempt.0.id)
            {
                record.transport_attempt_ids.push(attempt.0.id);
            }
        }
        let expected = record.revision;
        record.revision = Revision::new(expected.value().checked_add(1).ok_or_else(|| {
            aura_persistence::PersistenceError::InvalidRequest(
                "model discovery revision exhausted during restart recovery".to_owned(),
            )
        })?);
        record.state = ModelDiscoveryState::Cancelled;
        record.outcome = None;
        record.cancellation_reason = Some(ModelOperationCancellationReason::ApplicationRestarted);
        record.result = None;
        record.failure_id = None;
        record.provider_error = None;
        record.finished_at = Some(aura_domain_common::UtcTimestamp::now());
        persistence.transition_model_discovery_run(
            expected,
            &record,
            Some(ModelDiscoveryTerminalEventContext::Cancelled {
                cancelled_by: ModelOperationCancellationReason::ApplicationRestarted,
            }),
            Vec::new(),
        )?;
    }
    for mut record in persistence.active_model_availability_tests()? {
        for attempt in &safely_cancelled_attempts {
            if matches!(
                &attempt.0.request_evidence.operation_context,
                TransportOperationContext::ModelAvailabilityTest {
                    model_availability_test_id,
                    model_profile_id,
                } if *model_availability_test_id == record.id
                    && *model_profile_id == record.model_profile_id
            ) && !record.direct_transport_attempt_ids.contains(&attempt.0.id)
            {
                record.direct_transport_attempt_ids.push(attempt.0.id);
            }
        }
        let expected = record.revision;
        record.revision = Revision::new(expected.value().checked_add(1).ok_or_else(|| {
            aura_persistence::PersistenceError::InvalidRequest(
                "availability-test revision exhausted during restart recovery".to_owned(),
            )
        })?);
        record.state = ModelAvailabilityTestState::Cancelled;
        record.cancellation_reason = Some(ModelOperationCancellationReason::ApplicationRestarted);
        record.result = None;
        record.completed_at = Some(aura_domain_common::UtcTimestamp::now());
        persistence.transition_model_availability_test(
            expected,
            &record,
            Some(ModelOperationCancellationReason::ApplicationRestarted),
            Vec::new(),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use aura_domain_common::{
        ContentHash, CorrelationId, EndpointId, ModelDiscoveryRunId, NetworkRequestId, Revision,
        RuntimeInstanceId, TransportAttemptId, UtcTimestamp,
    };
    use aura_model_domain::{
        ModelAdapterIdentity, ModelApiFormat, ModelDiscoveryRunRecord, ModelDiscoveryState,
        ProviderRequestEvidence, ProviderRequestHeaderClass, ProviderRequestHeaderMetadata,
        ProviderRequestTargetMetadata, TransportAttemptKind, TransportAttemptRecord,
        TransportAttemptState, TransportOperationContext,
    };
    use aura_permission_domain::NormalizedOrigin;
    use aura_persistence::{PersistenceOptions, PersistenceStartup};

    use super::*;

    #[test]
    fn restart_recovery_blocks_on_a_current_nonterminal_attempt_without_inference() {
        let directory = tempfile::tempdir().expect("temporary P05 recovery root");
        let database_path = directory.path().join("data/aura.db");
        let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
            database_path.clone(),
            directory.path().join("data/artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("start test persistence")
        {
            PersistenceStartup::Ready(persistence) => persistence,
            PersistenceStartup::RecoveryRequired(recovery) => {
                panic!("unexpected recovery requirement: {}", recovery.reason)
            }
        };
        let endpoint_id = EndpointId::new();
        let discovery_run_id = ModelDiscoveryRunId::new();
        let correlation_id = CorrelationId::new();
        let endpoint_fingerprint = ContentHash::sha256(b"restart-recovery-endpoint");
        let discovery = ModelDiscoveryRunRecord {
            id: discovery_run_id,
            correlation_id,
            endpoint_id,
            source_models_revision: Revision::INITIAL,
            endpoint_revision_at_start: Revision::INITIAL,
            endpoint_fingerprint: endpoint_fingerprint.clone(),
            revision: Revision::INITIAL,
            state: ModelDiscoveryState::Queued,
            stage: None,
            outcome: None,
            cancellation_reason: None,
            transport_attempt_ids: Vec::new(),
            result: None,
            failure_id: None,
            provider_error: None,
            raw_response_artifact_id: None,
            started_at: UtcTimestamp::now(),
            finished_at: None,
        };
        persistence.create_model_discovery_run(&discovery, Vec::new()).expect("queue discovery");

        let body = br#"{"probe":true}"#;
        let attempt = TransportAttemptRecord {
            id: TransportAttemptId::new(),
            attempt_number: 1,
            attempt_kind: TransportAttemptKind::Initial,
            client_request_id: NetworkRequestId::new().to_string(),
            provider_request_id: None,
            endpoint_id,
            endpoint_fingerprint,
            request_evidence: ProviderRequestEvidence {
                body_sha256: ContentHash::sha256(body),
                body_byte_length: u64::try_from(body.len()).expect("body length"),
                target: ProviderRequestTargetMetadata {
                    method: "POST".to_owned(),
                    origin: NormalizedOrigin {
                        scheme: "https".to_owned(),
                        host: "api.example.test".to_owned(),
                        port: 443,
                    },
                    path: "/v1/models".to_owned(),
                    query_parameter_names: BTreeSet::new(),
                },
                headers: vec![ProviderRequestHeaderMetadata {
                    name: "content-type".to_owned(),
                    class: ProviderRequestHeaderClass::ContentMetadata,
                    non_secret_value: Some("application/json".to_owned()),
                }],
                compatibility_removals: BTreeSet::new(),
                applied_output_token_field_alias: None,
                adapter: ModelAdapterIdentity::v1(ModelApiFormat::OpenAiResponses),
                operation_context: TransportOperationContext::ModelDiscovery { discovery_run_id },
                tool_registry_snapshot_ref: None,
                tool_name_bindings: Vec::new(),
                historical_tool_call_wire_bindings: Vec::new(),
                canonical_body_artifact_id: None,
            },
            state: TransportAttemptState::AuthorizingNetwork,
            started_at: UtcTimestamp::now(),
            first_byte_at: None,
            first_model_event_at: None,
            finished_at: None,
            raw_stream_artifact_id: None,
            failure_id: None,
        };
        persistence
            .create_transport_attempt(correlation_id, &attempt)
            .expect("persist interrupted transport attempt");
        assert_eq!(
            persistence
                .nonterminal_configuration_transport_attempts()
                .expect("load interrupted attempts"),
            vec![attempt.clone()]
        );

        let error = cancel_interrupted_model_operations(&persistence)
            .expect_err("current non-terminal attempt must block Ready");
        assert!(error.to_string().contains("runtime safety blocker"));

        let recovered_attempt = persistence
            .transport_attempt(attempt.id)
            .expect("load recovered attempt")
            .expect("recovered attempt exists");
        assert_eq!(recovered_attempt.state, TransportAttemptState::AuthorizingNetwork);
        let recovered_discovery = persistence
            .model_discovery_run(discovery_run_id)
            .expect("load recovered discovery")
            .expect("recovered discovery exists");
        assert_eq!(recovered_discovery.state, ModelDiscoveryState::Queued);
        assert_eq!(recovered_discovery.cancellation_reason, None);
        assert!(recovered_discovery.transport_attempt_ids.is_empty());
        drop(persistence);

        let connection = rusqlite::Connection::open(database_path).expect("open event database");
        let events = connection
            .prepare("SELECT event_type, payload_json FROM events ORDER BY global_sequence")
            .expect("prepare event query")
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .expect("query events")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect events");
        assert!(events.iter().all(|(event_type, _)| {
            event_type != "ProviderTransportAttemptCancelled"
                && event_type != "ModelDiscoveryCancelled"
        }));
    }

    #[test]
    fn restart_recovery_only_forward_cancels_attempts_with_an_already_cancelled_owner() {
        let directory = tempfile::tempdir().expect("temporary P05 recovery root");
        let database_path = directory.path().join("data/aura.db");
        let persistence = match PersistenceCoordinator::start(PersistenceOptions::new(
            database_path.clone(),
            directory.path().join("data/artifacts"),
            RuntimeInstanceId::new(),
        ))
        .expect("start test persistence")
        {
            PersistenceStartup::Ready(persistence) => persistence,
            PersistenceStartup::RecoveryRequired(recovery) => {
                panic!("unexpected recovery requirement: {}", recovery.reason)
            }
        };
        let endpoint_id = EndpointId::new();
        let discovery_run_id = ModelDiscoveryRunId::new();
        let correlation_id = CorrelationId::new();
        let endpoint_fingerprint = ContentHash::sha256(b"cancelled-owner-endpoint");
        let mut discovery = ModelDiscoveryRunRecord {
            id: discovery_run_id,
            correlation_id,
            endpoint_id,
            source_models_revision: Revision::INITIAL,
            endpoint_revision_at_start: Revision::INITIAL,
            endpoint_fingerprint: endpoint_fingerprint.clone(),
            revision: Revision::INITIAL,
            state: ModelDiscoveryState::Queued,
            stage: None,
            outcome: None,
            cancellation_reason: None,
            transport_attempt_ids: Vec::new(),
            result: None,
            failure_id: None,
            provider_error: None,
            raw_response_artifact_id: None,
            started_at: UtcTimestamp::now(),
            finished_at: None,
        };
        persistence.create_model_discovery_run(&discovery, Vec::new()).expect("queue discovery");
        discovery.revision = Revision::new(2);
        discovery.state = ModelDiscoveryState::Cancelled;
        discovery.cancellation_reason = Some(ModelOperationCancellationReason::UserRequested);
        discovery.finished_at = Some(UtcTimestamp::now());
        persistence
            .transition_model_discovery_run(
                Revision::INITIAL,
                &discovery,
                Some(ModelDiscoveryTerminalEventContext::Cancelled {
                    cancelled_by: ModelOperationCancellationReason::UserRequested,
                }),
                Vec::new(),
            )
            .expect("commit cancelled discovery owner");

        let body = br#"{"probe":true}"#;
        let attempt = TransportAttemptRecord {
            id: TransportAttemptId::new(),
            attempt_number: 1,
            attempt_kind: TransportAttemptKind::Initial,
            client_request_id: NetworkRequestId::new().to_string(),
            provider_request_id: None,
            endpoint_id,
            endpoint_fingerprint,
            request_evidence: ProviderRequestEvidence {
                body_sha256: ContentHash::sha256(body),
                body_byte_length: u64::try_from(body.len()).expect("body length"),
                target: ProviderRequestTargetMetadata {
                    method: "POST".to_owned(),
                    origin: NormalizedOrigin {
                        scheme: "https".to_owned(),
                        host: "api.example.test".to_owned(),
                        port: 443,
                    },
                    path: "/v1/models".to_owned(),
                    query_parameter_names: BTreeSet::new(),
                },
                headers: vec![ProviderRequestHeaderMetadata {
                    name: "content-type".to_owned(),
                    class: ProviderRequestHeaderClass::ContentMetadata,
                    non_secret_value: Some("application/json".to_owned()),
                }],
                compatibility_removals: BTreeSet::new(),
                applied_output_token_field_alias: None,
                adapter: ModelAdapterIdentity::v1(ModelApiFormat::OpenAiResponses),
                operation_context: TransportOperationContext::ModelDiscovery { discovery_run_id },
                tool_registry_snapshot_ref: None,
                tool_name_bindings: Vec::new(),
                historical_tool_call_wire_bindings: Vec::new(),
                canonical_body_artifact_id: None,
            },
            state: TransportAttemptState::DispatchPrepared,
            started_at: UtcTimestamp::now(),
            first_byte_at: None,
            first_model_event_at: None,
            finished_at: None,
            raw_stream_artifact_id: None,
            failure_id: None,
        };
        persistence
            .create_transport_attempt(correlation_id, &attempt)
            .expect("persist incomplete post-cancellation attempt");

        cancel_interrupted_model_operations(&persistence)
            .expect("already-cancelled owner permits cancellation-only forward recovery");
        let recovered_attempt = persistence
            .transport_attempt(attempt.id)
            .expect("load recovered attempt")
            .expect("recovered attempt exists");
        assert_eq!(recovered_attempt.state, TransportAttemptState::Cancelled);
        let recovered_discovery = persistence
            .model_discovery_run(discovery_run_id)
            .expect("load recovered discovery")
            .expect("recovered discovery exists");
        assert_eq!(recovered_discovery, discovery);
        drop(persistence);

        let connection = rusqlite::Connection::open(database_path).expect("open event database");
        let attempt_cancelled_events = connection
            .query_row(
                "SELECT COUNT(*) FROM events
                 WHERE event_type='ProviderTransportAttemptCancelled'
                   AND json_extract(payload_json, '$.transport_attempt_id')=?1",
                [attempt.id.to_string()],
                |row| row.get::<_, i64>(0),
            )
            .expect("count terminal Attempt events");
        assert_eq!(attempt_cancelled_events, 1);
    }
}
