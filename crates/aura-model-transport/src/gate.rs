use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use aura_domain_common::EndpointId;
use aura_model_domain::ModelRequestPriority;
use tokio::sync::Notify;

use crate::{ModelCancellationToken, ModelTransportError};

const MAX_PRIORITY_BYPASSES: u32 = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EndpointGateMetrics {
    pub active_requests: u32,
    pub queued_requests: u32,
    pub configured_limit: u32,
}

#[derive(Clone)]
pub struct EndpointGate {
    endpoint_id: EndpointId,
    inner: Arc<GateInner>,
}

struct GateInner {
    state: Mutex<GateState>,
}

struct GateState {
    limit: u32,
    active: u32,
    next_sequence: u64,
    queue: VecDeque<Arc<Waiter>>,
}

struct Waiter {
    sequence: u64,
    priority: ModelRequestPriority,
    bypasses: Mutex<u32>,
    granted: AtomicBool,
    cancelled: AtomicBool,
    notify: Notify,
}

impl EndpointGate {
    pub fn new(endpoint_id: EndpointId, limit: u32) -> Result<Self, ModelTransportError> {
        if limit == 0 {
            return Err(ModelTransportError::InvalidEndpointLimit(endpoint_id));
        }
        Ok(Self {
            endpoint_id,
            inner: Arc::new(GateInner {
                state: Mutex::new(GateState {
                    limit,
                    active: 0,
                    next_sequence: 0,
                    queue: VecDeque::new(),
                }),
            }),
        })
    }

    #[must_use]
    pub const fn endpoint_id(&self) -> EndpointId {
        self.endpoint_id
    }

    pub async fn acquire(
        &self,
        priority: ModelRequestPriority,
        cancellation: &ModelCancellationToken,
    ) -> Result<EndpointPermit, ModelTransportError> {
        if cancellation.is_cancelled() {
            return Err(ModelTransportError::Cancelled);
        }
        let waiter = {
            let mut state =
                self.inner.state.lock().map_err(|_| ModelTransportError::GatePoisoned)?;
            if state.active < state.limit && state.queue.is_empty() {
                state.active = state.active.saturating_add(1);
                return Ok(EndpointPermit { gate: Some(self.clone()) });
            }
            let sequence = state.next_sequence;
            state.next_sequence = state.next_sequence.saturating_add(1);
            let waiter = Arc::new(Waiter {
                sequence,
                priority,
                bypasses: Mutex::new(0),
                granted: AtomicBool::new(false),
                cancelled: AtomicBool::new(false),
                notify: Notify::new(),
            });
            state.queue.push_back(waiter.clone());
            waiter
        };

        loop {
            let notified = waiter.notify.notified();
            if waiter.granted.load(Ordering::Acquire) {
                if cancellation.is_cancelled() {
                    self.release_permit()?;
                    return Err(ModelTransportError::Cancelled);
                }
                return Ok(EndpointPermit { gate: Some(self.clone()) });
            }
            tokio::select! {
                () = notified => {}
                () = cancellation.cancelled() => {
                    waiter.cancelled.store(true, Ordering::Release);
                    let mut state = self.inner.state.lock()
                        .map_err(|_| ModelTransportError::GatePoisoned)?;
                    if waiter.granted.load(Ordering::Acquire) {
                        state.active = state.active.saturating_sub(1);
                    } else {
                        state.queue.retain(|candidate| !Arc::ptr_eq(candidate, &waiter));
                    }
                    dispatch(&mut state)?;
                    return Err(ModelTransportError::Cancelled);
                }
            }
        }
    }

    pub fn update_limit(&self, limit: u32) -> Result<(), ModelTransportError> {
        if limit == 0 {
            return Err(ModelTransportError::InvalidEndpointLimit(self.endpoint_id));
        }
        let mut state = self.inner.state.lock().map_err(|_| ModelTransportError::GatePoisoned)?;
        state.limit = limit;
        dispatch(&mut state)
    }

    pub fn metrics(&self) -> Result<EndpointGateMetrics, ModelTransportError> {
        let state = self.inner.state.lock().map_err(|_| ModelTransportError::GatePoisoned)?;
        Ok(EndpointGateMetrics {
            active_requests: state.active,
            queued_requests: u32::try_from(state.queue.len()).unwrap_or(u32::MAX),
            configured_limit: state.limit,
        })
    }

    fn release_permit(&self) -> Result<(), ModelTransportError> {
        let mut state = self.inner.state.lock().map_err(|_| ModelTransportError::GatePoisoned)?;
        state.active = state.active.saturating_sub(1);
        dispatch(&mut state)
    }
}

pub struct EndpointPermit {
    gate: Option<EndpointGate>,
}

impl Drop for EndpointPermit {
    fn drop(&mut self) {
        if let Some(gate) = self.gate.take() {
            let _ = gate.release_permit();
        }
    }
}

#[derive(Default)]
pub struct EndpointGateRegistry {
    gates: Mutex<BTreeMap<EndpointId, EndpointGate>>,
}

impl EndpointGateRegistry {
    pub fn register_or_update(
        &self,
        endpoint_id: EndpointId,
        limit: u32,
    ) -> Result<EndpointGate, ModelTransportError> {
        let mut gates = self.gates.lock().map_err(|_| ModelTransportError::GatePoisoned)?;
        if let Some(gate) = gates.get(&endpoint_id) {
            gate.update_limit(limit)?;
            return Ok(gate.clone());
        }
        let gate = EndpointGate::new(endpoint_id, limit)?;
        gates.insert(endpoint_id, gate.clone());
        Ok(gate)
    }

    pub fn get(
        &self,
        endpoint_id: EndpointId,
    ) -> Result<Option<EndpointGate>, ModelTransportError> {
        self.gates
            .lock()
            .map_err(|_| ModelTransportError::GatePoisoned)
            .map(|gates| gates.get(&endpoint_id).cloned())
    }
}

fn dispatch(state: &mut GateState) -> Result<(), ModelTransportError> {
    state.queue.retain(|waiter| !waiter.cancelled.load(Ordering::Acquire));
    while state.active < state.limit && !state.queue.is_empty() {
        let selected = select_waiter(&state.queue)?;
        let Some(waiter) = state.queue.remove(selected) else {
            return Err(ModelTransportError::GatePoisoned);
        };
        for candidate in &state.queue {
            let mut bypasses =
                candidate.bypasses.lock().map_err(|_| ModelTransportError::GatePoisoned)?;
            *bypasses = bypasses.saturating_add(1);
        }
        state.active = state.active.saturating_add(1);
        waiter.granted.store(true, Ordering::Release);
        waiter.notify.notify_one();
    }
    Ok(())
}

fn select_waiter(queue: &VecDeque<Arc<Waiter>>) -> Result<usize, ModelTransportError> {
    let mut forced = None::<(usize, u32, u64)>;
    let mut prioritized = None::<(usize, u8, u64)>;
    for (index, waiter) in queue.iter().enumerate() {
        let bypasses = *waiter.bypasses.lock().map_err(|_| ModelTransportError::GatePoisoned)?;
        if bypasses >= MAX_PRIORITY_BYPASSES
            && forced.is_none_or(|(_, current, sequence)| {
                bypasses > current || (bypasses == current && waiter.sequence < sequence)
            })
        {
            forced = Some((index, bypasses, waiter.sequence));
        }
        let rank = priority_rank(waiter.priority);
        if prioritized.is_none_or(|(_, current, sequence)| {
            rank < current || (rank == current && waiter.sequence < sequence)
        }) {
            prioritized = Some((index, rank, waiter.sequence));
        }
    }
    forced
        .map(|(index, _, _)| index)
        .or_else(|| prioritized.map(|(index, _, _)| index))
        .ok_or(ModelTransportError::GatePoisoned)
}

const fn priority_rank(priority: ModelRequestPriority) -> u8 {
    match priority {
        ModelRequestPriority::BlockingRecovery => 0,
        ModelRequestPriority::InteractiveParent => 1,
        ModelRequestPriority::ChildAgent => 2,
        ModelRequestPriority::Maintenance => 3,
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use aura_domain_common::EndpointId;
    use aura_model_domain::ModelRequestPriority;
    use tokio::sync::mpsc;

    use super::EndpointGate;
    use crate::{ModelCancellationToken, ModelTransportError};

    #[tokio::test]
    async fn cancelling_a_queued_waiter_removes_it_without_leaking_a_permit() {
        let gate = EndpointGate::new(EndpointId::new(), 1).expect("gate");
        let holder = gate
            .acquire(ModelRequestPriority::InteractiveParent, &ModelCancellationToken::default())
            .await
            .expect("holder permit");
        let cancellation = ModelCancellationToken::default();
        let queued_gate = gate.clone();
        let queued_cancellation = cancellation.clone();
        let queued = tokio::spawn(async move {
            queued_gate.acquire(ModelRequestPriority::Maintenance, &queued_cancellation).await
        });
        tokio::task::yield_now().await;
        assert_eq!(gate.metrics().expect("metrics").queued_requests, 1);

        cancellation.cancel();
        assert!(matches!(queued.await.expect("waiter task"), Err(ModelTransportError::Cancelled)));
        assert_eq!(gate.metrics().expect("metrics").queued_requests, 0);
        assert_eq!(gate.metrics().expect("metrics").active_requests, 1);

        drop(holder);
        assert_eq!(gate.metrics().expect("metrics").active_requests, 0);
    }

    #[tokio::test]
    async fn lowering_limit_does_not_cancel_holders_and_blocks_new_work() {
        let gate = EndpointGate::new(EndpointId::new(), 2).expect("gate");
        let first = gate
            .acquire(ModelRequestPriority::InteractiveParent, &ModelCancellationToken::default())
            .await
            .expect("first permit");
        let second = gate
            .acquire(ModelRequestPriority::InteractiveParent, &ModelCancellationToken::default())
            .await
            .expect("second permit");
        gate.update_limit(1).expect("lower limit");
        assert_eq!(gate.metrics().expect("metrics").active_requests, 2);

        let queued_gate = gate.clone();
        let queued = tokio::spawn(async move {
            queued_gate
                .acquire(ModelRequestPriority::BlockingRecovery, &ModelCancellationToken::default())
                .await
        });
        tokio::task::yield_now().await;
        assert_eq!(gate.metrics().expect("metrics").queued_requests, 1);
        drop(first);
        assert_eq!(gate.metrics().expect("metrics").active_requests, 1);
        tokio::task::yield_now().await;
        assert!(!queued.is_finished());

        drop(second);
        let third = tokio::time::timeout(std::time::Duration::from_secs(1), queued)
            .await
            .expect("queued waiter woke")
            .expect("waiter task")
            .expect("third permit");
        assert_eq!(gate.metrics().expect("metrics").active_requests, 1);
        drop(third);
        assert_eq!(gate.metrics().expect("metrics").active_requests, 0);
    }

    #[tokio::test]
    async fn raising_limit_immediately_wakes_queued_work() {
        let gate = EndpointGate::new(EndpointId::new(), 1).expect("gate");
        let holder = gate
            .acquire(ModelRequestPriority::InteractiveParent, &ModelCancellationToken::default())
            .await
            .expect("holder permit");
        let queued_gate = gate.clone();
        let queued = tokio::spawn(async move {
            queued_gate
                .acquire(ModelRequestPriority::ChildAgent, &ModelCancellationToken::default())
                .await
        });
        wait_for_queue_depth(&gate, 1).await;

        gate.update_limit(2).expect("raise limit");
        let second = tokio::time::timeout(Duration::from_secs(1), queued)
            .await
            .expect("queued waiter woke")
            .expect("waiter task")
            .expect("second permit");
        let metrics = gate.metrics().expect("metrics");
        assert_eq!(metrics.configured_limit, 2);
        assert_eq!(metrics.active_requests, 2);
        assert_eq!(metrics.queued_requests, 0);

        drop(holder);
        drop(second);
        assert_eq!(gate.metrics().expect("metrics").active_requests, 0);
    }

    #[tokio::test]
    async fn all_frozen_request_sources_share_priority_order_and_fifo_within_a_priority() {
        let gate = EndpointGate::new(EndpointId::new(), 1).expect("gate");
        let holder = gate
            .acquire(ModelRequestPriority::InteractiveParent, &ModelCancellationToken::default())
            .await
            .expect("holder");
        let (sender, mut receiver) = mpsc::channel(4);
        let priorities = [
            ModelRequestPriority::Maintenance,
            ModelRequestPriority::ChildAgent,
            ModelRequestPriority::InteractiveParent,
            ModelRequestPriority::BlockingRecovery,
        ];
        let mut waiters = Vec::new();
        for (index, priority) in priorities.into_iter().enumerate() {
            let queued_gate = gate.clone();
            let queued_sender = sender.clone();
            waiters.push(tokio::spawn(async move {
                let permit = queued_gate
                    .acquire(priority, &ModelCancellationToken::default())
                    .await
                    .expect("queued permit");
                queued_sender.send((index, priority)).await.expect("record grant");
                drop(permit);
            }));
            wait_for_queue_depth(&gate, u32::try_from(index + 1).expect("queue depth")).await;
        }
        drop(sender);
        drop(holder);

        let mut granted = Vec::new();
        while let Some(item) = receiver.recv().await {
            granted.push(item);
        }
        for waiter in waiters {
            waiter.await.expect("waiter task");
        }
        assert_eq!(
            granted,
            vec![
                (3, ModelRequestPriority::BlockingRecovery),
                (2, ModelRequestPriority::InteractiveParent),
                (1, ModelRequestPriority::ChildAgent),
                (0, ModelRequestPriority::Maintenance),
            ]
        );
    }

    #[tokio::test]
    async fn aging_prevents_maintenance_from_permanent_starvation() {
        let gate = EndpointGate::new(EndpointId::new(), 1).expect("gate");
        let holder = gate
            .acquire(ModelRequestPriority::InteractiveParent, &ModelCancellationToken::default())
            .await
            .expect("holder");
        let (sender, mut receiver) = mpsc::channel(10);
        let maintenance_gate = gate.clone();
        let maintenance_sender = sender.clone();
        let mut waiters = vec![tokio::spawn(async move {
            let permit = maintenance_gate
                .acquire(ModelRequestPriority::Maintenance, &ModelCancellationToken::default())
                .await
                .expect("maintenance permit");
            maintenance_sender.send("maintenance").await.expect("record maintenance");
            drop(permit);
        })];
        wait_for_queue_depth(&gate, 1).await;

        for index in 0..9 {
            let recovery_gate = gate.clone();
            let recovery_sender = sender.clone();
            waiters.push(tokio::spawn(async move {
                let permit = recovery_gate
                    .acquire(
                        ModelRequestPriority::BlockingRecovery,
                        &ModelCancellationToken::default(),
                    )
                    .await
                    .expect("recovery permit");
                recovery_sender.send("recovery").await.expect("record recovery");
                drop(permit);
            }));
            wait_for_queue_depth(&gate, u32::try_from(index + 2).expect("queue depth")).await;
        }
        drop(sender);
        drop(holder);

        let mut granted = Vec::new();
        while let Some(kind) = receiver.recv().await {
            granted.push(kind);
        }
        for waiter in waiters {
            waiter.await.expect("waiter task");
        }
        assert_eq!(granted.len(), 10);
        assert_eq!(granted[8], "maintenance");
        assert_eq!(granted.iter().filter(|kind| **kind == "recovery").count(), 9);
    }

    async fn wait_for_queue_depth(gate: &EndpointGate, expected: u32) {
        tokio::time::timeout(Duration::from_secs(1), async {
            while gate.metrics().expect("metrics").queued_requests != expected {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("queue depth");
    }
}
