//! Session writes keep a bounded completion owner across caller timeouts and connection loss.
use super::workspace::{session_envelope, session_unknown_envelope, session_write_failure};
use crate::blocking_pool::BlockingAdmission;
use crate::command_settlement::{SettlementTask, SettlementWaiter, TaskEnding};
use crate::sync_boundary::{SyncSection, SyncSubject};
use crate::*;

pub(super) async fn execute(
    state: &RuntimeState,
    services: &RuntimeServices,
    request: &JsonRpcRequest,
    command: CommandInvocationId,
) -> Result<serde_json::Value, IpcErrorEnvelope> {
    let correlation = request.id;
    let method = request.method.clone();
    let Some(ticket) = state.settlements.begin(command, correlation, &method) else {
        return Err(session_envelope(
            "persistence.busy",
            ErrorCategory::Unavailable,
            "the session write did not run: all completion owners are busy".into(),
            true,
            correlation,
        ));
    };
    let waiter = SettlementWaiter::new(state.settlements.clone(), Some(ticket.clone()));
    let settlements = state.settlements.clone();
    let boundary = state.sync.clone();
    let budget = boundary.budget();
    let persistence = services.persistence.clone();
    // A session write needs the security authority for its permission
    // recheck, so a damaged Security/Aura domain refuses it with the same stable
    // `Unavailable` envelope the route already uses for its own refusals.
    let p04 = services
        .p04()
        .map_err(|_| crate::MissingCapability::P04SecurityAuthority.envelope(correlation))?
        .clone();
    let params = request.params.clone();
    let rename = method == "session.rename";
    let subject = SyncSubject::new().request(correlation).method(&method);
    let (admission_sender, admission_receiver) = tokio::sync::oneshot::channel();
    let admission_sender = Arc::new(Mutex::new(Some(admission_sender)));
    // Spawn before the first wait: aborting the connection drops only `waiter`, never this task.
    // The settlement registry's 64 slots bound all retained receipts and completions.
    let mut completion = tokio::spawn(async move {
        let task = SettlementTask::new(settlements.clone(), Some(ticket.clone()));
        let preparing = persistence.clone();
        let started = Arc::clone(&admission_sender);
        let admitted = boundary
            .run_owned(SyncSection::SessionCommand, subject.clone(), move || {
                if let Some(sender) = started.lock().ok().and_then(|mut slot| slot.take()) {
                    let _ = sender.send(true);
                }
                (|| -> anyhow::Result<aura_persistence::SessionWriteReceipt> {
                    Ok(if rename {
                        preparing.begin_rename_session(
                            command,
                            serde_json::from_value(params)?,
                            correlation,
                        )?
                    } else {
                        preparing.begin_create_session(
                            command,
                            serde_json::from_value(params)?,
                            p04.default_permission_mode()?,
                            correlation,
                        )?
                    })
                })()
                .map_err(|e| session_write_failure(correlation, command, &e))
            })
            .await;
        let result = match admitted {
            BlockingAdmission::Completed(Ok(receipt)) => match receipt.receive().await {
                Ok(committed) => match boundary
                    .run_owned(SyncSection::SessionCommand, subject, move || {
                        persistence
                            .publish_session_write(command, committed)
                            .map_err(anyhow::Error::from)
                            .map_err(|e| publication_failure(correlation, command, &e))
                    })
                    .await
                {
                    BlockingAdmission::Completed(Ok(receipt)) => receipt
                        .receive()
                        .await
                        .map_err(anyhow::Error::from)
                        .and_then(|session| Ok(serde_json::to_value(session)?))
                        .map_err(|e| publication_failure(correlation, command, &e)),
                    BlockingAdmission::Completed(Err(error)) => Err(error),
                    _ => Err(session_unknown_envelope(command, correlation, 0)),
                },
                Err(error) => Err(session_write_failure(correlation, command, &error.into())),
            },
            BlockingAdmission::Completed(Err(error)) => Err(error),
            BlockingAdmission::Refused { waited_ms } => {
                settlements.never_ran(&ticket);
                task.finished();
                if let Some(sender) = admission_sender.lock().ok().and_then(|mut slot| slot.take())
                {
                    let _ = sender.send(false);
                }
                return Err(session_envelope(
                    "persistence.busy",
                    ErrorCategory::Unavailable,
                    format!(
                        "the session write did not run: no worker was free after {waited_ms} ms"
                    ),
                    true,
                    correlation,
                ));
            }
            BlockingAdmission::Unknown { waited_ms } => {
                Err(session_unknown_envelope(command, correlation, waited_ms))
            }
        };
        if let Some(sender) = admission_sender.lock().ok().and_then(|mut slot| slot.take()) {
            let _ = sender.send(false);
        }
        let ending = TaskEnding::from_result(&result);
        settlements.task_finished(&ticket, ending);
        if result.as_ref().is_err_and(crate::command_settlement::envelope_is_unconfirmed) {
            // This owner has received an actual teardown/error, not an expired receipt timer.
            settlements.unresolved(&ticket);
        }
        task.finished();
        result
    });
    // Admission has its own bounded answer. Do not race that refusal with an identically timed
    // business wait: a refused section is provably not run and must keep its Busy answer.
    let answer = if admission_receiver.await == Ok(false) {
        Ok(completion.await)
    } else {
        tokio::time::timeout(budget, &mut completion).await
    };
    match answer {
        Ok(Ok(result)) => {
            waiter.received();
            result
        }
        Ok(Err(_)) => Err(session_unknown_envelope(command, correlation, 0)),
        Err(_) => Err(session_unknown_envelope(
            command,
            correlation,
            u64::try_from(budget.as_millis()).unwrap_or(u64::MAX),
        )),
    }
}

fn publication_failure(
    correlation: CorrelationId,
    command: CommandInvocationId,
    error: &anyhow::Error,
) -> IpcErrorEnvelope {
    crate::command_settlement::outcome_unknown_envelope(
        correlation,
        Some(command),
        format!(
            "the session transaction committed, but its publication could not be settled: {error}; \
             query the original invocation and do not resend the write"
        ),
    )
}
