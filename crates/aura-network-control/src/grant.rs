use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use aura_domain_common::{NetworkGrantId, UtcTimestamp};
use aura_permission_domain::{NetworkGrant, NetworkGrantIdentityScope, NormalizedOrigin};
use aura_persistence::PersistenceCoordinator;
use aura_security_contract::{
    InvocationIdentity, NetworkCallerKind, NetworkDataClass, NetworkDestinationClass,
    PermissionGrantLifetime, ResolvedNetworkDestination,
};

use crate::NetworkControlError;

pub trait NetworkGrantStore: Send + Sync {
    fn get(&self, id: NetworkGrantId) -> Option<NetworkGrant>;
}

#[derive(Clone, Default)]
pub struct InMemoryNetworkGrantStore {
    grants: Arc<Mutex<BTreeMap<NetworkGrantId, NetworkGrant>>>,
}

impl InMemoryNetworkGrantStore {
    pub fn insert(&self, grant: NetworkGrant) -> Result<(), NetworkControlError> {
        self.grants
            .lock()
            .map_err(|_| NetworkControlError::Transport("network grant store poisoned".to_owned()))?
            .insert(grant.id, grant);
        Ok(())
    }

    pub fn revoke(&self, id: NetworkGrantId) -> Result<bool, NetworkControlError> {
        let mut grants = self.grants.lock().map_err(|_| {
            NetworkControlError::Transport("network grant store poisoned".to_owned())
        })?;
        let Some(grant) = grants.get_mut(&id) else {
            return Ok(false);
        };
        grant.revoked_at = Some(UtcTimestamp::now());
        Ok(true)
    }
}

impl NetworkGrantStore for InMemoryNetworkGrantStore {
    fn get(&self, id: NetworkGrantId) -> Option<NetworkGrant> {
        self.grants.lock().ok()?.get(&id).cloned()
    }
}

#[derive(Clone)]
pub struct PersistenceNetworkGrantStore {
    persistence: PersistenceCoordinator,
}

impl PersistenceNetworkGrantStore {
    #[must_use]
    pub const fn new(persistence: PersistenceCoordinator) -> Self {
        Self { persistence }
    }
}

impl NetworkGrantStore for PersistenceNetworkGrantStore {
    fn get(&self, id: NetworkGrantId) -> Option<NetworkGrant> {
        self.persistence.network_grant(id).ok().flatten()
    }
}

pub(crate) struct GrantMatchQuery<'a> {
    pub identity: &'a InvocationIdentity,
    pub caller: NetworkCallerKind,
    pub origin: &'a NormalizedOrigin,
    pub destination: &'a ResolvedNetworkDestination,
    pub method: &'a str,
    pub data_classes: &'a [NetworkDataClass],
    pub upload_bytes: u64,
    pub download_limit: u64,
}

pub(crate) fn matching_grants(
    grants: &[NetworkGrant],
    query: GrantMatchQuery<'_>,
) -> Result<Vec<NetworkGrant>, NetworkControlError> {
    let now = UtcTimestamp::now();
    let mut matched = Vec::new();
    let mut saw_expired = false;
    for grant in grants {
        if grant.revoked_at.is_some() {
            continue;
        }
        if grant.expires_at.is_some_and(|expires| expires.value() <= now.value()) {
            saw_expired = true;
            continue;
        }
        if !identity_matches(grant, query.identity, query.caller)
            || !grant.origins.contains(query.origin)
            || !query
                .destination
                .classes
                .iter()
                .all(|class| grant.destination_classes.contains(class))
            || !grant.methods.iter().any(|allowed| allowed.eq_ignore_ascii_case(query.method))
            || !query.data_classes.iter().all(|class| grant.allowed_data_classes.contains(class))
            || grant.max_upload_bytes.is_some_and(|max| query.upload_bytes > max)
            || grant.max_download_bytes.is_some_and(|max| query.download_limit > max)
        {
            continue;
        }
        if query.destination.classes.contains(&NetworkDestinationClass::Metadata)
            && grant.lifetime != PermissionGrantLifetime::Once
        {
            continue;
        }
        matched.push(grant.clone());
    }
    if matched.is_empty() && saw_expired {
        Err(NetworkControlError::GrantExpired)
    } else {
        Ok(matched)
    }
}

fn identity_matches(
    grant: &NetworkGrant,
    identity: &InvocationIdentity,
    caller: NetworkCallerKind,
) -> bool {
    match grant.identity_scope {
        NetworkGrantIdentityScope::Session => {
            identity.session_id.is_some_and(|id| id.to_string() == grant.identity_scope_id)
        }
        NetworkGrantIdentityScope::TurnTask => {
            identity.turn_task_id.is_some_and(|id| id.to_string() == grant.identity_scope_id)
        }
        NetworkGrantIdentityScope::SandboxExecution => caller == NetworkCallerKind::SandboxProcess,
        NetworkGrantIdentityScope::SystemComponent => {
            identity.principal_id.to_string() == grant.identity_scope_id
        }
    }
}

#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    pub global_concurrent_connections: u32,
    pub per_origin_concurrent_connections: u32,
    pub per_origin_requests_per_window: u32,
    pub window: Duration,
    pub per_origin_upload_bytes_per_window: u64,
    pub per_origin_download_bytes_per_window: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            global_concurrent_connections: 64,
            per_origin_concurrent_connections: 8,
            per_origin_requests_per_window: 120,
            window: Duration::from_secs(60),
            per_origin_upload_bytes_per_window: 64 * 1024 * 1024,
            per_origin_download_bytes_per_window: 256 * 1024 * 1024,
        }
    }
}

#[derive(Clone)]
pub struct RateLimiter {
    config: RateLimitConfig,
    state: Arc<Mutex<RateLimitState>>,
}

#[derive(Default)]
struct RateLimitState {
    global_active: u32,
    origins: BTreeMap<NormalizedOrigin, OriginRateState>,
}

struct OriginRateState {
    active: u32,
    window_started: Instant,
    requests: u32,
    upload_bytes: u64,
    download_bytes: u64,
}

impl RateLimiter {
    #[must_use]
    pub fn new(config: RateLimitConfig) -> Self {
        Self { config, state: Arc::new(Mutex::new(RateLimitState::default())) }
    }

    pub fn acquire(
        &self,
        origin: &NormalizedOrigin,
        upload_bytes: u64,
    ) -> Result<RateLimitPermit, NetworkControlError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| NetworkControlError::Transport("rate limiter poisoned".to_owned()))?;
        if state.global_active >= self.config.global_concurrent_connections {
            return Err(NetworkControlError::RateLimited);
        }
        let now = Instant::now();
        let origin_state = state.origins.entry(origin.clone()).or_insert(OriginRateState {
            active: 0,
            window_started: now,
            requests: 0,
            upload_bytes: 0,
            download_bytes: 0,
        });
        if now.duration_since(origin_state.window_started) >= self.config.window {
            origin_state.window_started = now;
            origin_state.requests = 0;
            origin_state.upload_bytes = 0;
            origin_state.download_bytes = 0;
        }
        if origin_state.active >= self.config.per_origin_concurrent_connections
            || origin_state.requests >= self.config.per_origin_requests_per_window
            || origin_state.upload_bytes.saturating_add(upload_bytes)
                > self.config.per_origin_upload_bytes_per_window
        {
            return Err(NetworkControlError::RateLimited);
        }
        origin_state.active += 1;
        origin_state.requests += 1;
        origin_state.upload_bytes = origin_state.upload_bytes.saturating_add(upload_bytes);
        state.global_active += 1;
        drop(state);
        Ok(RateLimitPermit { limiter: self.clone(), origin: origin.clone(), released: false })
    }

    fn charge_download(
        &self,
        origin: &NormalizedOrigin,
        bytes: u64,
    ) -> Result<(), NetworkControlError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| NetworkControlError::Transport("rate limiter poisoned".to_owned()))?;
        let origin_state = state.origins.get_mut(origin).ok_or(NetworkControlError::RateLimited)?;
        if origin_state.download_bytes.saturating_add(bytes)
            > self.config.per_origin_download_bytes_per_window
        {
            return Err(NetworkControlError::RateLimited);
        }
        origin_state.download_bytes = origin_state.download_bytes.saturating_add(bytes);
        Ok(())
    }

    fn release(&self, origin: &NormalizedOrigin) {
        if let Ok(mut state) = self.state.lock() {
            state.global_active = state.global_active.saturating_sub(1);
            if let Some(origin_state) = state.origins.get_mut(origin) {
                origin_state.active = origin_state.active.saturating_sub(1);
            }
        }
    }
}

pub struct RateLimitPermit {
    limiter: RateLimiter,
    origin: NormalizedOrigin,
    released: bool,
}

impl RateLimitPermit {
    pub fn charge_download(&self, bytes: u64) -> Result<(), NetworkControlError> {
        self.limiter.charge_download(&self.origin, bytes)
    }
}

impl Drop for RateLimitPermit {
    fn drop(&mut self) {
        if !self.released {
            self.limiter.release(&self.origin);
            self.released = true;
        }
    }
}
