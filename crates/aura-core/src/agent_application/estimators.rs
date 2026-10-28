//! The estimators a request and its compaction are counted with.
use aura_context_runtime::ContextEstimatorRegistry;
use aura_domain_common::ModelProfileId;

/// Models whose requests no provider has counted go byte for byte, as fixtures sized in bytes
/// expect; every other model is counted by character class.
#[cfg(test)]
pub(crate) static UNMEASURED_BY_BYTES: std::sync::Mutex<
    std::collections::BTreeSet<ModelProfileId>,
> = std::sync::Mutex::new(std::collections::BTreeSet::new());

pub(super) fn for_model(model: ModelProfileId) -> ContextEstimatorRegistry {
    let estimators = ContextEstimatorRegistry::default();
    if counted_by_bytes(model) { estimators.counting_unmeasured_by_bytes() } else { estimators }
}

#[cfg(test)]
fn counted_by_bytes(model: ModelProfileId) -> bool {
    UNMEASURED_BY_BYTES.lock().is_ok_and(|models| models.contains(&model))
}

#[cfg(not(test))]
const fn counted_by_bytes(_: ModelProfileId) -> bool {
    false
}
