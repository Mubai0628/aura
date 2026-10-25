//! Async ownership of P04 file publication; only a proven pre-effect refusal retries.
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use super::*;
use crate::blocking_pool::BlockingAdmission;

struct Cancellation {
    id: CorrelationId,
    retired: AtomicBool,
}
impl Drop for Cancellation {
    fn drop(&mut self) {
        aura_fs_tools::clear_resource_wait_cancel(self.id);
    }
}
struct Owner(Arc<Cancellation>);
impl Drop for Owner {
    fn drop(&mut self) {
        self.0.retired.store(true, Ordering::Release);
        // A worker admitted before this future ended still owns the checkpoint.
        aura_fs_tools::cancel_resource_wait(self.0.id);
    }
}

impl P04Services {
    pub(super) async fn dispatch_publication(
        &self,
        method_name: &str,
        params: &Value,
        id: CorrelationId,
    ) -> Result<Value, IpcErrorEnvelope> {
        let deadline = Instant::now()
            + self.fs.mutation_wait_budget().min(crate::blocking_pool::BLOCKING_ADMISSION_BUDGET);
        let cancellation = Arc::new(Cancellation { id, retired: AtomicBool::new(false) });
        let _owner = Owner(Arc::clone(&cancellation));
        let waiting = crate::ipc_session::current_resource_wait_handler();
        loop {
            check_cancelled(id)?;
            let service = self.clone();
            let method = method_name.to_owned();
            let input = params.clone();
            let checkpoint = Arc::clone(&cancellation);
            let (sender, receiver) = tokio::sync::oneshot::channel();
            let result = self
                .publication_workers
                .run_settled(
                    deadline,
                    move || {
                        if checkpoint.retired.load(Ordering::Acquire) {
                            return Err(crate::p03::cancelled_envelope(id));
                        }
                        check_cancelled(id)?;
                        match method.as_str() {
                            method::ARTIFACT_EXPORT => {
                                encode(service.export_artifact(decode(&input, id)?, id)?, id)
                            }
                            method::SECURITY_QUARANTINE_RELEASE => {
                                encode(service.release_quarantine(decode(&input, id)?, id)?, id)
                            }
                            _ => unreachable!("only P04 file publishers use this owner"),
                        }
                    },
                    move |value: &Result<Value, IpcErrorEnvelope>| {
                        let _ = sender.send(value.clone());
                    },
                )
                .await;
            let result = match result {
                BlockingAdmission::Completed(result) => result,
                BlockingAdmission::Refused { waited_ms } => {
                    return crate::p03::P03Dispatch::Refused { waited_ms }
                        .into_route_result(id, None);
                }
                // Broker owns the real completion even if its connection's caller has timed out.
                BlockingAdmission::Unknown { waited_ms } => match receiver.await {
                    Ok(result) => result,
                    Err(_) => {
                        return crate::p03::P03Dispatch::Unknown { waited_ms }
                            .into_route_result(id, None);
                    }
                },
            };
            if !matches!(&result, Err(error) if error.code.as_str() == EXTERNAL_RESOURCE_BUSY_CODE
                || error.code.as_str() == crate::p03::FILE_RESOURCE_BUSY_CODE)
            {
                return result;
            }
            if let Some(waiting) = &waiting {
                waiting.entered(deadline);
            }
            check_cancelled(id)?;
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return result;
            }
            tokio::time::sleep(remaining.min(Duration::from_millis(20))).await;
            check_cancelled(id)?;
            if Instant::now() >= deadline {
                return result;
            }
            if let Some(waiting) = &waiting {
                let admitted = tokio::select! {
                    admitted = waiting.resume(deadline) => admitted,
                    _ = async {
                        while !aura_fs_tools::resource_wait_cancelled(id) {
                            tokio::time::sleep(Duration::from_millis(20)).await;
                        }
                    } => return Err(crate::p03::cancelled_envelope(id)),
                };
                if !admitted {
                    return result;
                }
            }
        }
    }
}

pub(super) fn check_cancelled(id: CorrelationId) -> Result<(), IpcErrorEnvelope> {
    if aura_fs_tools::resource_wait_cancelled(id) {
        Err(crate::p03::cancelled_envelope(id))
    } else {
        Ok(())
    }
}
