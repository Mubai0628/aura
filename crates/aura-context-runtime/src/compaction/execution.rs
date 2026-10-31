//! One compaction, attempt after attempt: each prepared by the factory, checked, authorized and
//! sent, until an answer Core can use is committed or the factory asks no further.
use super::*;

pub async fn execute_context_compaction<Factory, Caller, Sink, Facts>(
    operation: ContextCompactionOperationSpec,
    factory: &Factory,
    caller: &Caller,
    sink: &Sink,
    authorization_facts: &Facts,
) -> ContextCompactionExecutionOutcome
where
    Factory: CompactionAttemptFactory,
    Caller: CompactionModelCaller,
    Sink: CompactionLifecycleSink,
    Facts: crate::ModelEgressAuthorizationFacts,
{
    if let Err(failure) = validate_compaction_operation(&operation) {
        return finish_without_attempt(operation, sink, failure, 0).await;
    }
    let mut context_snapshot_ids = BTreeSet::new();
    let mut model_step_ids = BTreeSet::new();
    for attempt_ordinal in 1..=MAX_COMPACTION_ATTEMPTS {
        let attempt = match factory.prepare_attempt(&operation, attempt_ordinal).await {
            Ok(attempt) => attempt,
            Err(failure) => {
                return finish_without_attempt(
                    operation,
                    sink,
                    failure,
                    attempt_ordinal.saturating_sub(1),
                )
                .await;
            }
        };
        if let Err(failure) = validate_prepared_compaction_attempt(
            &operation,
            &attempt,
            attempt_ordinal,
            &mut context_snapshot_ids,
            &mut model_step_ids,
        ) {
            return finish_without_attempt(
                operation,
                sink,
                failure,
                attempt_ordinal.saturating_sub(1),
            )
            .await;
        }
        if let Err(error) = crate::ModelEgressAuthorizer::new(authorization_facts)
            .validate(operation.session_id, operation.model_profile_id, &attempt.egress_manifest)
            .await
        {
            return finish_without_attempt(
                operation,
                sink,
                CompactionExecutionFailure {
                    code: error.to_string(),
                    retryable: false,
                    side_effect_unknown: false,
                },
                attempt_ordinal.saturating_sub(1),
            )
            .await;
        }
        if let Err(error_code) = sink.mark_attempt_running(&operation, &attempt).await {
            return ContextCompactionExecutionOutcome::PersistenceInterrupted { error_code };
        }

        match caller.call(&attempt).await {
            CompactionModelCallTerminal::Completed { output, transport_attempt_id } => {
                match answer::usable_answer(&attempt, output, operation.output_character_limit) {
                    Ok(parsed) => {
                        let completed = ValidatedCompactionCommit {
                            operation,
                            attempt,
                            transport_attempt_id,
                            parsed,
                        };
                        if let Err(error_code) = sink.commit_completed(&completed).await {
                            return ContextCompactionExecutionOutcome::PersistenceInterrupted {
                                error_code,
                            };
                        }
                        return ContextCompactionExecutionOutcome::Completed {
                            attempt_ordinal,
                            transport_attempt_id,
                        };
                    }
                    Err((failure, why)) => {
                        let again = attempt_ordinal < MAX_COMPACTION_ATTEMPTS
                            && factory.asks_again(
                                &attempt,
                                FailedCompactionAttempt::UnusableAnswer { why },
                            );
                        let commit = CompactionAttemptFailureCommit {
                            operation: operation.clone(),
                            attempt: Some(attempt),
                            state: CompactionModelAttemptState::Failed,
                            transport_attempt_id: Some(transport_attempt_id),
                            error_code: failure.code.clone(),
                        };
                        if again {
                            if let Err(error_code) = sink.finish_retryable_attempt(&commit).await {
                                return ContextCompactionExecutionOutcome::PersistenceInterrupted {
                                    error_code,
                                };
                            }
                            continue;
                        }
                        return finish_with_attempt(commit, sink, failure, attempt_ordinal).await;
                    }
                }
            }
            CompactionModelCallTerminal::Failed { error_code, transport_attempt_id, retryable } => {
                let failure = CompactionExecutionFailure {
                    code: error_code.clone(),
                    retryable,
                    side_effect_unknown: false,
                };
                let again = attempt_ordinal < MAX_COMPACTION_ATTEMPTS
                    && factory
                        .asks_again(&attempt, FailedCompactionAttempt::NoAnswer { retryable });
                let commit = CompactionAttemptFailureCommit {
                    operation: operation.clone(),
                    attempt: Some(attempt),
                    state: CompactionModelAttemptState::Failed,
                    transport_attempt_id,
                    error_code,
                };
                if again {
                    if let Err(error_code) = sink.finish_retryable_attempt(&commit).await {
                        return ContextCompactionExecutionOutcome::PersistenceInterrupted {
                            error_code,
                        };
                    }
                    continue;
                }
                return finish_with_attempt(commit, sink, failure, attempt_ordinal).await;
            }
            CompactionModelCallTerminal::Cancelled { error_code, transport_attempt_id } => {
                let commit = CompactionAttemptFailureCommit {
                    operation,
                    attempt: Some(attempt),
                    state: CompactionModelAttemptState::Cancelled,
                    transport_attempt_id,
                    error_code: error_code.clone(),
                };
                if let Err(error_code) = sink.finish_operation(&commit).await {
                    return ContextCompactionExecutionOutcome::PersistenceInterrupted {
                        error_code,
                    };
                }
                return ContextCompactionExecutionOutcome::Cancelled {
                    error_code,
                    completed_attempts: attempt_ordinal,
                };
            }
            CompactionModelCallTerminal::UnknownSideEffect { error_code, transport_attempt_id } => {
                let failure = CompactionExecutionFailure {
                    code: error_code.clone(),
                    retryable: false,
                    side_effect_unknown: true,
                };
                let commit = CompactionAttemptFailureCommit {
                    operation,
                    attempt: Some(attempt),
                    state: CompactionModelAttemptState::UnknownSideEffect,
                    transport_attempt_id: Some(transport_attempt_id),
                    error_code,
                };
                return finish_with_attempt(commit, sink, failure, attempt_ordinal).await;
            }
        }
    }
    ContextCompactionExecutionOutcome::Failed {
        failure: CompactionExecutionFailure {
            code: "context.compaction.retry_exhausted".to_owned(),
            retryable: false,
            side_effect_unknown: false,
        },
        completed_attempts: MAX_COMPACTION_ATTEMPTS,
    }
}

fn validate_compaction_operation(
    operation: &ContextCompactionOperationSpec,
) -> Result<(), CompactionExecutionFailure> {
    let source_refs = operation
        .anchor_manifest
        .anchors
        .iter()
        .map(|anchor| anchor.source_ref.clone())
        .collect::<Vec<_>>();
    if operation.sequence == 0
        || operation.output_character_limit == 0
        || operation.source_span.start_sequence > operation.source_span.end_sequence
        || operation.source_span.source_refs != source_refs
        || canonical_json_v1_sha256(&operation.anchor_manifest.anchors)
            .map_or(true, |hash| hash != operation.anchor_manifest.manifest_hash)
    {
        return Err(CompactionExecutionFailure {
            code: "context.compaction.operation_invalid".to_owned(),
            retryable: false,
            side_effect_unknown: false,
        });
    }
    Ok(())
}

async fn finish_without_attempt<Sink: CompactionLifecycleSink>(
    operation: ContextCompactionOperationSpec,
    sink: &Sink,
    failure: CompactionExecutionFailure,
    completed_attempts: u8,
) -> ContextCompactionExecutionOutcome {
    let commit = CompactionAttemptFailureCommit {
        operation,
        attempt: None,
        state: CompactionModelAttemptState::Failed,
        transport_attempt_id: None,
        error_code: failure.code.clone(),
    };
    if let Err(error_code) = sink.finish_operation(&commit).await {
        return ContextCompactionExecutionOutcome::PersistenceInterrupted { error_code };
    }
    ContextCompactionExecutionOutcome::Failed { failure, completed_attempts }
}

async fn finish_with_attempt<Sink: CompactionLifecycleSink>(
    commit: CompactionAttemptFailureCommit,
    sink: &Sink,
    failure: CompactionExecutionFailure,
    completed_attempts: u8,
) -> ContextCompactionExecutionOutcome {
    if let Err(error_code) = sink.finish_operation(&commit).await {
        return ContextCompactionExecutionOutcome::PersistenceInterrupted { error_code };
    }
    ContextCompactionExecutionOutcome::Failed { failure, completed_attempts }
}
