//! Aura's own changes to the model settings, and how a write prepared on an earlier revision is
//! applied over them.
//!
//! Aura changes the model settings by itself in two places: a discovery run merges the models it
//! found, and an availability check learns which output-token field a connection accepts. A write
//! prepared on the revision before such a change is applied on the latest settings instead of
//! being refused as stale, when every revision after the one it was prepared on is one of these
//! changes, the settings in effect are exactly what those changes produced, and none of them
//! touched a provider, connection or model the write touches. The changes are kept in memory only:
//! after a restart, or once one is no longer among the last few, a stale write is refused as
//! before.
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};

use aura_config::{ConfigModule, SettingsFacadeError};
use aura_domain_common::{EndpointId, ModelProfileId, ProviderId};
use aura_model_domain::ModelRegistryConfig;

use crate::ModelConfigError;
use crate::service::{ModelConfigService, ModelConfigSnapshot, PreparedModelConfigMutation};

/// How many of Aura's own changes are kept.
const KEPT: usize = 32;

/// The objects one change to the model settings added, removed or changed.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct TouchedObjects {
    provider_order: bool,
    providers: BTreeSet<ProviderId>,
    endpoints: BTreeSet<EndpointId>,
    models: BTreeSet<ModelProfileId>,
}

impl TouchedObjects {
    fn between(before: &ModelRegistryConfig, after: &ModelRegistryConfig) -> Self {
        Self {
            provider_order: before.provider_order != after.provider_order,
            providers: changed_keys(&before.providers, &after.providers),
            endpoints: changed_keys(&before.endpoints, &after.endpoints),
            models: changed_keys(&before.models, &after.models),
        }
    }

    fn overlaps(&self, other: &Self) -> bool {
        self.provider_order && other.provider_order
            || !self.providers.is_disjoint(&other.providers)
            || !self.endpoints.is_disjoint(&other.endpoints)
            || !self.models.is_disjoint(&other.models)
    }

    fn extend(&mut self, other: &Self) {
        self.provider_order |= other.provider_order;
        self.providers.extend(&other.providers);
        self.endpoints.extend(&other.endpoints);
        self.models.extend(&other.models);
    }
}

fn changed_keys<K: Copy + Ord, V: PartialEq>(
    before: &BTreeMap<K, V>,
    after: &BTreeMap<K, V>,
) -> BTreeSet<K> {
    before
        .keys()
        .chain(after.keys())
        .filter(|key| before.get(key) != after.get(key))
        .copied()
        .collect()
}

/// One change of Aura's own: the settings it was prepared on and the settings it produces.
struct BackgroundChange {
    previous_revision: u64,
    previous: Arc<ModelRegistryConfig>,
    revision: u64,
    registry: Arc<ModelRegistryConfig>,
    touched: TouchedObjects,
}

/// Aura's own changes to the model settings, newest last. A change is kept from the moment it is
/// prepared; it counts only while the settings in effect show it took effect.
#[derive(Default)]
pub(crate) struct BackgroundChanges(Mutex<VecDeque<BackgroundChange>>);

/// How a write prepared on `expected` relates to the settings in effect.
pub(crate) enum Rebase {
    /// The write was prepared on the settings in effect.
    Current(u64),
    /// Only Aura's own changes came after the write's revision; these are the objects they
    /// touched.
    OverBackground { expected: u64, touched: TouchedObjects },
}

impl BackgroundChanges {
    fn record(&self, change: BackgroundChange) {
        let Ok(mut changes) = self.0.lock() else { return };
        changes.push_back(change);
        while changes.len() > KEPT {
            changes.pop_front();
        }
    }

    /// Whether a write prepared on `expected` may be prepared on `current`: refused as stale
    /// unless every revision in between is one of Aura's own changes, and `current`, followed back
    /// change by change, is exactly what they produced.
    pub(crate) fn before(
        &self,
        expected: u64,
        current: &ModelConfigSnapshot,
    ) -> Result<Rebase, ModelConfigError> {
        if current.revision == expected {
            return Ok(Rebase::Current(expected));
        }
        let changes = self.0.lock().map_err(|_| stale(expected, current.revision))?;
        let mut touched = TouchedObjects::default();
        let (mut revision, mut registry) = (current.revision, Arc::clone(&current.registry));
        while revision > expected {
            let change = changes
                .iter()
                .rev()
                .find(|change| change.revision == revision && *change.registry == *registry)
                .ok_or_else(|| stale(expected, current.revision))?;
            touched.extend(&change.touched);
            (revision, registry) = (change.previous_revision, Arc::clone(&change.previous));
        }
        if revision != expected {
            return Err(stale(expected, current.revision));
        }
        Ok(Rebase::OverBackground { expected, touched })
    }
}

impl Rebase {
    /// The revision the write is prepared on: `current`, unless the write touches an object one of
    /// Aura's own changes touched after the write's revision.
    pub(crate) fn onto(
        self,
        current: &ModelConfigSnapshot,
        candidate: &ModelRegistryConfig,
    ) -> Result<u64, ModelConfigError> {
        match self {
            Self::Current(revision) => Ok(revision),
            Self::OverBackground { expected, touched } => {
                if TouchedObjects::between(&current.registry, candidate).overlaps(&touched) {
                    return Err(stale(expected, current.revision));
                }
                Ok(current.revision)
            }
        }
    }
}

/// The refusal a write prepared on an earlier revision has always had.
fn stale(expected: u64, actual: u64) -> ModelConfigError {
    ModelConfigError::Settings(SettingsFacadeError::RevisionConflict {
        module: ConfigModule::Models,
        expected,
        actual,
    })
}

impl ModelConfigService {
    /// [`Self::prepare_update`] for a change Aura makes by itself: once it takes effect, a write
    /// prepared on the revision before it can still be applied over it.
    pub fn prepare_background_update<T, F>(
        &self,
        transaction_id: String,
        expected_revision: u64,
        mutate: F,
    ) -> Result<PreparedModelConfigMutation<T>, ModelConfigError>
    where
        F: FnOnce(&mut ModelRegistryConfig) -> Result<T, ModelConfigError>,
    {
        let prepared = self.prepare_update(transaction_id, expected_revision, mutate)?;
        if prepared.changed {
            self.background.record(BackgroundChange {
                previous_revision: prepared.previous.revision,
                previous: Arc::clone(&prepared.previous.registry),
                revision: prepared.candidate_revision.value(),
                registry: Arc::clone(&prepared.candidate_registry),
                touched: TouchedObjects::between(
                    &prepared.previous.registry,
                    &prepared.candidate_registry,
                ),
            });
        }
        Ok(prepared)
    }
}
