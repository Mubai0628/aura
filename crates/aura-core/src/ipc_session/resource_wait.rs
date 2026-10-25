//! A request's resource wait keeps its task; retries must reacquire execution capacity.
use std::{
    future::Future,
    sync::{Arc, OnceLock},
    time::Instant,
};

use tokio::sync::{mpsc, oneshot};

#[derive(Default)]
pub(crate) struct ResourceWaitSignals {
    receiver: Option<mpsc::UnboundedReceiver<ResourceWaitSignal>>,
}

pub(crate) enum ResourceWaitSignal {
    Entered { ticket: u64 },
    Resume { ticket: u64, permit: oneshot::Sender<()> },
}

impl ResourceWaitSignals {
    pub(crate) fn channel() -> (ResourceWaitHandler, Self) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (
            ResourceWaitHandler { sender, ticket: 0, deadline: Arc::new(OnceLock::new()) },
            Self { receiver: Some(receiver) },
        )
    }

    pub(crate) fn drain(&mut self) -> Vec<ResourceWaitSignal> {
        let mut signals = Vec::new();
        if let Some(receiver) = &mut self.receiver {
            while let Ok(signal) = receiver.try_recv() {
                signals.push(signal);
            }
        }
        signals
    }
}

#[derive(Clone)]
pub(crate) struct ResourceWaitHandler {
    sender: mpsc::UnboundedSender<ResourceWaitSignal>,
    ticket: u64,
    // Per request, shared with its owned continuation. Dropping it reclaims the deadline.
    deadline: Arc<OnceLock<Instant>>,
}

impl ResourceWaitHandler {
    pub(crate) fn for_ticket(&self, ticket: u64) -> Self {
        Self { sender: self.sender.clone(), ticket, deadline: Arc::new(OnceLock::new()) }
    }

    pub(crate) fn entered(&self, deadline: Instant) {
        let _ = self.deadline.set(deadline);
        let _ = self.sender.send(ResourceWaitSignal::Entered { ticket: self.ticket });
    }

    /// The reader grants capacity to the existing task, never starts the request again.
    pub(crate) async fn resume(&self, deadline: Instant) -> bool {
        let deadline = *self.deadline.get_or_init(|| deadline);
        if deadline <= Instant::now() {
            return false;
        }
        let (permit, granted) = oneshot::channel();
        if self.sender.send(ResourceWaitSignal::Resume { ticket: self.ticket, permit }).is_err() {
            return false;
        }
        matches!(tokio::time::timeout_at(deadline.into(), granted).await, Ok(Ok(())))
    }

    #[cfg(test)]
    pub(crate) fn deadline(&self) -> Option<Instant> {
        self.deadline.get().copied()
    }
}

tokio::task_local! {
    pub(crate) static RESOURCE_WAIT_HANDLER: ResourceWaitHandler;
}

pub(crate) fn current_resource_wait_handler() -> Option<ResourceWaitHandler> {
    RESOURCE_WAIT_HANDLER.try_with(Clone::clone).ok().filter(|handler| !handler.sender.is_closed())
}

/// Tokio task locals do not propagate into spawned tasks. Carry the same request explicitly
/// into the Broker's owned work, whose lifetime can outlast the connection's handler.
pub(crate) async fn with_resource_wait_handler<T>(
    handler: Option<ResourceWaitHandler>,
    work: impl Future<Output = T>,
) -> T {
    match handler {
        Some(handler) => RESOURCE_WAIT_HANDLER.scope(handler, work).await,
        None => work.await,
    }
}
