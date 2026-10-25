//! Execution facts for the shell publisher.

/// The managed background's own activity record, published by the real executor.
///
/// It is derived from the background attempt the Shell backend actually observed, and it
/// carries **no Agent run**: the managed process is not the Agent's call, and its lifetime is
/// not bounded by that call returning.
///
/// R2: ONE REAL BACKGROUND INSTANCE IS ONE ACTIVITY IDENTITY.
/// The generation is deliberately **not** `update.sequence`. The sequence is the observation
/// ordering of one managed process - a dedup and out-of-order key - not a new activity. Treating
/// it as a generation minted a fresh `ActivityId` per update, so `Running(1)`, `Running(2)`,
/// `Unknown(3)`, `Exited(4)` were four different activities and the terminal update ended only the
/// last one, leaving the first three advertised as `running` forever. The generation is now derived
/// from the process identity that really distinguishes one managed instance from another, so every
/// observation of one instance publishes and ends the *same* row.
#[must_use]
pub(crate) fn shell_background_record(
    request: &aura_execution_contract::DispatchRequest,
    update: &aura_windows_user_contract::RunnerBackgroundUpdate,
) -> aura_activity_domain::ActivityRecord {
    use aura_activity_domain::*;
    let identity = &request.operation.content.identity;
    let execution = ActivityExecutionRef::ShellExecution {
        operation_id: update.attempt.operation_id,
        attempt_ordinal: update.attempt.ordinal.get(),
    };
    let generation = background_generation(update.attempt.operation_id);
    let id = ActivityId::derive(ActivityKind::ShellManagedBackground, &execution, generation);
    ActivityRecord {
        tool_name: request.operation.content.action_kind.clone(),
        private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
        id,
        kind: ActivityKind::ShellManagedBackground,
        execution,
        generation,
        tool_id: aura_domain_common::ToolId::from_stable_name(
            &request.operation.content.action_kind,
        ),
        tool_group: ActivityToolGroup::Shell,
        owner: ActivityOwnerScope {
            session_id: identity.session_id.unwrap_or_default(),
            workspace_id: identity.workspace_id.unwrap_or_default(),
            // Deliberately `None`: this owner has no Agent identity, and nothing fills
            // one in on its behalf.
            agent_run_id: None,
            turn_task_id: None,
            model_step_id: None,
            runtime_instance_id: request.plan.prepared.backend.runtime_instance_id,
        },
        resources: vec![ActivityResource {
            target: ActivityResourceTarget::Unproven {
                hint: format!("managed background of {}", request.operation.content.action_kind),
            },
            relation: ActivityResourceRelation::UnknownWriteSet,
            display_name: "managed background".to_owned(),
            display_name_truncated: false,
        }],
        parameters: vec![ActivityParameter {
            key: "invocation".to_owned(),
            value: update.attempt.operation_id.to_string(),
            truncated: false,
        }],
        started_at: update.observed_at,
        runtime_instance_id: request.plan.prepared.backend.runtime_instance_id,
        published_at_revision: aura_domain_common::Revision::INITIAL,
    }
}

/// The stable generation of one real managed background instance (R2).
///
/// It is a deterministic fold of the background's own operation identity - the fact that separates
/// one instance from another - so it is stable across every observation of that instance and
/// distinct between instances. It is never the observation sequence.
pub(super) fn background_generation(operation_id: aura_domain_common::SecurityOperationId) -> u32 {
    let digest = aura_domain_common::jcs::canonical_sha256(&(
        "aura.activity.shell_background_generation.v1",
        operation_id.to_string(),
    ));
    digest
        .map(|hash| u32::from_str_radix(hash.hex().get(..8).unwrap_or("0"), 16).unwrap_or(1).max(1))
        .unwrap_or(1)
}

pub(crate) fn shell_background_identity(
    attempt: &aura_execution_contract::AttemptRef,
) -> (aura_activity_domain::ActivityId, u32) {
    let generation = background_generation(attempt.operation_id);
    let execution = aura_activity_domain::ActivityExecutionRef::ShellExecution {
        operation_id: attempt.operation_id,
        attempt_ordinal: attempt.ordinal.get(),
    };
    (
        aura_activity_domain::ActivityId::derive(
            aura_activity_domain::ActivityKind::ShellManagedBackground,
            &execution,
            generation,
        ),
        generation,
    )
}

/// Whether a background observation is for the same real instance, and whether it is new.
///
/// The sequence decides only *ordering*: a repeated or older observation of the same instance is
/// not a new activity, and a late observation that arrives after a terminal one must not resurrect
/// a process that really stopped. This is the dedup/out-of-order rule R2 asks for, expressed as a
/// value so it is testable without a host.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BackgroundObservationDisposition {
    /// First time this instance is seen: publish the row.
    Publish,
    /// A later observation of the same live instance: refresh nothing, mint no new identity.
    Duplicate,
    /// An observation older than one already applied: ignore it rather than moving state back.
    Stale,
    /// The instance has already reached a terminal observation; a late `Running` must not
    /// resurrect it.
    AfterTerminal,
}

/// Classifies one background observation against what has already been applied (R2).
///
/// `last_sequence` is the highest sequence already applied for this instance, and `terminated` says
/// whether a terminal observation has already been seen.
#[must_use]
pub(crate) fn background_observation_disposition(
    sequence: u64,
    last_sequence: Option<u64>,
    terminated: bool,
) -> BackgroundObservationDisposition {
    match last_sequence {
        None => BackgroundObservationDisposition::Publish,
        Some(_) if terminated => BackgroundObservationDisposition::AfterTerminal,
        Some(applied) if sequence > applied => BackgroundObservationDisposition::Publish,
        Some(applied) if sequence == applied => BackgroundObservationDisposition::Duplicate,
        Some(_) => BackgroundObservationDisposition::Stale,
    }
}

/// Whether a background observation means the managed process has really stopped.
///
/// `Unknown` is deliberately **not** treated as stopped: an observation the executor could
/// not classify must not remove a live activity, because removing it would present the
/// process as finished when its termination is exactly what is not known.
#[must_use]
pub(crate) fn background_observation_ends(
    state: aura_windows_user_contract::BackgroundState,
) -> bool {
    matches!(
        state,
        aura_windows_user_contract::BackgroundState::Exited
            | aura_windows_user_contract::BackgroundState::Interrupted
    )
}
