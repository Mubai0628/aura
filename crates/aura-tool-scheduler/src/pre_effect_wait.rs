//! Temporary execution-capacity release after a proven pre-effect resource refusal.
//!
//! The dispatch permit still owns its frozen resource locks and Attempt. Only
//! the executing slots are suspended, and they must be reacquired before the
//! backend can try its original authorization and effect again. An executing
//! or Unknown backend never calls this entry merely because its caller ended.
use std::{
    future::Future,
    sync::{Arc, Mutex},
};

use tokio::sync::{OwnedSemaphorePermit, Semaphore, watch};

use crate::{
    ToolScheduleErrorV1,
    capacity::{ToolCapacity, ToolCapacitySlot},
};

struct ExecutingSlots {
    _local: OwnedSemaphorePermit,
    _global: ToolCapacitySlot,
}

#[derive(Clone)]
pub struct DispatchRuntimeCapacity {
    local: Arc<Semaphore>,
    global: ToolCapacity,
    slots: Arc<Mutex<Option<ExecutingSlots>>>,
}

impl DispatchRuntimeCapacity {
    pub(crate) fn new(
        local: OwnedSemaphorePermit,
        global: ToolCapacitySlot,
        capacity: ToolCapacity,
    ) -> Self {
        Self {
            local: Arc::clone(local.semaphore()),
            global: capacity,
            slots: Arc::new(Mutex::new(Some(ExecutingSlots { _local: local, _global: global }))),
        }
    }

    /// Scopes the original dispatch's cancellation and absolute deadline to its
    /// owned backend task. Tokio task locals are explicitly installed there.
    pub async fn scope<F: Future>(
        self,
        cancellation: watch::Receiver<bool>,
        deadline: tokio::time::Instant,
        work: F,
    ) -> F::Output {
        CAPACITY.scope(WaitContext { capacity: self, cancellation, deadline }, work).await
    }
}

#[derive(Clone)]
struct WaitContext {
    capacity: DispatchRuntimeCapacity,
    cancellation: watch::Receiver<bool>,
    deadline: tokio::time::Instant,
}

tokio::task_local! { static CAPACITY: WaitContext; }

pub struct PreEffectCapacityWait {
    context: WaitContext,
}

/// Call only after the common file owner proved that no mutation began.
/// Returns None outside an orchestrated dispatch; it never invents a limiter.
pub fn suspend_after_resource_refusal() -> Result<Option<PreEffectCapacityWait>, ToolScheduleErrorV1>
{
    let Ok(context) = CAPACITY.try_with(Clone::clone) else {
        return Ok(None);
    };
    let slots = context
        .capacity
        .slots
        .lock()
        .map_err(|_| ToolScheduleErrorV1::StateUnavailable)?
        .take()
        .ok_or(ToolScheduleErrorV1::StateUnavailable)?;
    drop(slots);
    Ok(Some(PreEffectCapacityWait { context }))
}

impl PreEffectCapacityWait {
    /// Reclaims the same generation's local and global slots within the original
    /// dispatch budget. Cancellation leaves the pre-effect attempt unexecuted.
    pub async fn resume(self) -> Result<(), ToolScheduleErrorV1> {
        let work = async {
            let mut cancellation = self.context.cancellation.clone();
            if *cancellation.borrow() {
                return Err(ToolScheduleErrorV1::CancelledBeforeDispatch);
            }
            let local = tokio::select! {
                biased;
                _ = cancellation.changed() => return Err(ToolScheduleErrorV1::CancelledBeforeDispatch),
                result = Arc::clone(&self.context.capacity.local).acquire_owned() =>
                    result.map_err(|_| ToolScheduleErrorV1::StateUnavailable)?,
            };
            let global = self
                .context
                .capacity
                .global
                .acquire(&mut cancellation)
                .await
                .ok_or(ToolScheduleErrorV1::CancelledBeforeDispatch)?;
            if *cancellation.borrow() {
                return Err(ToolScheduleErrorV1::CancelledBeforeDispatch);
            }
            let mut slots = self
                .context
                .capacity
                .slots
                .lock()
                .map_err(|_| ToolScheduleErrorV1::StateUnavailable)?;
            if slots.is_some() {
                return Err(ToolScheduleErrorV1::StateUnavailable);
            }
            *slots = Some(ExecutingSlots { _local: local, _global: global });
            Ok(())
        };
        tokio::time::timeout_at(self.context.deadline, work)
            .await
            .map_err(|_| ToolScheduleErrorV1::CancelledBeforeDispatch)?
    }
}
