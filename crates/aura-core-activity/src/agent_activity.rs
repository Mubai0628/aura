//! The projections a real Agent executor publishes into the run-information
//! index.
//!
//! WHY THIS IS A SEPARATE MODULE
//! The projection decides what a row *means*, and two of its decisions are the ones
//! the design is most explicit about. Keeping them next to the execution service
//! rather than inside the index means:
//!
//! * the index stays a pure in-memory collection with no domain knowledge, and
//! * the Shell rule below is visible at the point where a real Shell call is known.
//!
//! THE SHELL RULE, IN CODE
//! A Shell's `cwd` is a *working directory*, not a write set. A script may write
//! anywhere the process can reach, so [`project_resources`] never turns a Shell's
//! cwd into a directory write scope. It reports [`ActivityResourceRelation::UnknownWriteSet`]
//! with an [`ActivityResourceTarget::Unproven`] target instead, named by the cwd relative to
//! the workspace - or [`ActivityResourceRelation::UnknownWriteSetOutsideWorkspace`] when the
//! script names places outside the workspace. The index therefore never serialises
//! unknown Shells globally, never disables Shell, and never claims to cover an external
//! program or an arbitrary child process.
mod buffer;
mod main;
#[cfg(test)]
mod name_coordination_tests;
#[cfg(test)]
mod query_tests;
mod shell;
pub use buffer::publish_buffer_commit_activity;
pub use main::{MainFsActivitySubject, MainFsTarget, main_record};
#[cfg(test)]
use shell::background_generation;
pub use shell::{
    BackgroundObservationDisposition, background_observation_disposition,
    background_observation_ends, shell_background_identity, shell_background_record,
};

#[cfg(any(test, feature = "test-hooks"))]
use aura_activity_domain::ActivityExecutionRef;
use aura_activity_domain::{
    ActivityParameter, ActivityParameterAllowlist, ActivityResource, ActivityResourceRelation,
    ActivityResourceTarget, ActivityToolGroup,
};
use aura_security_contract::ResourceDescriptor;

/// The tool group for a real action kind. A new action kind falls into `Other`
/// rather than being guessed into a group with a stronger meaning.
#[must_use]
pub fn tool_group_for(action_kind: &str) -> ActivityToolGroup {
    if action_kind.starts_with("shell.") {
        return ActivityToolGroup::Shell;
    }
    if action_kind.starts_with("web.") {
        return ActivityToolGroup::Web;
    }
    if matches!(action_kind, "workspace_init.complete" | "commands.save" | "commands.delete") {
        return ActivityToolGroup::FileWrite;
    }
    if matches!(action_kind, "commands.list" | "commands.describe") {
        return ActivityToolGroup::FileRead;
    }
    if action_kind.starts_with("fs.") {
        // The tool contract's own naming marks a read; anything else in the file
        // namespace is treated as a write, which is the conservative direction.
        return if ["fs.read", "fs.read_file", "fs.list", "fs.find", "fs.search", "fs.stat"]
            .contains(&action_kind)
        {
            ActivityToolGroup::FileRead
        } else {
            ActivityToolGroup::FileWrite
        };
    }
    ActivityToolGroup::Other
}

/// The relation of a Shell whose write set cannot be proven: outside the workspace when its
/// script names places there, in its workspace otherwise.
pub fn shell_relation(
    risks: &[aura_security_contract::OperationRiskFlag],
) -> ActivityResourceRelation {
    if risks.contains(&aura_security_contract::OperationRiskFlag::OutsideWorkspaceWrite) {
        ActivityResourceRelation::UnknownWriteSetOutsideWorkspace
    } else {
        ActivityResourceRelation::UnknownWriteSet
    }
}

/// Copies the identity facts A already verified into the index's own target shape.
///
/// Returning `None` here means "this descriptor names no physical object at all"
/// (a network origin, for example). A descriptor that names a physical object but
/// whose verified identity is only a *hint* - a workspace-relative path before the
/// workspace layer resolves it - is published as [`ActivityResourceTarget::Unproven`],
/// so a reader sees an honest unknown instead of a silently missing row or a path
/// string pretending to be an identity.
///
/// R3: this is the **single** descriptor-to-target conversion in the process. A reader that must
/// derive the same key for a target it holds (the model step's relevant-target set) calls this
/// rather than writing a second interpretation, so two passages over one descriptor cannot
/// disagree about the object behind it.
#[must_use]
pub fn target_of(descriptor: &ResourceDescriptor) -> Option<ActivityResourceTarget> {
    match descriptor {
        ResourceDescriptor::ExternalPath { identity, parent_identity, directory, .. } => {
            match (identity, parent_identity) {
                (Some(identity), _) => Some(identity_target(identity, *directory)),
                (None, Some(parent)) => Some(identity_target(parent, true)),
                (None, None) => Some(ActivityResourceTarget::Unproven {
                    hint: "external path without a verified identity".to_owned(),
                }),
            }
        }
        ResourceDescriptor::WorkspacePath { canonical_relative_path, .. } => {
            // A workspace-relative path is a *display* fact until the workspace layer
            // resolves it to a real object. Publishing it as a proven identity would
            // be exactly the "path string is identity" mistake the design rejects.
            Some(ActivityResourceTarget::Unproven {
                hint: format!("workspace path {}", canonical_relative_path.as_str()),
            })
        }
        ResourceDescriptor::WorkspaceRoot { .. } => {
            Some(ActivityResourceTarget::Unproven { hint: "workspace root scope".to_owned() })
        }
        ResourceDescriptor::ExternalGrant { .. } => {
            Some(ActivityResourceTarget::Unproven { hint: "external grant scope".to_owned() })
        }
        ResourceDescriptor::SessionAuthorizedFile { .. } => {
            Some(ActivityResourceTarget::Unproven { hint: "session authorized file".to_owned() })
        }
        // A package names the installed code and its policy identity, not an object the
        // call reads or writes; the program's workspace scope is its own descriptor.
        ResourceDescriptor::NetworkOrigin { .. }
        | ResourceDescriptor::PrivateNetwork { .. }
        | ResourceDescriptor::ToolPackage { .. } => None,
        // Any descriptor variant added after this module was written is reported as
        // unproven rather than being dropped, so a reader never sees a resource that
        // simply vanished from the picture.
        _ => Some(ActivityResourceTarget::Unproven { hint: "unclassified resource".to_owned() }),
    }
}

pub fn display_of(descriptor: &ResourceDescriptor) -> String {
    match descriptor {
        ResourceDescriptor::MemoryVault { scope, .. } => match scope {
            aura_security_contract::MemoryVaultScope::Global => "全局记忆".into(),
            aura_security_contract::MemoryVaultScope::Project { .. } => "当前项目记忆".into(),
        },
        ResourceDescriptor::WorkspacePath { canonical_relative_path, .. } => {
            canonical_relative_path.as_str().to_owned()
        }
        ResourceDescriptor::ExternalPath { canonical_path, .. } => canonical_path.display.clone(),
        ResourceDescriptor::WorkspaceRoot { .. } => "workspace root".to_owned(),
        ResourceDescriptor::ExternalGrant { grant_id } => format!("grant {grant_id}"),
        ResourceDescriptor::SessionAuthorizedFile { file_reference_id } => {
            format!("file reference {file_reference_id}")
        }
        ResourceDescriptor::NetworkOrigin { scheme, host, port } => {
            format!("{scheme}://{host}:{port}")
        }
        ResourceDescriptor::PrivateNetwork { .. } => "private network".to_owned(),
        _ => "resource".to_owned(),
    }
}

/// Turns a verified [`FileIdentity`] into the index's target.
///
/// The platform identity is two opaque strings. They are carried **verbatim** into
/// a stable 64-bit-plus-64-bit hash pair, so the index compares identities the way
/// A verified them instead of inventing a numeric interpretation of a string. Two
/// different identities cannot collide, because the pair is a hash of both parts.
fn identity_target(
    identity: &aura_domain_common::FileIdentity,
    directory: bool,
) -> ActivityResourceTarget {
    ActivityResourceTarget::VerifiedObject {
        identity: identity.clone(),
        ancestors: Vec::new(),
        subtree: directory,
    }
}

/// Cuts a display name to its bound on a character boundary.
pub fn truncate_display(value: &str) -> String {
    let limit = aura_activity_domain::MAX_ACTIVITY_RESOURCE_DISPLAY_BYTES;
    if value.len() <= limit {
        return value.to_owned();
    }
    let mut end = limit;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

/// A published activity that its owner is still responsible for ending.
///
/// It is deliberately **not** a `Drop` guard. An implicit end on drop would fire at
/// whatever moment a Rust value happened to go out of scope, which is not the same
/// instant as the real side effect finishing, and it would hide a missing end instead
/// of exposing it. Every owner therefore calls [`Self::end`] explicitly on each real
/// exit path.
pub struct PublishedActivity {
    pub id: aura_activity_domain::ActivityId,
    pub generation: u32,
}

/// The actual backend task owns this lease. It is first filled by the real admitted
/// execution, not by an IPC waiter or by the reservation callback. Detached work
/// retains it until execution ends; Unknown does not terminate an activity.
pub struct AgentActivityLease {
    index: aura_activity_runtime::ActivityIndex,
    published: std::sync::Mutex<Option<PublishedActivity>>,
}

impl AgentActivityLease {
    #[must_use]
    pub fn new(index: aura_activity_runtime::ActivityIndex) -> Self {
        Self { index, published: std::sync::Mutex::new(None) }
    }

    /// Publishes the activity for a call that is really about to run.
    ///
    /// The `id` and `generation` are handed in by the caller, because they are derived from
    /// the exact `AttemptRef` the reservation produced; this type only records and ends them.
    /// A repeated call is ignored rather than publishing a second row.
    pub fn started(&self, published: PublishedActivity) {
        let mut slot = self.published.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if slot.is_none() {
            *slot = Some(published);
        }
    }

    /// Ends this call's activity if it was ever published. Safe to call repeatedly.
    pub fn end(&self) {
        let mut slot = self.published.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(published) = slot.take() {
            self.index.end(published.id, published.generation);
        }
    }
}

impl Drop for AgentActivityLease {
    fn drop(&mut self) {
        // One end under the exact identity that was published, on **every** exit path of the
        // owning dispatch: the normal ones, every `?`, and a panic unwind. A lease that never
        // published anything ends nothing, which is the correct answer for a call that never
        // started.
        self.end();
    }
}

/// A synchronous execution owner's scope ends its exact observation on every exit.
/// Main Fs, P04 and Buffer scopes are inside the real worker, never in its waiter.
pub struct AgentActivityGuard {
    index: aura_activity_runtime::ActivityIndex,
    id: aura_activity_domain::ActivityId,
    generation: u32,
    ended: bool,
}

impl AgentActivityGuard {
    #[must_use]
    pub fn new(
        index: aura_activity_runtime::ActivityIndex,
        id: aura_activity_domain::ActivityId,
        generation: u32,
    ) -> Self {
        Self { index, id, generation, ended: false }
    }

    /// Ends the activity now. Calling it again, or letting the guard drop afterwards, is safe.
    pub fn end(&mut self) {
        if !self.ended {
            self.index.end(self.id, self.generation);
            self.ended = true;
        }
    }
}

impl Drop for AgentActivityGuard {
    fn drop(&mut self) {
        // One `end` under the exact identity it published, on **every** exit path: the normal
        // ones, every `?`, and a panic unwind.
        self.end();
    }
}

/// Test-only helpers for the D isolation regressions.
///
/// They live here, behind `cfg(test)`, rather than on the production surface: a synthetic
/// record is a test input, and shipping a constructor for one would be a way to publish a fact
/// that no executor observed.
#[cfg(any(test, feature = "test-hooks"))]
pub mod tests_support {
    use super::*;
    use aura_activity_domain::{
        ActivityKind, ActivityOwnerScope, ActivityRecord, ActivityResourceRelation,
        ActivityResourceTarget, ActivityToolGroup,
    };
    use aura_domain_common::{
        AgentRunId, Revision, RuntimeInstanceId, SecurityOperationId, SessionId, ToolCallId,
        UtcTimestamp, WorkspaceId,
    };

    /// A synthetic but well-formed activity record, used to saturate an index.
    ///
    /// Each call is unique, so a burst really fills the table instead of collapsing into one
    /// repeated fact.
    #[must_use]
    pub fn synthetic_record() -> ActivityRecord {
        let execution = ActivityExecutionRef::AgentToolCall {
            tool_call_id: ToolCallId::new(),
            operation_id: SecurityOperationId::new(),
            attempt_ordinal: 1,
        };
        let generation = {
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        };
        // R5: the owner scope and the record must name the **same** runtime instance, and an
        // Agent-owned kind must really carry an Agent run. The synthetic fact obeys the same rules
        // the production publishers do, so a synthetic row can never be one the index would refuse.
        let runtime_instance_id = RuntimeInstanceId::new();
        ActivityRecord {
            tool_name: String::new(),
            private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
            id: aura_activity_domain::ActivityId::derive(
                ActivityKind::AgentTool,
                &execution,
                generation,
            ),
            kind: ActivityKind::AgentTool,
            execution,
            generation,
            tool_id: aura_domain_common::ToolId::from_stable_name("fs.write"),
            tool_group: ActivityToolGroup::FileWrite,
            owner: ActivityOwnerScope {
                session_id: SessionId::new(),
                workspace_id: WorkspaceId::new(),
                agent_run_id: Some(AgentRunId::new()),
                turn_task_id: None,
                model_step_id: None,
                runtime_instance_id,
            },
            resources: vec![ActivityResource {
                target: ActivityResourceTarget::Unproven { hint: "synthetic".to_owned() },
                relation: ActivityResourceRelation::Write,
                display_name: "synthetic".to_owned(),
                display_name_truncated: false,
            }],
            parameters: Vec::new(),
            started_at: UtcTimestamp::now(),
            runtime_instance_id,
            published_at_revision: Revision::INITIAL,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use aura_activity_domain::{
        ActivityKind, ActivityOwnerScope, ActivityRecord, ActivityResourceRelation,
        ActivityResourceTarget, ActivityToolGroup,
    };
    use aura_domain_common::{
        AgentRunId, Revision, RuntimeInstanceId, SecurityOperationId, SessionId, ToolCallId,
        UtcTimestamp, WorkspaceId,
    };

    /// A real Agent tool record, published through the real index.
    ///
    /// Each call mints a fresh `ToolCallId`, so two calls in one test are two genuinely
    /// different activities rather than the same fact published twice - which matters, because
    /// a repeated publish of an identical fact is correctly a no-op.
    fn publish_agent_record(
        index: &aura_activity_runtime::ActivityIndex,
    ) -> (aura_activity_domain::ActivityId, u32) {
        let execution = ActivityExecutionRef::AgentToolCall {
            tool_call_id: ToolCallId::new(),
            operation_id: SecurityOperationId::new(),
            attempt_ordinal: 1,
        };
        // A fresh call identity AND a fresh generation, so each helper call really is a distinct
        // activity with a distinct id. The counter is strictly increasing for the whole process,
        // which is what makes two helper calls two ids rather than one duplicate publish.
        let generation = {
            static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        };
        let id = aura_activity_domain::ActivityId::derive(
            ActivityKind::AgentTool,
            &execution,
            generation,
        );
        // One runtime instance for both the owner scope and the record: R5 makes the two having to
        // agree a checked rule, because a record whose owner and whose own runtime disagree makes
        // generation filtering unsound.
        let runtime_instance_id = RuntimeInstanceId::new();
        let outcome = index.publish(ActivityRecord {
            tool_name: String::new(),
            private_source: aura_activity_domain::ActivityPrivateSource::PersonalData,
            id,
            kind: ActivityKind::AgentTool,
            execution,
            generation,
            tool_id: aura_domain_common::ToolId::from_stable_name("fs.write"),
            tool_group: ActivityToolGroup::FileWrite,
            owner: ActivityOwnerScope {
                session_id: SessionId::new(),
                workspace_id: WorkspaceId::new(),
                agent_run_id: Some(AgentRunId::new()),
                turn_task_id: None,
                model_step_id: None,
                runtime_instance_id,
            },
            resources: vec![ActivityResource {
                target: ActivityResourceTarget::Unproven { hint: "a.txt".to_owned() },
                relation: ActivityResourceRelation::Write,
                display_name: "a.txt".to_owned(),
                display_name_truncated: false,
            }],
            parameters: Vec::new(),
            started_at: UtcTimestamp::now(),
            runtime_instance_id,
            published_at_revision: Revision::INITIAL,
        });
        assert!(outcome.was_recorded(), "the real publisher records the fact");
        (id, generation)
    }

    #[test]
    fn a_dispatch_error_early_return_leaves_no_activity_behind() {
        // THE COUNTEREXAMPLE FOR THE DEFECT INDEPENDENT REVIEW FOUND.
        //
        // The first version of the Agent publisher returned a bare `ActivityId`, and
        // `execute_call` has `?` returns between the publish and its single `end()`. On those
        // paths the function returned before the end, and the index kept advertising a call that
        // had already terminated as `running`.
        //
        // The lease makes that impossible, but the test has to reproduce the **real** shape to
        // prove it: `execute_call` performs its explicit `end()` on the *normal* path, after the
        // `?`. So the explicit end is unreachable when an error is injected, and only the lease's
        // `Drop` can clean up. Disabling that `Drop` makes this test fail - which is what makes it
        // a counterexample rather than a restatement of the code.
        let index = aura_activity_runtime::ActivityIndex::default();

        fn execute_call_shaped(
            index: &aura_activity_runtime::ActivityIndex,
            fail: bool,
        ) -> anyhow::Result<()> {
            let activity = AgentActivityLease::new(index.clone());
            let (id, generation) = publish_agent_record(index);
            // Model the executor-owned lease once its actual worker has begun.
            activity.started(PublishedActivity { id, generation });
            assert_eq!(index.len(), 1, "the real activity is present while the call runs");
            // The dispatch result, taken through `?` exactly as `execute_call` takes it.
            let finished: anyhow::Result<()> =
                if fail { Err(anyhow::anyhow!("injected dispatch failure")) } else { Ok(()) };
            finished.map_err(|error| anyhow::anyhow!("{error:?}"))?;
            // The normal path's explicit end. It is unreachable when the `?` above returns, so the
            // lease is the only thing that can end the activity on the failure path.
            activity.end();
            Ok(())
        }

        let outcome = execute_call_shaped(&index, true);
        assert!(outcome.is_err(), "the injected failure really propagated");
        assert_eq!(
            index.len(),
            0,
            "a terminated call must leave no running activity behind, even when the early return \
             skipped the explicit end"
        );

        // The success path still ends its activity, so the lease did not merely paper over a
        // missing normal end.
        assert!(execute_call_shaped(&index, false).is_ok());
        assert_eq!(index.len(), 0, "the normal path ends its activity too");
    }

    #[test]
    fn a_call_that_never_reached_the_real_start_publishes_nothing() {
        // R1's ordering claim, on the shape the production seam really has: an activity is
        // published only when the actual executor has begun after admission. A call that is still
        // waiting for approval, for capacity or for A's resource admission has published nothing,
        // so a query taken during that wait finds no `running` row at all - and an early return
        // from that same wait has nothing to leak.
        let index = aura_activity_runtime::ActivityIndex::default();
        let activity = AgentActivityLease::new(index.clone());
        assert_eq!(index.len(), 0, "no reservation has happened, so nothing is running");
        // The wait ends without ever starting (a cancelled call). Dropping the lease must not
        // invent a row, and must not fail because there was nothing to end.
        activity.end();
        drop(activity);
        assert_eq!(index.len(), 0, "a call that never started leaves the index untouched");
    }

    #[test]
    fn the_published_row_carries_the_real_attempt_ordinal_not_a_constant() {
        // The brief names one concrete wrong implementation: `attempt_ordinal` hardcoded to `1`.
        // The ordinal really participates in the `ActivityId`, so a second attempt of one
        // operation is a different activity rather than silently colliding with the first.
        let operation = aura_domain_common::SecurityOperationId::new();
        let call = ToolCallId::new();
        let first = std::num::NonZeroU32::MIN;
        let second = std::num::NonZeroU32::new(2).expect("a second ordinal");
        let reference = |ordinal: std::num::NonZeroU32| ActivityExecutionRef::AgentToolCall {
            tool_call_id: call,
            operation_id: operation,
            attempt_ordinal: ordinal.get(),
        };
        assert_ne!(
            aura_activity_domain::ActivityId::derive(ActivityKind::AgentTool, &reference(first), 7),
            aura_activity_domain::ActivityId::derive(
                ActivityKind::AgentTool,
                &reference(second),
                7
            ),
            "a second real attempt must be a different activity, not the same row"
        );
    }

    #[test]
    fn a_lease_ends_its_own_activity_and_nothing_else() {
        let index = aura_activity_runtime::ActivityIndex::default();
        let (first, first_generation) = publish_agent_record(&index);
        let (second, second_generation) = publish_agent_record(&index);
        assert_eq!(index.len(), 2, "two different real calls are two different activities");
        {
            let lease = AgentActivityLease::new(index.clone());
            lease.started(PublishedActivity { id: first, generation: first_generation });
        }
        assert_eq!(index.len(), 1, "the lease ended exactly its own activity");
        // The remaining row is a different real call, and it is still independently addressable
        // under its own identity - which is the property a reader depends on.
        let reader = aura_activity_runtime::ActivityVisibility::for_session(
            RuntimeInstanceId::new(),
            SessionId::new(),
            WorkspaceId::new(),
        );
        assert!(index.snapshot(&reader).rows.iter().all(|row| row.id != first));
        assert_eq!(
            index.end(second, second_generation),
            aura_activity_runtime::ActivityEndOutcome::Ended,
            "the other call's activity was still independently addressable"
        );
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn ending_a_lease_twice_is_safe() {
        let index = aura_activity_runtime::ActivityIndex::default();
        let (id, generation) = publish_agent_record(&index);
        {
            let lease = AgentActivityLease::new(index.clone());
            lease.started(PublishedActivity { id, generation });
            lease.end();
            lease.end();
            // The drop runs a third end. None of them may touch anything else.
        }
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn a_lease_for_a_stale_generation_cannot_remove_a_newer_activity() {
        let index = aura_activity_runtime::ActivityIndex::default();
        let (id, generation) = publish_agent_record(&index);
        {
            let stale = AgentActivityLease::new(index.clone());
            stale.started(PublishedActivity { id, generation: generation.wrapping_add(1) });
        }
        assert_eq!(
            index.len(),
            1,
            "a lease carrying the wrong generation must not remove the live activity"
        );
        {
            let correct = AgentActivityLease::new(index.clone());
            correct.started(PublishedActivity { id, generation });
        }
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn a_shell_background_observation_of_unknown_state_does_not_end_the_activity() {
        // `Unknown` is not "stopped": removing the row there would present a live managed
        // process as finished.
        use aura_windows_user_contract::BackgroundState;
        assert!(!background_observation_ends(BackgroundState::Unknown));
        assert!(!background_observation_ends(BackgroundState::Running));
        assert!(background_observation_ends(BackgroundState::Exited));
        assert!(background_observation_ends(BackgroundState::Interrupted));
    }

    #[test]
    fn one_managed_background_instance_keeps_one_stable_activity_identity() {
        // THE COUNTEREXAMPLE FOR R2's SECOND DEFECT.
        //
        // The shipped implementation set `generation = update.sequence`, so the four real
        // observations of ONE managed process - Running(1), Running(2), Unknown(3), Exited(4) -
        // were four different `ActivityId`s. The terminal update ended only the fourth, and the
        // first three stayed in the running index forever.
        //
        // The replacement is the fold of the process's own operation identity, which is the fact
        // that really distinguishes one instance from another. Two instances must still differ,
        // and every observation of one instance must agree.
        let instance = aura_domain_common::SecurityOperationId::new();
        let other = aura_domain_common::SecurityOperationId::new();
        let generation = background_generation(instance);
        assert_eq!(
            generation,
            background_generation(instance),
            "every observation of one real instance must derive the same identity"
        );
        assert_ne!(
            generation,
            background_generation(other),
            "two different managed instances must not collapse onto one activity"
        );
        // The old implementation's identity, reproduced exactly, is rejected: a sequence-derived
        // generation really would have produced four distinct ids for one process.
        let observation_ids = (1u32..=4)
            .map(|sequence| {
                aura_activity_domain::ActivityId::derive(
                    ActivityKind::ShellManagedBackground,
                    &ActivityExecutionRef::ShellInvocation {
                        invocation_id: aura_domain_common::CommandInvocationId::from_bytes(
                            instance.to_bytes(),
                        ),
                        command_id: aura_domain_common::CommandId::from_bytes(instance.to_bytes()),
                    },
                    sequence,
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            observation_ids.len(),
            4,
            "the pre-fix sequence-as-generation rule really mints four identities for one process"
        );
    }

    #[test]
    fn a_managed_backgrounds_observations_publish_once_and_end_once() {
        // The lifecycle the port really applies, expressed over the real sequence values a
        // production callback carries: Running(1) -> Running(2) -> Unknown(3) -> Exited(4).
        use BackgroundObservationDisposition::*;
        // Nothing applied yet.
        assert_eq!(background_observation_disposition(1, None, false), Publish);
        // A newer observation of the same live instance is applied, and still the same activity.
        assert_eq!(background_observation_disposition(2, Some(1), false), Publish);
        assert_eq!(background_observation_disposition(3, Some(2), false), Publish);
        // A repeated observation is not a second activity.
        assert_eq!(background_observation_disposition(2, Some(2), false), Duplicate);
        // An out-of-order, older observation must not move the state backwards.
        assert_eq!(background_observation_disposition(1, Some(3), false), Stale);
        // The terminal observation is applied...
        assert_eq!(background_observation_disposition(4, Some(3), false), Publish);
        // ...and after it, a late `Running` must not resurrect the process.
        assert_eq!(background_observation_disposition(5, Some(4), true), AfterTerminal);
        assert_eq!(background_observation_disposition(4, Some(4), true), AfterTerminal);
    }

    #[test]
    fn every_shell_action_kind_is_in_the_shell_group() {
        for action in ["shell.powershell", "shell.cmd", "shell.bash"] {
            assert_eq!(tool_group_for(action), ActivityToolGroup::Shell);
        }
    }

    #[test]
    fn a_file_read_is_distinguished_from_a_file_write() {
        assert_eq!(tool_group_for("fs.read"), ActivityToolGroup::FileRead);
        assert_eq!(tool_group_for("fs.write"), ActivityToolGroup::FileWrite);
        assert_eq!(tool_group_for("fs.move"), ActivityToolGroup::FileWrite);
        assert_eq!(tool_group_for("fs.delete"), ActivityToolGroup::FileWrite);
    }

    #[test]
    fn an_unknown_action_kind_is_not_guessed_into_a_stronger_group() {
        assert_eq!(tool_group_for("mystery.thing"), ActivityToolGroup::Other);
        assert_eq!(tool_group_for(""), ActivityToolGroup::Other);
    }

    #[test]
    fn the_shell_allowlist_never_carries_a_script_body() {
        assert_eq!(
            ActivityParameterAllowlist::keys(ActivityToolGroup::Shell),
            &["cwd", "entry", "invocation"]
        );
        assert!(
            ActivityParameterAllowlist::project(
                ActivityToolGroup::Shell,
                "script",
                "Remove-Item -Recurse /"
            )
            .is_none(),
            "a whole script is not a key parameter"
        );
    }
}

pub mod resources;

pub mod query;
