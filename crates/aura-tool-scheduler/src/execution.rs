use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use aura_domain_common::ToolInstanceId;
use aura_domain_common::jcs::CanonicalHashV1;
use aura_tool_contract::{ResolvedResourceAccessV1, ToolExecutionPlanCallV1, ToolVersion};
use thiserror::Error;
use tokio::sync::{
    OwnedRwLockReadGuard, OwnedRwLockWriteGuard, OwnedSemaphorePermit, RwLock, Semaphore, watch,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolInstanceExecutionGenerationKeyV1 {
    pub tool_instance_id: ToolInstanceId,
    pub tool_version: ToolVersion,
    pub execution_fingerprint: CanonicalHashV1,
    pub local_concurrency_limit: u32,
}

#[derive(Clone, Debug)]
pub struct ToolScheduleRequestV1<'a> {
    pub plan_call: &'a ToolExecutionPlanCallV1,
    pub generation: ToolInstanceExecutionGenerationKeyV1,
    /// Core-derived MCP/Plugin source serialization key. It is absent for
    /// native Package/Builtin tools and never acts as a permission Grant.
    pub source_lock_key: Option<&'a str>,
}

#[derive(Clone)]
pub struct ToolExecutionSchedulerV1 {
    global: crate::capacity::ToolCapacity,
    local_generations: Arc<Mutex<BTreeMap<ToolInstanceExecutionGenerationMapKey, Weak<Semaphore>>>>,
    resource_locks: Arc<Mutex<BTreeMap<String, Weak<RwLock<()>>>>>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ToolInstanceExecutionGenerationMapKey {
    tool_instance_id: String,
    semantic_version: Option<String>,
    definition_revision: u64,
    execution_fingerprint: String,
    local_concurrency_limit: u32,
}

impl From<&ToolInstanceExecutionGenerationKeyV1> for ToolInstanceExecutionGenerationMapKey {
    fn from(value: &ToolInstanceExecutionGenerationKeyV1) -> Self {
        Self {
            tool_instance_id: value.tool_instance_id.to_string(),
            semantic_version: value.tool_version.semantic_version.as_ref().map(ToString::to_string),
            definition_revision: value.tool_version.definition_revision.value(),
            execution_fingerprint: value.execution_fingerprint.hex().to_owned(),
            local_concurrency_limit: value.local_concurrency_limit,
        }
    }
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ToolScheduleErrorV1 {
    #[error("the frozen execution plan and instance generation are inconsistent")]
    InvalidFrozenPlan,
    #[error("tool scheduling was cancelled before Runtime dispatch")]
    CancelledBeforeDispatch,
    #[error("the scheduler synchronization state is unavailable")]
    StateUnavailable,
}

/// RAII guard held from the final pre-dispatch checkpoint until the Runtime
/// and its process tree have reached a proven terminal state. Dropping a
/// Future is never treated as process termination evidence.
#[allow(dead_code)]
enum HeldResourceLock {
    Shared(OwnedRwLockReadGuard<()>),
    Exclusive(OwnedRwLockWriteGuard<()>),
}

pub struct ToolDispatchPermitV1 {
    _resource_permits: Vec<HeldResourceLock>,
    capacity: crate::pre_effect_wait::DispatchRuntimeCapacity,
    timeout: Duration,
}

/// Resource/source and execution-generation readiness acquired before the
/// concrete Runtime host capacity and global Runtime slot. This split lets the
/// Core wait for a one-process Sandbox Host without counting that wait as an
/// active ordinary Tool attempt.
pub struct ToolDispatchReadinessPermitV1 {
    _resource_permits: Vec<HeldResourceLock>,
    _local_permit: OwnedSemaphorePermit,
    global: crate::capacity::ToolCapacity,
    timeout: Duration,
}

impl ToolDispatchReadinessPermitV1 {
    /// Activates the global ordinary-Tool Runtime slot only after the caller
    /// has acquired the selected Host's real capacity permit. This is the only
    /// point that consumes executing capacity, so resource locks, approval and
    /// backend preparation wait without occupying a running slot.
    pub async fn activate_runtime_slot(
        self,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Result<ToolDispatchPermitV1, ToolScheduleErrorV1> {
        let global_slot = self
            .global
            .acquire(cancellation)
            .await
            .ok_or(ToolScheduleErrorV1::CancelledBeforeDispatch)?;
        Ok(ToolDispatchPermitV1 {
            _resource_permits: self._resource_permits,
            capacity: crate::pre_effect_wait::DispatchRuntimeCapacity::new(
                self._local_permit,
                global_slot,
                self.global,
            ),
            timeout: self.timeout,
        })
    }
}

impl ToolDispatchPermitV1 {
    #[must_use]
    pub fn runtime_capacity(&self) -> crate::pre_effect_wait::DispatchRuntimeCapacity {
        self.capacity.clone()
    }
    #[must_use]
    pub const fn timeout(&self) -> Duration {
        self.timeout
    }
}

impl ToolExecutionSchedulerV1 {
    pub fn new(global_max_concurrent_tool_calls: u32) -> Result<Self, ToolScheduleErrorV1> {
        validate_global_limit(global_max_concurrent_tool_calls)?;
        Ok(Self {
            global: crate::capacity::ToolCapacity::new(global_max_concurrent_tool_calls),
            local_generations: Arc::new(Mutex::new(BTreeMap::new())),
            resource_locks: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    /// Applies a committed execution-strategy value to the one global capacity.
    ///
    /// This scheduler stays the sole owner: callers never create a second
    /// limiter around it, so a settings change cannot leave an old and a new
    /// capacity running side by side. Members already holding a slot keep it.
    pub fn set_global_limit(
        &self,
        global_max_concurrent_tool_calls: u32,
    ) -> Result<(), ToolScheduleErrorV1> {
        validate_global_limit(global_max_concurrent_tool_calls)?;
        self.global.set_limit(global_max_concurrent_tool_calls);
        Ok(())
    }

    /// The committed global limit and how many calls currently hold a slot.
    #[must_use]
    pub fn global_capacity(&self) -> (u32, u32) {
        (self.global.limit(), self.global.in_flight())
    }

    /// Acquires resource/source locks and the generation-local semaphore before
    /// the global Runtime slot. Queue, permission, approval, and resource wait
    /// therefore never consume the global four-way ordinary Tool budget.
    pub async fn acquire_dispatch_permit(
        &self,
        request: ToolScheduleRequestV1<'_>,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Result<ToolDispatchPermitV1, ToolScheduleErrorV1> {
        self.acquire_dispatch_readiness(request, cancellation)
            .await?
            .activate_runtime_slot(cancellation)
            .await
    }

    /// Acquires every deterministic scheduling constraint except the global
    /// active-Runtime slot. The Core uses this before acquiring capacity from
    /// the explicitly selected Runtime Host.
    pub async fn acquire_dispatch_readiness(
        &self,
        request: ToolScheduleRequestV1<'_>,
        cancellation: &mut watch::Receiver<bool>,
    ) -> Result<ToolDispatchReadinessPermitV1, ToolScheduleErrorV1> {
        validate_request(&request)?;
        let intents = resource_lock_intents(&request)?;
        let mut resource_permits = Vec::with_capacity(intents.len());
        for (lock_key, exclusive) in intents {
            let lock = shared_rwlock(&self.resource_locks, lock_key)?;
            resource_permits.push(acquire_rwlock(lock, exclusive, cancellation).await?);
        }
        let local_limit = request.generation.local_concurrency_limit;
        let local = shared_semaphore(
            &self.local_generations,
            ToolInstanceExecutionGenerationMapKey::from(&request.generation),
            local_limit,
        )?;
        let local_permit = acquire_cancelable(local, cancellation).await?;
        Ok(ToolDispatchReadinessPermitV1 {
            _resource_permits: resource_permits,
            _local_permit: local_permit,
            global: self.global.clone(),
            timeout: Duration::from_millis(request.plan_call.timeout_ms),
        })
    }

    pub fn prune_idle_generations(&self) -> Result<usize, ToolScheduleErrorV1> {
        let mut generations =
            self.local_generations.lock().map_err(|_| ToolScheduleErrorV1::StateUnavailable)?;
        let before = generations.len();
        generations.retain(|_, semaphore| semaphore.strong_count() > 0);
        Ok(before.saturating_sub(generations.len()))
    }
}

fn validate_global_limit(limit: u32) -> Result<(), ToolScheduleErrorV1> {
    if !(aura_tool_contract::MIN_MAX_CONCURRENT_TOOL_CALLS
        ..=aura_tool_contract::MAX_MAX_CONCURRENT_TOOL_CALLS)
        .contains(&limit)
    {
        return Err(ToolScheduleErrorV1::InvalidFrozenPlan);
    }
    Ok(())
}

fn validate_request(request: &ToolScheduleRequestV1<'_>) -> Result<(), ToolScheduleErrorV1> {
    request
        .plan_call
        .local_concurrency_limit
        .is_none_or(|limit| limit == request.generation.local_concurrency_limit)
        .then_some(())
        .ok_or(ToolScheduleErrorV1::InvalidFrozenPlan)?;
    if !(aura_tool_contract::MIN_MAX_CONCURRENT_TOOL_CALLS
        ..=aura_tool_contract::MAX_MAX_CONCURRENT_TOOL_CALLS)
        .contains(&request.generation.local_concurrency_limit)
        || !(aura_tool_contract::MIN_TOOL_TIMEOUT_MS..=aura_tool_contract::MAX_TOOL_TIMEOUT_MS)
            .contains(&request.plan_call.timeout_ms)
    {
        return Err(ToolScheduleErrorV1::InvalidFrozenPlan);
    }
    if !request
        .plan_call
        .resolved_resource_claims
        .windows(2)
        .all(|pair| pair[0].canonical_lock_key.as_bytes() < pair[1].canonical_lock_key.as_bytes())
    {
        return Err(ToolScheduleErrorV1::InvalidFrozenPlan);
    }
    Ok(())
}

fn exclusive_resource_access(access: ResolvedResourceAccessV1) -> bool {
    !matches!(access, ResolvedResourceAccessV1::Read | ResolvedResourceAccessV1::Observe)
}

fn resource_lock_intents(
    request: &ToolScheduleRequestV1<'_>,
) -> Result<BTreeMap<String, bool>, ToolScheduleErrorV1> {
    let mut intents = BTreeMap::new();
    for claim in &request.plan_call.resolved_resource_claims {
        let exclusive = exclusive_resource_access(claim.access);
        intents
            .entry(claim.canonical_lock_key.clone())
            .and_modify(|current| *current = *current || exclusive)
            .or_insert(exclusive);
    }
    if let Some(source_lock_key) = request.source_lock_key {
        if source_lock_key.is_empty()
            || source_lock_key.contains('\0')
            || source_lock_key.chars().any(char::is_control)
        {
            return Err(ToolScheduleErrorV1::InvalidFrozenPlan);
        }
        intents.insert(format!("source:{source_lock_key}"), true);
    }
    Ok(intents)
}

fn shared_rwlock(
    map: &Mutex<BTreeMap<String, Weak<RwLock<()>>>>,
    key: String,
) -> Result<Arc<RwLock<()>>, ToolScheduleErrorV1> {
    let mut map = map.lock().map_err(|_| ToolScheduleErrorV1::StateUnavailable)?;
    if let Some(existing) = map.get(&key).and_then(Weak::upgrade) {
        return Ok(existing);
    }
    let lock = Arc::new(RwLock::new(()));
    map.insert(key, Arc::downgrade(&lock));
    Ok(lock)
}

async fn acquire_rwlock(
    lock: Arc<RwLock<()>>,
    exclusive: bool,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<HeldResourceLock, ToolScheduleErrorV1> {
    loop {
        if *cancellation.borrow() {
            return Err(ToolScheduleErrorV1::CancelledBeforeDispatch);
        }
        if exclusive {
            tokio::select! {
                biased;
                change = cancellation.changed() => {
                    if change.is_err() || *cancellation.borrow() {
                        return Err(ToolScheduleErrorV1::CancelledBeforeDispatch);
                    }
                }
                guard = Arc::clone(&lock).write_owned() => {
                    return Ok(HeldResourceLock::Exclusive(guard));
                }
            }
        } else {
            tokio::select! {
                biased;
                change = cancellation.changed() => {
                    if change.is_err() || *cancellation.borrow() {
                        return Err(ToolScheduleErrorV1::CancelledBeforeDispatch);
                    }
                }
                guard = Arc::clone(&lock).read_owned() => {
                    return Ok(HeldResourceLock::Shared(guard));
                }
            }
        }
    }
}

fn shared_semaphore<K>(
    map: &Mutex<BTreeMap<K, Weak<Semaphore>>>,
    key: K,
    limit: u32,
) -> Result<Arc<Semaphore>, ToolScheduleErrorV1>
where
    K: Ord,
{
    let mut map = map.lock().map_err(|_| ToolScheduleErrorV1::StateUnavailable)?;
    if let Some(existing) = map.get(&key).and_then(Weak::upgrade) {
        return Ok(existing);
    }
    let semaphore = Arc::new(Semaphore::new(
        usize::try_from(limit).map_err(|_| ToolScheduleErrorV1::InvalidFrozenPlan)?,
    ));
    map.insert(key, Arc::downgrade(&semaphore));
    Ok(semaphore)
}

async fn acquire_cancelable(
    semaphore: Arc<Semaphore>,
    cancellation: &mut watch::Receiver<bool>,
) -> Result<OwnedSemaphorePermit, ToolScheduleErrorV1> {
    loop {
        if *cancellation.borrow() {
            return Err(ToolScheduleErrorV1::CancelledBeforeDispatch);
        }
        tokio::select! {
            biased;
            change = cancellation.changed() => {
                if change.is_err() || *cancellation.borrow() {
                    return Err(ToolScheduleErrorV1::CancelledBeforeDispatch);
                }
            }
            permit = Arc::clone(&semaphore).acquire_owned() => {
                return permit.map_err(|_| ToolScheduleErrorV1::StateUnavailable);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use aura_domain_common::Revision;
    use aura_domain_common::ToolCallId;
    use aura_tool_contract::{
        ResolvedResourceAccessV1, ResolvedResourceClaimV1, ResolvedResourceKindV1, ToolVersion,
    };
    use tokio::{sync::Barrier, time};

    use super::*;

    fn plan_call(
        ordinal: u32,
        local_limit: u32,
        lock_key: Option<&str>,
    ) -> ToolExecutionPlanCallV1 {
        plan_locked(ordinal, local_limit, lock_key, ResolvedResourceAccessV1::Read)
    }

    fn plan_locked(
        ordinal: u32,
        local_limit: u32,
        lock_key: Option<&str>,
        access: ResolvedResourceAccessV1,
    ) -> ToolExecutionPlanCallV1 {
        ToolExecutionPlanCallV1 {
            tool_call_id: ToolCallId::from_bytes([u8::try_from(ordinal).expect("ordinal"); 16]),
            ordinal,
            dependency_tool_call_ids: vec![],
            execution_atomic_group: None,
            resolved_resource_claims: lock_key
                .map(|lock_key| {
                    vec![ResolvedResourceClaimV1 {
                        kind: ResolvedResourceKindV1::WorkspacePath,
                        access,
                        canonical_resource_identity: lock_key.to_owned(),
                        canonical_lock_key: lock_key.to_owned(),
                        core_derived: false,
                        resolution_evidence_hash: CanonicalHashV1::sha256(lock_key.as_bytes()),
                    }]
                })
                .unwrap_or_default(),
            timeout_ms: 1_000,
            max_retries: 3,
            local_concurrency_limit: Some(local_limit),
            entity_revision: Revision::INITIAL,
        }
    }

    fn generation(instance: u8, revision: u8, limit: u32) -> ToolInstanceExecutionGenerationKeyV1 {
        ToolInstanceExecutionGenerationKeyV1 {
            tool_instance_id: ToolInstanceId::from_bytes([instance; 16]),
            tool_version: ToolVersion::new(None, Revision::INITIAL).expect("version"),
            execution_fingerprint: CanonicalHashV1::sha256(&[revision]),
            local_concurrency_limit: limit,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn global_runtime_attempts_never_exceed_four() {
        let scheduler = ToolExecutionSchedulerV1::new(4).expect("scheduler");
        let active = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(5));
        let tasks = (1..=12)
            .map(|ordinal| {
                let scheduler = scheduler.clone();
                let active = Arc::clone(&active);
                let maximum = Arc::clone(&maximum);
                let barrier = Arc::clone(&barrier);
                tokio::spawn(async move {
                    let (_cancel_tx, mut cancel_rx) = watch::channel(false);
                    let call = plan_call(ordinal, 12, None);
                    let _permit = scheduler
                        .acquire_dispatch_permit(
                            ToolScheduleRequestV1 {
                                plan_call: &call,
                                generation: generation(1, 1, 12),
                                source_lock_key: None,
                            },
                            &mut cancel_rx,
                        )
                        .await
                        .expect("permit");
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    maximum.fetch_max(now, Ordering::SeqCst);
                    if ordinal <= 4 {
                        barrier.wait().await;
                    }
                    time::sleep(Duration::from_millis(15)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                })
            })
            .collect::<Vec<_>>();
        barrier.wait().await;
        for task in tasks {
            task.await.expect("task");
        }
        assert_eq!(maximum.load(Ordering::SeqCst), 4);
    }

    #[tokio::test]
    async fn same_generation_and_resource_serialize_but_new_generation_can_drain_in_parallel() {
        let scheduler = ToolExecutionSchedulerV1::new(4).expect("scheduler");
        let (_cancel_tx, mut cancel_rx) = watch::channel(false);
        let call_one = plan_call(1, 1, None);
        let permit_one = scheduler
            .acquire_dispatch_permit(
                ToolScheduleRequestV1 {
                    plan_call: &call_one,
                    generation: generation(1, 1, 1),
                    source_lock_key: None,
                },
                &mut cancel_rx,
            )
            .await
            .expect("first");
        let call_two = plan_call(2, 1, None);
        let (_same_generation_tx, mut same_generation_rx) = watch::channel(false);
        let mut same_generation = Box::pin(scheduler.acquire_dispatch_permit(
            ToolScheduleRequestV1 {
                plan_call: &call_two,
                generation: generation(1, 1, 1),
                source_lock_key: None,
            },
            &mut same_generation_rx,
        ));
        assert!(time::timeout(Duration::from_millis(20), &mut same_generation).await.is_err());

        let call_three = plan_call(3, 1, None);
        scheduler
            .acquire_dispatch_permit(
                ToolScheduleRequestV1 {
                    plan_call: &call_three,
                    generation: generation(1, 2, 1),
                    source_lock_key: None,
                },
                &mut cancel_rx,
            )
            .await
            .expect("new generation runs while old drains");
        drop(permit_one);
        same_generation.await.expect("old generation resumes");

        let locked_one = plan_locked(4, 2, Some("workspace:path"), ResolvedResourceAccessV1::Write);
        let resource_permit = scheduler
            .acquire_dispatch_permit(
                ToolScheduleRequestV1 {
                    plan_call: &locked_one,
                    generation: generation(2, 1, 2),
                    source_lock_key: None,
                },
                &mut cancel_rx,
            )
            .await
            .expect("resource one");
        let locked_two = plan_locked(5, 2, Some("workspace:path"), ResolvedResourceAccessV1::Write);
        assert!(
            time::timeout(
                Duration::from_millis(20),
                scheduler.acquire_dispatch_permit(
                    ToolScheduleRequestV1 {
                        plan_call: &locked_two,
                        generation: generation(3, 1, 2),
                        source_lock_key: None,
                    },
                    &mut cancel_rx,
                )
            )
            .await
            .is_err()
        );
        drop(resource_permit);
    }

    /// Two operations on **different** files run at the same time; the same file serialises; and a
    /// reader does not exclude another reader.
    ///
    /// An earlier fix removed a claim that made every file operation in a workspace
    /// exclude every other one. That fix is only observable at this level: the scheduler decides
    /// exclusion from each plan call's own `canonical_lock_key`, so the evidence has to be that
    /// two *different* keys are both held at once, not that a workspace-wide key is absent.
    ///
    /// The last two assertions are the counter-examples that keep this from becoming "nothing
    /// serialises": the same path at write access really does exclude, and a second writer of a
    /// path a reader holds is kept out — so the parallel case above is a real distinction rather
    /// than an implementation that stopped locking altogether.
    #[tokio::test]
    async fn different_file_keys_run_in_parallel_while_one_key_serialises() {
        let scheduler = ToolExecutionSchedulerV1::new(8).expect("scheduler");
        let (_cancel_tx, mut cancel_rx) = watch::channel(false);

        // Two different files, both writers, in the same workspace and the same generation.
        let first =
            plan_locked(1, 4, Some("workspace:path:a.txt"), ResolvedResourceAccessV1::Write);
        let second =
            plan_locked(2, 4, Some("workspace:path:b.txt"), ResolvedResourceAccessV1::Write);
        let held_one = scheduler
            .acquire_dispatch_permit(
                ToolScheduleRequestV1 {
                    plan_call: &first,
                    generation: generation(1, 1, 4),
                    source_lock_key: None,
                },
                &mut cancel_rx,
            )
            .await
            .expect("the first file's lock is free");
        // The decisive fact: the second file's permit is granted **while the first is held**.
        let held_two = time::timeout(
            Duration::from_secs(5),
            scheduler.acquire_dispatch_permit(
                ToolScheduleRequestV1 {
                    plan_call: &second,
                    generation: generation(1, 1, 4),
                    source_lock_key: None,
                },
                &mut cancel_rx,
            ),
        )
        .await
        .expect("a different file must not be blocked by another file's write")
        .expect("the second file's lock is free");

        // A reader of an unrelated path is admitted too: reads are shared, and the workspace is
        // not a single lock.
        let reader =
            plan_locked(3, 4, Some("workspace:path:c.txt"), ResolvedResourceAccessV1::Read);
        let _held_read = time::timeout(
            Duration::from_secs(5),
            scheduler.acquire_dispatch_permit(
                ToolScheduleRequestV1 {
                    plan_call: &reader,
                    generation: generation(1, 1, 4),
                    source_lock_key: None,
                },
                &mut cancel_rx,
            ),
        )
        .await
        .expect("an unrelated read must not be blocked")
        .expect("the read lock is free");

        // Counter-example one: the **same** path at write access is excluded while held.
        let same_path =
            plan_locked(4, 4, Some("workspace:path:a.txt"), ResolvedResourceAccessV1::Write);
        assert!(
            time::timeout(
                Duration::from_millis(50),
                scheduler.acquire_dispatch_permit(
                    ToolScheduleRequestV1 {
                        plan_call: &same_path,
                        generation: generation(1, 1, 4),
                        source_lock_key: None,
                    },
                    &mut cancel_rx,
                )
            )
            .await
            .is_err(),
            "two writers of the same file must serialise"
        );

        // Counter-example two: a writer is excluded by a reader that is still holding the path,
        // so excluding writers from readers was not lost along with the workspace-wide lock.
        let read_holder =
            plan_locked(5, 4, Some("workspace:path:d.txt"), ResolvedResourceAccessV1::Read);
        let held_shared = scheduler
            .acquire_dispatch_permit(
                ToolScheduleRequestV1 {
                    plan_call: &read_holder,
                    generation: generation(2, 1, 4),
                    source_lock_key: None,
                },
                &mut cancel_rx,
            )
            .await
            .expect("the shared read lock is free");
        let write_same =
            plan_locked(6, 4, Some("workspace:path:d.txt"), ResolvedResourceAccessV1::Write);
        assert!(
            time::timeout(
                Duration::from_millis(50),
                scheduler.acquire_dispatch_permit(
                    ToolScheduleRequestV1 {
                        plan_call: &write_same,
                        generation: generation(2, 1, 4),
                        source_lock_key: None,
                    },
                    &mut cancel_rx,
                )
            )
            .await
            .is_err(),
            "a writer must not enter while a reader holds the same path"
        );
        drop(held_shared);
        // Once the reader is done, the writer really proceeds — the exclusion was a wait, not a
        // permanent refusal.
        let _after = time::timeout(
            Duration::from_secs(5),
            scheduler.acquire_dispatch_permit(
                ToolScheduleRequestV1 {
                    plan_call: &write_same,
                    generation: generation(2, 1, 4),
                    source_lock_key: None,
                },
                &mut cancel_rx,
            ),
        )
        .await
        .expect("the writer proceeds once the reader releases")
        .expect("the released path is free");
        drop(held_one);
        drop(held_two);
    }

    #[tokio::test]
    async fn shared_read_locks_do_not_serialize() {
        let scheduler = ToolExecutionSchedulerV1::new(4).expect("scheduler");
        let (_cancel_tx, mut cancel_rx) = watch::channel(false);
        let first_call = plan_call(1, 2, Some("workspace:path"));
        let _first = scheduler
            .acquire_dispatch_permit(
                ToolScheduleRequestV1 {
                    plan_call: &first_call,
                    generation: generation(1, 1, 2),
                    source_lock_key: None,
                },
                &mut cancel_rx,
            )
            .await
            .expect("first reader");
        let second_call = plan_call(2, 2, Some("workspace:path"));
        time::timeout(
            Duration::from_millis(50),
            scheduler.acquire_dispatch_permit(
                ToolScheduleRequestV1 {
                    plan_call: &second_call,
                    generation: generation(2, 1, 2),
                    source_lock_key: None,
                },
                &mut cancel_rx,
            ),
        )
        .await
        .expect("shared read must not wait for another reader")
        .expect("second reader");
    }

    #[tokio::test]
    async fn exclusive_write_waits_for_shared_read() {
        let scheduler = ToolExecutionSchedulerV1::new(4).expect("scheduler");
        let (_cancel_tx, mut cancel_rx) = watch::channel(false);
        let reader_call = plan_call(1, 2, Some("workspace:path"));
        let reader = scheduler
            .acquire_dispatch_permit(
                ToolScheduleRequestV1 {
                    plan_call: &reader_call,
                    generation: generation(1, 1, 2),
                    source_lock_key: None,
                },
                &mut cancel_rx,
            )
            .await
            .expect("reader");
        let writer_call =
            plan_locked(2, 2, Some("workspace:path"), ResolvedResourceAccessV1::Write);
        assert!(
            time::timeout(
                Duration::from_millis(20),
                scheduler.acquire_dispatch_permit(
                    ToolScheduleRequestV1 {
                        plan_call: &writer_call,
                        generation: generation(2, 1, 2),
                        source_lock_key: None,
                    },
                    &mut cancel_rx,
                )
            )
            .await
            .is_err()
        );
        drop(reader);
        scheduler
            .acquire_dispatch_permit(
                ToolScheduleRequestV1 {
                    plan_call: &writer_call,
                    generation: generation(2, 1, 2),
                    source_lock_key: None,
                },
                &mut cancel_rx,
            )
            .await
            .expect("writer after reader");
    }

    #[tokio::test]
    async fn cancellation_while_waiting_never_consumes_a_global_slot() {
        let scheduler = ToolExecutionSchedulerV1::new(1).expect("scheduler");
        let (_first_tx, mut first_rx) = watch::channel(false);
        let first_call = plan_locked(1, 1, Some("same"), ResolvedResourceAccessV1::Write);
        let _first = scheduler
            .acquire_dispatch_permit(
                ToolScheduleRequestV1 {
                    plan_call: &first_call,
                    generation: generation(1, 1, 1),
                    source_lock_key: None,
                },
                &mut first_rx,
            )
            .await
            .expect("first");
        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        let second_call = plan_locked(2, 1, Some("same"), ResolvedResourceAccessV1::Write);
        let waiting = scheduler.acquire_dispatch_permit(
            ToolScheduleRequestV1 {
                plan_call: &second_call,
                generation: generation(2, 1, 1),
                source_lock_key: None,
            },
            &mut cancel_rx,
        );
        tokio::pin!(waiting);
        cancel_tx.send(true).expect("cancel");
        assert!(matches!(waiting.await, Err(ToolScheduleErrorV1::CancelledBeforeDispatch)));
    }

    #[tokio::test]
    async fn host_capacity_wait_can_happen_before_the_global_runtime_slot() {
        let scheduler = ToolExecutionSchedulerV1::new(1).expect("scheduler");
        let (_waiting_tx, mut waiting_rx) = watch::channel(false);
        let waiting_call = plan_call(1, 1, None);
        let _waiting_for_host = scheduler
            .acquire_dispatch_readiness(
                ToolScheduleRequestV1 {
                    plan_call: &waiting_call,
                    generation: generation(1, 1, 1),
                    source_lock_key: None,
                },
                &mut waiting_rx,
            )
            .await
            .expect("readiness before Host capacity");

        let (_active_tx, mut active_rx) = watch::channel(false);
        let active_call = plan_call(2, 1, None);
        let active_readiness = scheduler
            .acquire_dispatch_readiness(
                ToolScheduleRequestV1 {
                    plan_call: &active_call,
                    generation: generation(2, 1, 1),
                    source_lock_key: None,
                },
                &mut active_rx,
            )
            .await
            .expect("independent readiness");
        time::timeout(
            Duration::from_millis(50),
            active_readiness.activate_runtime_slot(&mut active_rx),
        )
        .await
        .expect("Host wait must not consume the global Runtime slot")
        .expect("active Runtime permit");
    }
}
