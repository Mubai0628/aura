use std::{
    future::Future,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use aura_domain_common::{ArtifactId, ContentHash, NormalizedIssueCode, Revision, UtcTimestamp};
use aura_model_domain::{ProviderCatalogSnapshot, ProviderCatalogState, ProviderCatalogStatus};
use aura_network_control::{
    CoreHttpClientGateway, GatewayRequest, ProviderTransportGatewayRequest,
    StandardNetworkRequestGrant,
};
use aura_permission_domain::CoreRequestGrant;
use aura_security_contract::NetworkCallerKind;
use time::Duration;

use crate::{CatalogError, parse_models_dev_catalog};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CatalogConditionalRequest {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogFetchResponse {
    pub status: u16,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub body: Vec<u8>,
}

pub trait CatalogSource: Send + Sync {
    fn fetch(
        &self,
        conditional: CatalogConditionalRequest,
    ) -> impl Future<Output = Result<CatalogFetchResponse, CatalogError>> + Send;
}

impl<T> CatalogSource for Arc<T>
where
    T: CatalogSource + ?Sized,
{
    async fn fetch(
        &self,
        conditional: CatalogConditionalRequest,
    ) -> Result<CatalogFetchResponse, CatalogError> {
        self.as_ref().fetch(conditional).await
    }
}

pub trait CatalogRequestAuthorizer: Send + Sync {
    fn authorize_catalog_request(
        &self,
        conditional: CatalogConditionalRequest,
    ) -> impl Future<
        Output = Result<
            (CoreRequestGrant<StandardNetworkRequestGrant>, GatewayRequest),
            CatalogError,
        >,
    > + Send;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EndpointDiscoveryFetchResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub provider_request_id: Option<String>,
    pub body: Vec<u8>,
}

pub trait EndpointDiscoverySource: Send + Sync {
    fn fetch_models(
        &self,
    ) -> impl Future<Output = Result<EndpointDiscoveryFetchResponse, CatalogError>> + Send;
}

impl<T> EndpointDiscoverySource for Arc<T>
where
    T: EndpointDiscoverySource + ?Sized,
{
    async fn fetch_models(&self) -> Result<EndpointDiscoveryFetchResponse, CatalogError> {
        self.as_ref().fetch_models().await
    }
}

pub trait EndpointDiscoveryRequestAuthorizer: Send + Sync {
    fn authorize_endpoint_discovery(
        &self,
    ) -> impl Future<Output = Result<ProviderTransportGatewayRequest, CatalogError>> + Send;
}

pub struct CoreGatewayEndpointDiscoverySource<A> {
    gateway: Arc<CoreHttpClientGateway>,
    authorizer: A,
}

impl<A> CoreGatewayEndpointDiscoverySource<A> {
    #[must_use]
    pub fn new(gateway: Arc<CoreHttpClientGateway>, authorizer: A) -> Self {
        Self { gateway, authorizer }
    }
}

impl<A> EndpointDiscoverySource for CoreGatewayEndpointDiscoverySource<A>
where
    A: EndpointDiscoveryRequestAuthorizer,
{
    async fn fetch_models(&self) -> Result<EndpointDiscoveryFetchResponse, CatalogError> {
        let request = self.authorizer.authorize_endpoint_discovery().await?;
        if request.grant.caller != NetworkCallerKind::ModelTransport {
            return Err(CatalogError::WrongCaller);
        }
        if request.immutable_request.method != "GET" || !request.immutable_request.body.is_empty() {
            return Err(CatalogError::InvalidDiscoveryRequest);
        }
        let response = self.gateway.execute_provider_transport(request).await?;
        Ok(EndpointDiscoveryFetchResponse {
            status: response.status,
            content_type: header(&response.headers, "content-type"),
            provider_request_id: provider_request_id(&response.headers),
            body: response.body,
        })
    }
}

pub struct CoreGatewayCatalogSource<A> {
    gateway: Arc<CoreHttpClientGateway>,
    authorizer: A,
}

impl<A> CoreGatewayCatalogSource<A> {
    #[must_use]
    pub fn new(gateway: Arc<CoreHttpClientGateway>, authorizer: A) -> Self {
        Self { gateway, authorizer }
    }
}

impl<A> CatalogSource for CoreGatewayCatalogSource<A>
where
    A: CatalogRequestAuthorizer,
{
    async fn fetch(
        &self,
        conditional: CatalogConditionalRequest,
    ) -> Result<CatalogFetchResponse, CatalogError> {
        let (grant, request) = self.authorizer.authorize_catalog_request(conditional).await?;
        if request.plan.caller != NetworkCallerKind::ModelsCatalog
            || grant.caller != NetworkCallerKind::ModelsCatalog
        {
            return Err(CatalogError::WrongCaller);
        }
        let response = self.gateway.execute(grant, request).await?;
        Ok(CatalogFetchResponse {
            status: response.status,
            etag: header(&response.headers, "etag"),
            last_modified: header(&response.headers, "last-modified"),
            body: response.body,
        })
    }
}

fn header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

fn provider_request_id(headers: &[(String, String)]) -> Option<String> {
    ["x-request-id", "request-id", "cf-ray"]
        .into_iter()
        .find_map(|name| header(headers, name))
        .filter(|value| !value.trim().is_empty())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogCacheRecord {
    pub snapshot: Option<ProviderCatalogSnapshot>,
    pub last_error_code: Option<NormalizedIssueCode>,
}

pub trait CatalogCache: Send + Sync {
    fn load(&self) -> Result<CatalogCacheRecord, CatalogError>;
    fn persist_success(
        &self,
        raw: &[u8],
        snapshot: ProviderCatalogSnapshot,
    ) -> Result<ProviderCatalogSnapshot, CatalogError>;
    fn persist_revalidated(&self, snapshot: ProviderCatalogSnapshot) -> Result<(), CatalogError>;
    fn persist_failure(&self, issue_code: &NormalizedIssueCode) -> Result<(), CatalogError>;
}

impl<T> CatalogCache for Arc<T>
where
    T: CatalogCache + ?Sized,
{
    fn load(&self) -> Result<CatalogCacheRecord, CatalogError> {
        self.as_ref().load()
    }

    fn persist_success(
        &self,
        raw: &[u8],
        snapshot: ProviderCatalogSnapshot,
    ) -> Result<ProviderCatalogSnapshot, CatalogError> {
        self.as_ref().persist_success(raw, snapshot)
    }

    fn persist_revalidated(&self, snapshot: ProviderCatalogSnapshot) -> Result<(), CatalogError> {
        self.as_ref().persist_revalidated(snapshot)
    }

    fn persist_failure(&self, issue_code: &NormalizedIssueCode) -> Result<(), CatalogError> {
        self.as_ref().persist_failure(issue_code)
    }
}

#[derive(Default)]
pub struct InMemoryCatalogCache {
    record: RwLock<Option<CatalogCacheRecord>>,
}

impl CatalogCache for InMemoryCatalogCache {
    fn load(&self) -> Result<CatalogCacheRecord, CatalogError> {
        Ok(self
            .record
            .read()
            .map_err(|_| CatalogError::LockPoisoned)?
            .clone()
            .unwrap_or(CatalogCacheRecord { snapshot: None, last_error_code: None }))
    }

    fn persist_success(
        &self,
        _raw: &[u8],
        mut snapshot: ProviderCatalogSnapshot,
    ) -> Result<ProviderCatalogSnapshot, CatalogError> {
        snapshot.raw_artifact_id = ArtifactId::new();
        *self.record.write().map_err(|_| CatalogError::LockPoisoned)? =
            Some(CatalogCacheRecord { snapshot: Some(snapshot.clone()), last_error_code: None });
        Ok(snapshot)
    }

    fn persist_revalidated(&self, snapshot: ProviderCatalogSnapshot) -> Result<(), CatalogError> {
        *self.record.write().map_err(|_| CatalogError::LockPoisoned)? =
            Some(CatalogCacheRecord { snapshot: Some(snapshot), last_error_code: None });
        Ok(())
    }

    fn persist_failure(&self, issue_code: &NormalizedIssueCode) -> Result<(), CatalogError> {
        let mut guard = self.record.write().map_err(|_| CatalogError::LockPoisoned)?;
        let snapshot = guard.as_ref().and_then(|record| record.snapshot.clone());
        *guard = Some(CatalogCacheRecord { snapshot, last_error_code: Some(issue_code.clone()) });
        Ok(())
    }
}

pub trait ProviderCatalogClient {
    fn refresh(&self)
    -> impl Future<Output = Result<ProviderCatalogSnapshot, CatalogError>> + Send;
    fn current_snapshot(&self) -> Option<ProviderCatalogSnapshot>;
}

pub struct ProviderCatalogService<S, C> {
    source: S,
    cache: C,
    status: RwLock<ProviderCatalogStatus>,
    /// The process-wide singleflight flag. An `Arc` because the flag is not released here:
    /// a refresh whose synchronous half runs on the caller's bounded owner is finished by
    /// whichever of the two really ends it, and [`RefreshOwnership`] is the handle that
    /// travels there.
    refresh_in_progress: Arc<AtomicBool>,
}

/// Owns a service's process-wide refresh flag until it is dropped.
///
/// WHY THIS IS ITS OWN TYPE. `begin_refresh` takes the flag, and this is the only thing that
/// clears it. A refresh is two phases - an asynchronous fetch and then a synchronous commit
/// that writes through the persistence writer - and the commit is not run by the future that
/// started it: it is run by the bounded owner that is allowed to block. Handing this handle to
/// the phase that really finishes the refresh is what makes "the refresh is over" a fact about
/// the work rather than about the caller. Dropping it on every path, including a panic, is what
/// keeps a refused or failed refresh from leaving the catalog `Refreshing…` for the life of the
/// process.
///
/// EXACTLY ONE OWNER. The handle is returned by [`ProviderCatalogService::begin_refresh`] and
/// nowhere else, and that call returns one only to the caller that won the compare-exchange.
/// That makes the invariant structural: while a handle lives, the flag is taken, so no later
/// `begin_refresh` can succeed, so there is no later owner whose occupancy this handle's drop
/// could clear. A superseded owner cannot exist, and the handle is not `Clone` on purpose.
#[must_use = "dropping the handle releases the refresh flag immediately"]
pub struct RefreshOwnership(Arc<AtomicBool>);

impl Drop for RefreshOwnership {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

const CATALOG_MAX_AGE: Duration = Duration::hours(24);

impl<S, C> ProviderCatalogService<S, C>
where
    C: CatalogCache,
{
    pub fn open(source: S, cache: C) -> Result<Self, CatalogError> {
        // A cache is an availability optimization, not a prerequisite for
        // starting Aura. Reject an unreadable snapshot, retain only the
        // normalized failure text in memory, and allow a later explicit
        // refresh to replace it with a verified catalog.
        let cached = match cache.load() {
            Ok(cached) => cached,
            Err(error) => {
                CatalogCacheRecord { snapshot: None, last_error_code: Some(issue_code(&error)) }
            }
        };
        let state =
            cached.snapshot.as_ref().map_or(ProviderCatalogState::Unavailable, |snapshot| {
                if is_stale(snapshot.last_successful_refresh_at, UtcTimestamp::now()) {
                    ProviderCatalogState::Stale
                } else {
                    ProviderCatalogState::Fresh
                }
            });
        let checked_at = cached.snapshot.as_ref().map(|snapshot| snapshot.checked_at);
        let last_successful_refresh_at =
            cached.snapshot.as_ref().map(|snapshot| snapshot.last_successful_refresh_at);
        Ok(Self {
            source,
            cache,
            status: RwLock::new(ProviderCatalogStatus {
                state,
                snapshot: cached.snapshot,
                checked_at,
                last_successful_refresh_at,
                last_error_code: cached.last_error_code,
            }),
            refresh_in_progress: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn status(&self) -> Result<ProviderCatalogStatus, CatalogError> {
        self.status.read().map_err(|_| CatalogError::LockPoisoned).map(|status| status.clone())
    }

    #[must_use]
    pub fn current_snapshot(&self) -> Option<ProviderCatalogSnapshot> {
        self.status.read().ok().and_then(|status| status.snapshot.clone())
    }

    /// Whether some caller currently owns this service's refresh slot.
    ///
    /// Read by the asynchronous owner of a two-phase refresh, which must not start a second
    /// fetch for a refresh it does not own, and by the regressions that have to show an
    /// occupancy is still held while its real work is unfinished.
    #[must_use]
    pub fn refresh_in_progress(&self) -> bool {
        self.refresh_in_progress.load(Ordering::Acquire)
    }

    /// Takes the refresh slot and returns its one owner.
    ///
    /// `Ok(None)` means another caller owns the refresh; the current status remains
    /// authoritative and nothing was started. `Ok(Some(ownership))` means this caller took the
    /// slot: the flag stays taken until that handle is dropped, and dropping it is the only
    /// thing that ever gives the slot back.
    ///
    /// WHY THE HANDLE IS RETURNED HERE. A handle that could be created without winning this
    /// compare-exchange could clear an occupancy it does not own - a second caller could take a
    /// handle while the first is still working and release the slot on its way out, letting a
    /// third refresh start on top of work that is still running. Making the only way to obtain
    /// a handle be the successful compare-exchange removes that possibility structurally: while
    /// a handle lives the flag is taken, so no later `begin_refresh` can succeed, so no later
    /// owner exists whose occupancy an older handle could clear.
    pub fn begin_refresh(&self) -> Result<Option<RefreshOwnership>, CatalogError> {
        if self
            .refresh_in_progress
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(None);
        }
        // From here on this handle owns the slot, including on the error exit below: dropping it
        // is what gives the slot back, and no other caller can have taken it in between.
        let ownership = RefreshOwnership(Arc::clone(&self.refresh_in_progress));
        let mut status = self.status.write().map_err(|_| CatalogError::LockPoisoned)?;
        status.state = if status.snapshot.is_some() {
            ProviderCatalogState::RefreshingWithLkg
        } else {
            ProviderCatalogState::LoadingWithoutCache
        };
        status.last_error_code = None;
        Ok(Some(ownership))
    }

    #[must_use]
    pub fn needs_refresh(&self, now: UtcTimestamp) -> bool {
        self.status.read().map_or(true, |status| {
            status
                .last_successful_refresh_at
                .is_none_or(|last_successful| is_stale(last_successful, now))
        })
    }

    /// The **asynchronous** half of a refresh: the network fetch, and nothing else.
    ///
    /// WHY THE REFRESH IS SPLIT. The cache behind this service is synchronous storage: every
    /// `persist_*` reaches the one persistence writer, which can be busy, and writing through
    /// it from inside a future runs that wait on whatever thread drives the future - a Tokio
    /// worker, on the Core's background refresh. Draining a reactor worker that way stops every
    /// other task on that runtime for as long as the write waits. So the fetch is this call and
    /// the storage is [`Self::commit_refresh`], and the caller runs the second one on its own
    /// bounded synchronous owner, never on the thread that awaited the first.
    ///
    /// This half writes no cache row, no artifact and no status; it reads the in-memory status
    /// only for the conditional-request headers.
    pub async fn fetch_refresh(&self) -> Result<CatalogFetchResponse, CatalogError>
    where
        S: CatalogSource,
    {
        self.source.fetch(self.conditional()?).await
    }

    /// The **synchronous** half of a refresh: persist a fetched response, or the failure of the
    /// fetch, and publish the status every reader of this service sees.
    ///
    /// It performs at most one cache write and never touches the network, which is exactly why
    /// the caller must run it on the bounded owner that is allowed to block. Both answers are
    /// storage work: a successful refresh persists the new snapshot, and a failed one persists
    /// the failure so the next `load` still reports it.
    pub fn commit_refresh(
        &self,
        fetched: Result<CatalogFetchResponse, CatalogError>,
    ) -> Result<ProviderCatalogSnapshot, CatalogError> {
        match fetched {
            Ok(response) => match self.apply_response(response) {
                Ok(snapshot) => {
                    self.publish_success(snapshot.clone())?;
                    Ok(snapshot)
                }
                Err(error) => {
                    self.publish_failure(&error)?;
                    Err(error)
                }
            },
            Err(error) => {
                self.publish_failure(&error)?;
                Err(error)
            }
        }
    }

    /// One whole refresh on the calling thread: the asynchronous half, then the synchronous one.
    ///
    /// This is the pre-boundary composition and it is kept for a caller that already is a
    /// blocking thread (a fixture, a test); a Core that owns a [`SyncBoundary`] uses the two
    /// halves instead, so its storage work never runs on a reactor worker. It takes the
    /// ownership handle from [`Self::begin_refresh`] and holds it for the whole composition, so
    /// a composed refresh releases the slot when it, and only it, really ends.
    ///
    /// [`SyncBoundary`]: ../../aura_core/sync_boundary/struct.SyncBoundary.html
    pub async fn refresh_after_begin(
        &self,
        ownership: RefreshOwnership,
    ) -> Result<ProviderCatalogSnapshot, CatalogError>
    where
        S: CatalogSource,
    {
        let _ownership = ownership;
        self.commit_refresh(self.fetch_refresh().await)
    }

    fn conditional(&self) -> Result<CatalogConditionalRequest, CatalogError> {
        let status = self.status.read().map_err(|_| CatalogError::LockPoisoned)?;
        Ok(CatalogConditionalRequest {
            etag: status.snapshot.as_ref().and_then(|snapshot| snapshot.etag.clone()),
            last_modified: status
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.last_modified.clone()),
        })
    }

    fn publish_success(&self, snapshot: ProviderCatalogSnapshot) -> Result<(), CatalogError> {
        *self.status.write().map_err(|_| CatalogError::LockPoisoned)? = ProviderCatalogStatus {
            state: ProviderCatalogState::Fresh,
            checked_at: Some(snapshot.checked_at),
            last_successful_refresh_at: Some(snapshot.last_successful_refresh_at),
            snapshot: Some(snapshot),
            last_error_code: None,
        };
        Ok(())
    }

    fn publish_failure(&self, error: &CatalogError) -> Result<(), CatalogError> {
        let issue_code = issue_code(error);
        {
            let mut status = self.status.write().map_err(|_| CatalogError::LockPoisoned)?;
            status.state = if status.snapshot.is_some() {
                ProviderCatalogState::RefreshFailedWithLkg
            } else {
                ProviderCatalogState::Unavailable
            };
            status.checked_at = Some(UtcTimestamp::now());
            status.last_error_code = Some(issue_code.clone());
        }
        self.cache.persist_failure(&issue_code)
    }

    fn apply_response(
        &self,
        response: CatalogFetchResponse,
    ) -> Result<ProviderCatalogSnapshot, CatalogError> {
        let now = UtcTimestamp::now();
        match response.status {
            200 => {
                let content_hash = ContentHash::blake3(&response.body);
                if let Some(mut snapshot) = self.current_snapshot()
                    && snapshot.catalog_content_hash == content_hash
                {
                    snapshot.checked_at = now;
                    snapshot.last_successful_refresh_at = now;
                    snapshot.etag = response.etag;
                    snapshot.last_modified = response.last_modified;
                    self.cache.persist_revalidated(snapshot.clone())?;
                    return Ok(snapshot);
                }
                let providers = parse_models_dev_catalog(&response.body)?;
                let provider_order = providers.keys().cloned().collect();
                let catalog_revision = self
                    .current_snapshot()
                    .map(|snapshot| {
                        snapshot
                            .catalog_revision
                            .value()
                            .checked_add(1)
                            .map(Revision::new)
                            .ok_or(CatalogError::RevisionOverflow)
                    })
                    .transpose()?
                    .unwrap_or(Revision::INITIAL);
                let snapshot = ProviderCatalogSnapshot {
                    catalog_revision,
                    catalog_content_hash: content_hash,
                    checked_at: now,
                    last_successful_refresh_at: now,
                    etag: response.etag,
                    last_modified: response.last_modified,
                    raw_artifact_id: ArtifactId::new(),
                    provider_order,
                    providers,
                };
                self.cache.persist_success(&response.body, snapshot)
            }
            304 => {
                let mut snapshot =
                    self.current_snapshot().ok_or(CatalogError::NotModifiedWithoutCache)?;
                snapshot.checked_at = now;
                snapshot.last_successful_refresh_at = now;
                if response.etag.is_some() {
                    snapshot.etag = response.etag;
                }
                if response.last_modified.is_some() {
                    snapshot.last_modified = response.last_modified;
                }
                self.cache.persist_revalidated(snapshot.clone())?;
                Ok(snapshot)
            }
            status => Err(CatalogError::HttpStatus(status)),
        }
    }
}

impl<S, C> ProviderCatalogClient for ProviderCatalogService<S, C>
where
    S: CatalogSource,
    C: CatalogCache,
{
    async fn refresh(&self) -> Result<ProviderCatalogSnapshot, CatalogError> {
        let Some(ownership) = self.begin_refresh()? else {
            return self.current_snapshot().ok_or(CatalogError::RefreshInProgress);
        };
        self.refresh_after_begin(ownership).await
    }

    fn current_snapshot(&self) -> Option<ProviderCatalogSnapshot> {
        ProviderCatalogService::current_snapshot(self)
    }
}

fn is_stale(last_successful_refresh_at: UtcTimestamp, now: UtcTimestamp) -> bool {
    now.value() - last_successful_refresh_at.value() >= CATALOG_MAX_AGE
}

fn issue_code(error: &CatalogError) -> NormalizedIssueCode {
    let value = match error {
        CatalogError::Cache(_) | CatalogError::LockPoisoned => "persistence.catalog_cache_failed",
        CatalogError::Decode(_) | CatalogError::Invalid(_) => "provider.catalog_invalid",
        CatalogError::Network(aura_network_control::NetworkControlError::ResponseTooLarge) => {
            "provider.catalog_response_too_large"
        }
        _ => "provider.catalog_refresh_failed",
    };
    NormalizedIssueCode::new(value).expect("catalog issue codes are registered constants")
}
