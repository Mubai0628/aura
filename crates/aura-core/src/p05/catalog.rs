use std::sync::Arc;

use anyhow::Context;
use aura_domain_common::{CorrelationId, NetworkRequestId, PrincipalId, Revision, UtcTimestamp};
use aura_model_catalog::{
    CatalogCache, CatalogConditionalRequest, CatalogError, CatalogRequestAuthorizer, CatalogSource,
    CoreGatewayCatalogSource, PersistenceCatalogCache, ProviderCatalogService,
    ProviderConfigurationPolicy, project_provider_cards, project_provider_model_page,
};
use aura_model_config::{ModelConfigService, ModelConfigSnapshot};
use aura_model_domain::{
    ModelCatalogProviderGetRequest, ProviderCatalogConfigurationClass,
    ProviderCatalogConnectionDefaultsView, ProviderCatalogEntry, ProviderCatalogModelPage,
    ProviderCatalogProjection, ProviderCatalogStatus, ProviderPresetVariantView,
    has_trusted_provider_preset, provider_preset_registry_v1,
};
use aura_network_control::{
    CoreHttpClientGateway, GatewayRequest, NetworkControlPlane, StandardNetworkRequestGrant,
};
use aura_permission_domain::{
    CoreRequestGrant, NetworkByteLimits, NetworkDestinationPolicy, NetworkProtocolClass,
    NetworkRequestPlan, RedirectPolicy,
};
use aura_security_contract::{
    InvocationActor, InvocationIdentity, NetworkCallerKind, NetworkDataClass,
    NetworkDestinationClass,
};
use thiserror::Error;
use tokio::task::JoinHandle;

use super::provider_icon::ProviderIconService;

const MODELS_DEV_CATALOG_URL: &str = "https://models.dev/api.json";
const CATALOG_REQUEST_TIMEOUT_MS: u64 = 30_000;
/// models.dev's catalog passed 5 MiB in October 2026. The
/// earlier 5 MiB limit refused every refresh as `provider.catalog_response_too_large`, so no
/// discovered model got its window or output limit. 32 MiB leaves room for its growth.
const CATALOG_MAX_RESPONSE_BYTES: u64 = 32 * 1024 * 1024;
const CATALOG_GRANT_TTL_SECONDS: i64 = 30;

pub(crate) type PersistentProviderCatalog = ProviderCatalogService<
    CoreGatewayCatalogSource<P05CatalogRequestAuthorizer>,
    PersistenceCatalogCache,
>;

#[derive(Debug, Error)]
pub(crate) enum P05CatalogError {
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error(transparent)]
    Persistence(#[from] aura_persistence::PersistenceError),
}

pub(crate) struct P05CatalogRequestAuthorizer {
    gateway: Arc<CoreHttpClientGateway>,
}

impl P05CatalogRequestAuthorizer {
    #[must_use]
    pub(crate) fn new(gateway: Arc<CoreHttpClientGateway>) -> Self {
        Self { gateway }
    }
}

impl CatalogRequestAuthorizer for P05CatalogRequestAuthorizer {
    async fn authorize_catalog_request(
        &self,
        conditional: CatalogConditionalRequest,
    ) -> Result<(CoreRequestGrant<StandardNetworkRequestGrant>, GatewayRequest), CatalogError> {
        let request_id = NetworkRequestId::new();
        let mut headers = vec![("accept-encoding".to_owned(), "gzip, br, zstd".to_owned())];
        if let Some(etag) = conditional.etag {
            headers.push(("if-none-match".to_owned(), etag));
        }
        if let Some(last_modified) = conditional.last_modified {
            headers.push(("if-modified-since".to_owned(), last_modified));
        }
        let plan = NetworkRequestPlan {
            request_id,
            identity: InvocationIdentity {
                principal_id: PrincipalId::new(),
                actor: InvocationActor::SystemRuntime,
                workspace_id: None,
                session_id: None,
                turn_task_id: None,
                agent_run_id: None,
                parent_agent_run_id: None,
                correlation_id: CorrelationId::new(),
                permission_snapshot_id: None,
            },
            caller: NetworkCallerKind::ModelsCatalog,
            method: "GET".to_owned(),
            requested_url: MODELS_DEV_CATALOG_URL.to_owned(),
            destination: None,
            request_data: Vec::new(),
            data_classes: vec![NetworkDataClass::PublicQuery],
            redirect_policy: RedirectPolicy::SameOrigin { max_hops: 3 },
            byte_limits: catalog_byte_limits(),
            timeout_ms: CATALOG_REQUEST_TIMEOUT_MS,
            credential_refs: Vec::new(),
            network_grant_ids: Vec::new(),
        };
        let authorized = self.gateway.authorize(plan).await?;
        let destination = authorized.destination;
        let grant = CoreRequestGrant {
            logical_request_id: request_id,
            purpose: StandardNetworkRequestGrant,
            caller: NetworkCallerKind::ModelsCatalog,
            destination_policy: NetworkDestinationPolicy {
                protocol: NetworkProtocolClass::Http,
                host: destination.host,
                port: destination.port,
                allowed_destination_classes: vec![NetworkDestinationClass::Public],
            },
            methods: vec!["GET".to_owned()],
            redirect_policy: authorized.plan.redirect_policy,
            data_classes: authorized.plan.data_classes.clone(),
            byte_limits: authorized.plan.byte_limits.clone(),
            expires_at: UtcTimestamp::from_value(
                UtcTimestamp::now().value() + time::Duration::seconds(CATALOG_GRANT_TTL_SECONDS),
            ),
        };
        Ok((grant, GatewayRequest { plan: authorized.plan, headers, body: Vec::new() }))
    }
}

fn catalog_byte_limits() -> NetworkByteLimits {
    NetworkByteLimits {
        max_upload_bytes: 1,
        max_download_bytes: CATALOG_MAX_RESPONSE_BYTES,
        max_decompressed_bytes: CATALOG_MAX_RESPONSE_BYTES,
    }
}

pub(crate) fn open_catalog(
    gateway: Arc<CoreHttpClientGateway>,
    persistence: Arc<aura_persistence::PersistenceCoordinator>,
) -> Result<PersistentProviderCatalog, CatalogError> {
    ProviderCatalogService::open(
        CoreGatewayCatalogSource::new(gateway.clone(), P05CatalogRequestAuthorizer::new(gateway)),
        PersistenceCatalogCache::new(persistence),
    )
}

pub(crate) fn project_catalog(
    status: &ProviderCatalogStatus,
    config: &ModelConfigSnapshot,
    persistence: &aura_persistence::PersistenceCoordinator,
) -> Result<ProviderCatalogProjection, P05CatalogError> {
    let icons = persistence.provider_catalog_icons()?;
    let cards = project_provider_cards(status, &config.registry, &CatalogProjectionPolicy, &icons)?;
    Ok(ProviderCatalogProjection {
        state: status.state,
        catalog_revision: status.snapshot.as_ref().map(|snapshot| snapshot.catalog_revision),
        catalog_content_hash: status
            .snapshot
            .as_ref()
            .map(|snapshot| snapshot.catalog_content_hash.clone()),
        checked_at: status.checked_at,
        last_successful_refresh_at: status.last_successful_refresh_at,
        cards,
        last_error_code: status.last_error_code.clone(),
        projection_revision: Revision::INITIAL,
    })
}

pub(crate) fn catalog_configuration_class(
    entry: &ProviderCatalogEntry,
) -> ProviderCatalogConfigurationClass {
    if has_trusted_provider_preset(&entry.catalog_provider_id) {
        return ProviderCatalogConfigurationClass::TrustedPreset;
    }
    if validated_models_dev_api_suggestion(entry).is_some() {
        ProviderCatalogConfigurationClass::ManualConfiguration
    } else {
        ProviderCatalogConfigurationClass::CatalogInformationOnly
    }
}

struct CatalogProjectionPolicy;

impl ProviderConfigurationPolicy for CatalogProjectionPolicy {
    fn class_for(&self, entry: &ProviderCatalogEntry) -> ProviderCatalogConfigurationClass {
        catalog_configuration_class(entry)
    }

    fn connection_defaults_for(
        &self,
        entry: &ProviderCatalogEntry,
    ) -> Option<ProviderCatalogConnectionDefaultsView> {
        let variants = provider_preset_registry_v1()
            .into_iter()
            .filter(|preset| preset.catalog_provider_id == entry.catalog_provider_id)
            .map(|preset| ProviderPresetVariantView {
                variant_id: preset.variant_id.to_owned(),
                display_name: preset.display_name.to_owned(),
                base_url: preset.base_url.to_owned(),
                api_format: preset.api_format,
                auth_scheme: preset.auth_scheme,
                discovery_mode: preset.discovery_mode,
                chat_instruction_role: preset.chat_instruction_role,
                chat_output_token_field: preset.chat_output_token_field,
                recommended: preset.recommended,
            })
            .collect::<Vec<_>>();
        if !variants.is_empty() {
            return Some(ProviderCatalogConnectionDefaultsView::AuraPreset { variants });
        }
        validated_models_dev_api_suggestion(entry).map(|base_url| {
            ProviderCatalogConnectionDefaultsView::ModelsDevApiSuggestion { base_url }
        })
    }
}

fn validated_models_dev_api_suggestion(entry: &ProviderCatalogEntry) -> Option<String> {
    let value = entry.api_url.as_ref()?;
    let url = url::Url::parse(value).ok()?;
    (url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none())
    .then(|| value.clone())
}

pub(crate) fn project_catalog_provider_page(
    status: &ProviderCatalogStatus,
    config: &ModelConfigSnapshot,
    request: &ModelCatalogProviderGetRequest,
    persistence: &aura_persistence::PersistenceCoordinator,
) -> Result<ProviderCatalogModelPage, P05CatalogError> {
    let icons = persistence.provider_catalog_icons()?;
    Ok(project_provider_model_page(
        status,
        &config.registry,
        &CatalogProjectionPolicy,
        request,
        &icons,
    )?)
}

/// Refresh the Provider catalog in the background, on the bounded synchronous owner.
///
/// WHY THIS IS TWO PHASES AND NOT ONE BLOCKING CALL.
/// Every step of a refresh after the fetch ends in a synchronous storage call: the failure path
/// persists the issue code, the success path persists the snapshot, and the tail commits the
/// provider-catalog projection. All of them go through the one persistence writer, whose
/// admission point (`Point::QueueAdmission`) waits for the storage lane - measurably for seconds
/// when that lane is held. A reactor worker inside one of those waits is not "a slow task":
/// `crate::sync_boundary`'s module doc states the contract that forbids it, and the measured
/// consequence is that the runtime's other workers sit on their own condvar with no timeout while
/// nothing is left to poll the timer driver.
///
/// Wrapping the *whole* refresh in one blocking call would also keep a reactor worker out of the
/// writer, but it would put the network fetch on a blocking thread and hold a boundary permit for
/// the length of the request, and it would move the work to a pool that is not the process's
/// bounded owner: the synchronous steps would then be unbounded, unrecorded and unrefusable,
/// which is the one thing this boundary exists to prevent. So the fetch stays asynchronous and
/// only the storage half is a section:
///
/// * [`ProviderCatalogService::fetch_refresh`] - asynchronous network work, no storage.
/// * this bounded [`SyncSection::Projection`] section - the cache write, the status write, the
///   projection read, the projection commit and its publication.
///
/// WHO OWNS WHAT
/// * `ownership` is the refresh slot's one owner, taken by [`ProviderCatalogService::begin_refresh`]
///   and handed here by whoever started the refresh. It is carried by the future below from its
///   first poll and then moved into the commit section, so it is released by the work that really
///   finishes the refresh - never by the caller's budget. Every exit is covered by construction:
///   a cancellation before the first poll or during the fetch drops the future and releases it; a
///   panic in the fetch unwinds the future and releases it; a refused section drops the closure
///   with the handle inside it and releases it; the commit section releases it when its own real
///   work ends, including when that work panics.
/// * A refusal is never reported as a successful refresh and never as an empty catalog. A refused
///   section provably did not run, so its closure is dropped with the handle inside it,
///   `last_successful_refresh_at` is untouched and the status stays the real
///   `Refreshing…`/`Loading…` state `begin_refresh` published. `needs_refresh` is therefore still
///   true and the pre-existing P05 maintenance check ([`P05Services::refresh_catalog_if_due`], the
///   same 60 s budget that started this refresh) retries on its existing budget. No new retry
///   mechanism, no new error vocabulary.
/// * Every real error of the synchronous half - the cache write, the status read, the model config
///   read, the projection build, the projection commit and the publication - is recorded on its
///   own in the section's outcome and written to the Core log by the thread that really ran the
///   work, so a late result is settled rather than swallowed and no error is reported as a
///   successful refresh.
pub(crate) fn spawn_catalog_refresh<S, C>(
    sync: crate::sync_boundary::SyncBoundary,
    catalog: Arc<ProviderCatalogService<S, C>>,
    model_config: Arc<ModelConfigService>,
    persistence: Arc<aura_persistence::PersistenceCoordinator>,
    provider_icons: ProviderIconService,
    ownership: aura_model_catalog::RefreshOwnership,
) -> JoinHandle<()>
where
    S: CatalogSource + 'static,
    C: CatalogCache + 'static,
{
    tokio::spawn(async move {
        // The occupancy is held by this future from its first poll. The handle is the proof of
        // ownership, so there is nothing to check and no way to start a fetch for a refresh this
        // task does not own.
        let ownership = ownership;
        // ASYNCHRONOUS HALF: the network fetch. No cache row, no artifact, no status write, and
        // therefore no reactor worker waiting on the persistence writer.
        let fetched = catalog.fetch_refresh().await;
        // SYNCHRONOUS HALF: one bounded section on the process's single owner. Nothing inside it
        // runs on a reactor worker.
        let section_catalog = Arc::clone(&catalog);
        let section_config = Arc::clone(&model_config);
        let section_persistence = Arc::clone(&persistence);
        let section_icons = provider_icons;
        let admission = sync
            .run_settled(
                crate::sync_boundary::SyncSection::Projection,
                crate::sync_boundary::SyncSubject::new().method("provider_catalog.refresh"),
                move || {
                    let outcome = commit_and_publish_catalog_refresh(
                        &section_catalog,
                        &section_config,
                        &section_persistence,
                        &section_icons,
                        fetched,
                    );
                    // The refresh is over when this section's real work ends, so its one owner -
                    // this handle - is dropped here, by the thread that really ran the work. The
                    // release is a separate fact from the outcome and is recorded as one.
                    drop(ownership);
                    outcome
                },
                {
                    // A late result has an owner: this sink is invoked once, by the thread that
                    // really ran the work, even when the caller's budget expired long before.
                    move |outcome: &CatalogRefreshSettlement| {
                        outcome.record("provider_catalog.refresh_settled");
                    }
                },
            )
            .await;
        match &admission {
            crate::blocking_pool::BlockingAdmission::Refused { waited_ms } => {
                // The section provably did not run, so its closure was dropped with the occupancy
                // inside it: the slot is already back and the scheduled check retries.
                append_catalog_refresh_log(
                    "provider_catalog.refresh_not_run",
                    serde_json::json!({
                        "reason": "boundary_refused",
                        "waited_ms": waited_ms,
                        "ownership_released": true,
                    }),
                );
            }
            crate::blocking_pool::BlockingAdmission::Unknown { waited_ms } => {
                // The caller cannot distinguish a panic from a section still running past its
                // budget. Only the actual work releases its occupancy; a late result is recorded
                // by the settlement sink. Neither failure nor release is known here.
                append_catalog_refresh_log(
                    "provider_catalog.refresh_outcome_unknown",
                    serde_json::json!({
                        "reason": "section_outcome_unknown",
                        "waited_ms": waited_ms,
                        "ownership_released": null,
                    }),
                );
            }
            crate::blocking_pool::BlockingAdmission::Completed(_) => {
                // The real outcome was already recorded by the settlement sink, on the thread
                // that produced it. Reaching here is not a claim that the refresh succeeded.
            }
        }
    })
}

/// What one catalog refresh's synchronous half really did, error by error.
///
/// Every field is a separate real fact, because "the refresh ended" and "the refresh succeeded"
/// are different statements: `Completed` is the section returning, and a fetch that failed, a
/// cache that refused the write or a projection that could not be published all still have to be
/// reported as themselves rather than folded into one boolean.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct CatalogRefreshSettlement {
    /// The fetch's own outcome reached the cache: `Ok` when the cache accepted it.
    pub(crate) cache_ok: bool,
    /// The real error from the cache write / status publication of the fetch result.
    pub(crate) cache_error: Option<String>,
    /// The real error from reading the status the projection is built from.
    pub(crate) status_error: Option<String>,
    /// The real error from reading the model config snapshot.
    pub(crate) config_error: Option<String>,
    /// The real error from building the provider-catalog projection.
    pub(crate) projection_error: Option<String>,
    /// The real error from committing the projection.
    pub(crate) commit_error: Option<String>,
    /// The real error from publishing the committed projection.
    pub(crate) publish_error: Option<String>,
}

impl CatalogRefreshSettlement {
    #[must_use]
    pub(crate) fn failed(&self) -> bool {
        self.cache_error.is_some()
            || self.status_error.is_some()
            || self.config_error.is_some()
            || self.projection_error.is_some()
            || self.commit_error.is_some()
            || self.publish_error.is_some()
    }

    fn record(&self, event: &'static str) {
        append_catalog_refresh_log(
            event,
            serde_json::json!({
                "cache_ok": self.cache_ok,
                "cache_error": self.cache_error,
                "status_error": self.status_error,
                "config_error": self.config_error,
                "projection_error": self.projection_error,
                "commit_error": self.commit_error,
                "publish_error": self.publish_error,
                "failed": self.failed(),
                "ownership_released": true,
                "method": "provider_catalog.refresh",
            }),
        );
    }
}

/// The whole synchronous half of a refresh: cache, status, projection, commit, publication.
///
/// It is one function so that the section that runs it is one section, and so that every error it
/// can hit is a named field of its result instead of a discarded `let _ =`.
fn commit_and_publish_catalog_refresh<S, C>(
    catalog: &ProviderCatalogService<S, C>,
    model_config: &ModelConfigService,
    persistence: &aura_persistence::PersistenceCoordinator,
    provider_icons: &ProviderIconService,
    fetched: Result<aura_model_catalog::CatalogFetchResponse, CatalogError>,
) -> CatalogRefreshSettlement
where
    C: CatalogCache,
{
    let mut settlement = CatalogRefreshSettlement::default();
    // The fetch result decides what is persisted, not whether the projection is committed: a
    // failed refresh publishes its failure through the real status below, exactly as it did
    // before this boundary existed.
    match catalog.commit_refresh(fetched) {
        Ok(_) => settlement.cache_ok = true,
        Err(error) => settlement.cache_error = Some(error.to_string()),
    }
    let status = match catalog.status() {
        Ok(status) => status,
        Err(error) => {
            settlement.status_error = Some(error.to_string());
            return settlement;
        }
    };
    let snapshot = match model_config.snapshot() {
        Ok(snapshot) => snapshot,
        Err(error) => {
            settlement.config_error = Some(error.to_string());
            return settlement;
        }
    };
    let projection = match project_catalog(&status, &snapshot, persistence) {
        Ok(projection) => projection,
        Err(error) => {
            settlement.projection_error = Some(error.to_string());
            return settlement;
        }
    };
    let changes = match persistence.commit_model_projection_snapshots(vec![
        aura_persistence::ModelProjectionDraft::ProviderCatalog(projection),
    ]) {
        Ok(changes) => changes,
        Err(error) => {
            settlement.commit_error = Some(error.to_string());
            return settlement;
        }
    };
    if let Err(error) = persistence.publish_model_projection_changes(&changes) {
        settlement.publish_error = Some(error.to_string());
    }
    provider_icons.schedule_missing();
    settlement
}

fn append_catalog_refresh_log(event: &'static str, payload: serde_json::Value) {
    crate::append_core_log(event, payload);
}

/// Everything the scheduled refresh path needs, so the path itself can be driven by a
/// regression with a controlled catalog source instead of the process's real network gateway.
///
/// This is not a second implementation of the path: [`refresh_catalog_if_due_for`] *is* the
/// scheduled path, and `P05Services::refresh_catalog_if_due` is the one product caller that
/// fills this in from its own services. Splitting it out is what makes the ownership and
/// settlement rules of the path testable against the real boundary and the real persistence
/// without a real `models.dev` request in the loop.
pub(crate) struct CatalogRefreshPath<S, C> {
    pub(crate) sync: crate::sync_boundary::SyncBoundary,
    pub(crate) catalog: Arc<ProviderCatalogService<S, C>>,
    pub(crate) model_config: Arc<ModelConfigService>,
    pub(crate) persistence: Arc<aura_persistence::PersistenceCoordinator>,
    pub(crate) provider_icons: ProviderIconService,
}

/// Starts a scheduled catalog refresh when one is due, and says whether one really started.
///
/// WHY THIS IS ASYNCHRONOUS. The whole announcement of a refresh - reading the status, the
/// model config snapshot, building the loading projection and committing it - is synchronous
/// storage work through the one persistence writer, and this entry is called from the P05
/// maintenance task, which is a Tokio task. It therefore runs as one bounded
/// [`crate::sync_boundary::SyncSection::Projection`] section instead of inside the task.
///
/// WHO OWNS THE REFRESH. `begin_refresh` returns the slot's one owner, and that handle travels
/// with the work: into the announcement section, out of it with the announcement's real result,
/// and then into [`spawn_catalog_refresh`], which releases it when the storage half really ends.
/// The caller's budget never releases it. That is what makes the four section outcomes mean what
/// they say:
///
/// * `Refused` - the section provably did not run. Its closure was dropped with the handle inside
///   it, so the slot is already back and this call reports `Ok(false)`. Nothing was announced.
/// * `Completed(Ok(()))` - the announcement really committed the loading projection. The handle
///   comes back out of the section inside the value, and is handed to the driver, which owns the
///   refresh from here. `Ok(true)` says this call took the occupancy and a refresh run really
///   started; it does not claim the run has succeeded.
/// * `Completed(Err(error))` - the announcement really ran and really failed. The handle comes
///   back with the failure and is released here, after the work that failed has ended. No fetch
///   is started and the error is reported rather than swallowed by a `Ok(false)`.
/// * `Unknown` - the section started and produced no value at all, which is a panic in the
///   announcement: it may have had a partial effect, its own unwinding already released the
///   occupancy, and this is reported as an error rather than as "no refresh was started".
///
/// `Ok(false)` therefore means no refresh was started by this call, for exactly two reasons: the
/// refresh was not due, or another owner already holds the slot. A boundary refusal is a third
/// and is recorded where it happens (the boundary's own `sync_boundary.section_not_run` line and
/// its `refused` counter). All of them leave `last_successful_refresh_at` untouched, so
/// `needs_refresh` stays true and the next scheduled check - the same 60 s budget that called
/// this - retries. A refusal is never reported as a successful refresh.
///
/// WHY `run_owned` AND NOT `run`. `run_owned` awaits the section's *real* result through a
/// receipt that holds no boundary permit, so a caller whose budget expires still gets the
/// announcement's actual outcome instead of walking away from it. Ownership then settles with
/// the work, and there is no window in which the announcement has committed a loading projection
/// while nothing behind it owns the refresh.
pub(crate) async fn refresh_catalog_if_due_for<S, C>(
    path: CatalogRefreshPath<S, C>,
) -> anyhow::Result<bool>
where
    S: CatalogSource + 'static,
    C: CatalogCache + 'static,
{
    let CatalogRefreshPath { sync, catalog, model_config, persistence, provider_icons } = path;
    if !catalog.needs_refresh(UtcTimestamp::now()) {
        return Ok(false);
    }
    let Some(ownership) = catalog
        .begin_refresh()
        .context("failed to enter scheduled provider-catalog refresh state")?
    else {
        // Another owner already holds the refresh slot; the current status stays authoritative.
        return Ok(false);
    };
    let announce_catalog = Arc::clone(&catalog);
    let announce_config = Arc::clone(&model_config);
    let announce_persistence = Arc::clone(&persistence);
    let announced = sync
        .run_owned(
            crate::sync_boundary::SyncSection::Projection,
            crate::sync_boundary::SyncSubject::new().method("provider_catalog.schedule"),
            move || -> (anyhow::Result<()>, aura_model_catalog::RefreshOwnership) {
                let announced = (|| -> anyhow::Result<()> {
                    let loading_catalog = project_catalog(
                        &announce_catalog
                            .status()
                            .context("failed to read scheduled provider-catalog loading state")?,
                        &announce_config
                            .snapshot()
                            .context("failed to read model config for scheduled catalog refresh")?,
                        &announce_persistence,
                    )
                    .context("failed to build scheduled provider-catalog loading projection")?;
                    let changes = announce_persistence
                        .commit_model_projection_snapshots(vec![
                            aura_persistence::ModelProjectionDraft::ProviderCatalog(
                                loading_catalog,
                            ),
                        ])
                        .context(
                            "failed to commit scheduled provider-catalog loading projection",
                        )?;
                    announce_persistence.publish_model_projection_changes(&changes).context(
                        "failed to publish scheduled provider-catalog loading projection",
                    )?;
                    Ok(())
                })();
                // The occupancy travels with the announcement's real result: it is released by
                // whoever really ends this refresh, never by this section returning.
                (announced, ownership)
            },
        )
        .await;
    match announced {
        crate::blocking_pool::BlockingAdmission::Completed((Ok(()), ownership)) => {
            // The refresh now belongs to the task this starts, and that task's own section
            // releases the refresh slot when it really ends.
            drop(spawn_catalog_refresh(
                sync.clone(),
                Arc::clone(&catalog),
                Arc::clone(&model_config),
                Arc::clone(&persistence),
                provider_icons.clone(),
                ownership,
            ));
            Ok(true)
        }
        crate::blocking_pool::BlockingAdmission::Completed((Err(error), _ownership)) => {
            // The announcement itself failed. The occupancy is released here, after the real work
            // that failed has ended, and the failure is reported rather than reported as "nothing
            // started".
            Err(error)
        }
        crate::blocking_pool::BlockingAdmission::Refused { waited_ms } => {
            // The section provably did not run, so its closure was dropped with the occupancy
            // inside it: the slot is already back and the next check retries.
            append_catalog_refresh_log(
                "provider_catalog.refresh_not_started",
                serde_json::json!({
                    "reason": "boundary_refused",
                    "waited_ms": waited_ms,
                    "ownership_released": true,
                }),
            );
            Ok(false)
        }
        crate::blocking_pool::BlockingAdmission::Unknown { waited_ms } => {
            // The announcement started and produced no value, which is a panic: it may have had a
            // partial effect, its own unwinding released the occupancy, and it must not be
            // reported as a started refresh.
            anyhow::bail!(
                "the scheduled provider-catalog announcement section panicked after {waited_ms} ms; \
                 it may have committed a loading projection and the result has no owner"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use aura_model_catalog::ProviderConfigurationPolicy;
    use aura_model_domain::{
        ProviderCatalogConfigurationClass, ProviderCatalogConnectionDefaultsView,
        ProviderCatalogEntry,
    };

    use super::{CatalogProjectionPolicy, catalog_configuration_class};

    fn catalog_entry(catalog_provider_id: &str, api_url: Option<&str>) -> ProviderCatalogEntry {
        ProviderCatalogEntry {
            catalog_provider_id: catalog_provider_id.to_owned(),
            display_name: catalog_provider_id.to_owned(),
            npm_package: None,
            api_url: api_url.map(str::to_owned),
            documentation_url: None,
            model_order: Vec::new(),
            models: BTreeMap::new(),
        }
    }

    /// models.dev's catalog measured 5,413,993 bytes. A limit below that refused every refresh
    /// as `provider.catalog_response_too_large`, so no model got its catalog limits.
    #[test]
    fn the_catalog_limit_admits_the_current_models_dev_catalog_with_room_to_grow() {
        let limits = super::catalog_byte_limits();
        assert!(limits.max_download_bytes >= 4 * 5_413_993, "{limits:?}");
        assert!(limits.max_decompressed_bytes >= 4 * 5_413_993, "{limits:?}");
        assert_eq!(limits.max_upload_bytes, 1, "the catalog request sends nothing");
    }

    #[test]
    fn trusted_preset_defaults_come_only_from_the_compiled_registry() {
        let entry = catalog_entry("openai", Some("https://untrusted.example.test/v9"));

        assert_eq!(
            catalog_configuration_class(&entry),
            ProviderCatalogConfigurationClass::TrustedPreset
        );
        let defaults = CatalogProjectionPolicy
            .connection_defaults_for(&entry)
            .expect("compiled preset variants");
        let ProviderCatalogConnectionDefaultsView::AuraPreset { variants } = defaults else {
            panic!("trusted Provider must project signed Aura presets");
        };
        assert!(variants.iter().any(|variant| {
            variant.recommended
                && variant.variant_id == "responses"
                && variant.base_url == "https://api.openai.com/v1"
        }));
        assert!(
            variants.iter().all(|variant| variant.base_url != "https://untrusted.example.test/v9")
        );
    }

    #[test]
    fn valid_models_dev_api_is_only_an_untrusted_https_base_url_suggestion() {
        let entry = catalog_entry("provider-a", Some("https://api.example.test/v1"));

        assert_eq!(
            catalog_configuration_class(&entry),
            ProviderCatalogConfigurationClass::ManualConfiguration
        );
        assert_eq!(
            CatalogProjectionPolicy.connection_defaults_for(&entry),
            Some(ProviderCatalogConnectionDefaultsView::ModelsDevApiSuggestion {
                base_url: "https://api.example.test/v1".to_owned(),
            })
        );
    }

    #[test]
    fn unsafe_or_non_base_urls_are_information_only() {
        for api_url in [
            "http://api.example.test/v1",
            "https://user:password@api.example.test/v1",
            "https://api.example.test/v1?tenant=a",
            "https://api.example.test/v1#fragment",
        ] {
            let entry = catalog_entry("provider-a", Some(api_url));
            assert_eq!(
                catalog_configuration_class(&entry),
                ProviderCatalogConfigurationClass::CatalogInformationOnly,
                "unexpected class for {api_url}"
            );
            assert_eq!(CatalogProjectionPolicy.connection_defaults_for(&entry), None);
        }
    }
}
