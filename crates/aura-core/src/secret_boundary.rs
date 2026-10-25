//! Secret platform access keeps the original command's bounded settlement owner.
use crate::blocking_pool::BlockingAdmission;
use crate::command_settlement::{SettlementTask, SettlementWaiter, TaskEnding};
use crate::sync_boundary::{SyncSection, SyncSubject};
use crate::*;

pub(crate) async fn run<T, F, V>(
    state: &RuntimeState,
    request: &JsonRpcRequest,
    work: F,
    value: V,
) -> std::result::Result<T, IpcErrorEnvelope>
where
    T: Send + 'static,
    F: FnOnce() -> std::result::Result<T, IpcErrorEnvelope> + Send + 'static,
    V: FnOnce(&T) -> Value + Send + 'static,
{
    let ticket = match request.command_invocation_id {
        Some(command) => Some(
            state
                .settlements
                .begin(command, request.id, &request.method)
                .ok_or_else(|| busy(request.id))?,
        ),
        None => None,
    };
    let waiter = SettlementWaiter::new(state.settlements.clone(), ticket.clone());
    let settlements = state.settlements.clone();
    let worker_ticket = ticket.clone();
    let outcome = state
        .sync
        .run(
            SyncSection::CredentialSecret,
            SyncSubject::new().method(&request.method).request(request.id),
            move || {
                let task = SettlementTask::new(settlements.clone(), worker_ticket.clone());
                let result = work();
                if let Some(ticket) = worker_ticket {
                    let ending = match &result {
                        Ok(result) => TaskEnding::Succeeded(value(result)),
                        Err(error) => TaskEnding::Failed(error.clone()),
                    };
                    settlements.task_finished(&ticket, ending);
                    if result
                        .as_ref()
                        .is_err_and(crate::command_settlement::envelope_is_unconfirmed)
                    {
                        settlements.unresolved(&ticket);
                    }
                }
                task.finished();
                result
            },
        )
        .await;
    match outcome {
        BlockingAdmission::Completed(result) => {
            waiter.received();
            result
        }
        BlockingAdmission::Refused { .. } => {
            if let Some(ticket) = ticket {
                state.settlements.never_ran(&ticket);
            }
            waiter.received();
            Err(busy(request.id))
        }
        BlockingAdmission::Unknown { .. } => {
            Err(crate::command_settlement::outcome_unknown_envelope(
                request.id,
                request.command_invocation_id,
                "凭据操作的结果还没有确认，请刷新查看，不要重新输入密钥。".into(),
            ))
        }
    }
}

fn busy(id: CorrelationId) -> IpcErrorEnvelope {
    IpcErrorEnvelope {
        code: ErrorCode::new("credential.busy").expect("static code"),
        category: ErrorCategory::Unavailable,
        correlation_id: id,
        terminal: IpcRequestTerminal::Failed,
        message: "凭据操作没有开始：同时进行的操作太多，请稍后重试。".into(),
        retryable: false,
        details: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests;
