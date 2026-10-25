//! Built-in Shell uses only compat; other execution retains its scoped selection.
use super::*;
use aura_execution_orchestrator::Selection;

impl AgentExecution {
    /// Chooses the backend for one call. `pub(super)` because the selection
    /// regression drives exactly this entry point rather than a copy of it.
    pub(super) async fn select_backend(
        &self,
        policy: &aura_permission_engine::policy::PolicyInput<'_>,
        candidates: &[Arc<dyn aura_backend_api::ExecutionBackend>],
        explicit: Option<&str>,
    ) -> Result<Selection, aura_execution_contract::BackendError> {
        // A package program runs only on its own Native backend, chosen explicitly.
        if package_tools::program::is_program(&policy.operation.content) {
            let Some(native) = candidates
                .iter()
                .find(|backend| backend.descriptor().id.0 == package_tools::program::NATIVE_ID)
            else {
                return Ok(Selection::Denied { reason: PolicyDenyReason::BackendUnavailable });
            };
            return self
                .orchestrator
                .select(policy, std::slice::from_ref(native), Some(&native.descriptor().id))
                .await;
        }
        if policy.operation.content.action_kind.starts_with("git.") {
            let expected = if super::git::is_write(&policy.operation.content.action_kind) {
                super::git::NATIVE_ID
            } else {
                super::git::BACKEND_ID
            };
            let Some(git) = candidates.iter().find(|backend| backend.descriptor().id.0 == expected)
            else {
                return Ok(Selection::Denied { reason: PolicyDenyReason::BackendUnavailable });
            };
            return self
                .orchestrator
                .select(policy, std::slice::from_ref(git), Some(&git.descriptor().id))
                .await;
        }
        if aura_security_contract::built_in_shell(
            &policy.operation.content.source,
            &policy.operation.content.action_kind,
        ) {
            let Some(compat) = candidates.iter().find(|backend| {
                backend.descriptor().id.0
                    == if policy.operation.content.action_kind.starts_with("terminal.") {
                        super::terminal::BACKEND_ID
                    } else {
                        aura_backend_windows_user::compat::COMPAT_BACKEND_ID
                    }
            }) else {
                return Ok(Selection::Denied { reason: PolicyDenyReason::BackendUnavailable });
            };
            let id = compat.descriptor().id;
            if explicit.is_some_and(|requested| requested != id.0) {
                return Ok(Selection::Denied { reason: PolicyDenyReason::BackendUnavailable });
            }
            return self.orchestrator.select(policy, std::slice::from_ref(compat), Some(&id)).await;
        }
        let native = candidates
            .iter()
            .find(|backend| backend.descriptor().class == BackendClass::Native)
            .cloned();
        let restricted = candidates
            .iter()
            .filter(|backend| backend.descriptor().class != BackendClass::Native)
            .cloned()
            .collect::<Vec<_>>();
        if explicit == Some(aura_backend_windows_user::NATIVE_BACKEND_ID) {
            let Some(native) = native else {
                return Ok(Selection::Denied { reason: PolicyDenyReason::BackendUnavailable });
            };
            let id = native.descriptor().id;
            return self.orchestrator.select(policy, &[native], Some(&id)).await;
        }
        // Three separate questions, asked separately.
        //
        // `policy_assessment` is "may this call run at all". `policy_admits_restricted`
        // is "is a restricted backend the right kind of backend for it", compared
        // without readiness: the binding-refusal candidates below are exactly the
        // ones reported as `RepairRequired`, and a comparison that insisted on
        // readiness would read them as "wrong kind of backend" instead of "right
        // kind of backend, currently broken". [`Self::orchestrator`]'s selector then
        // asks the third question, "which healthy backend", using readiness.
        //
        // Conflating the second and third questions is what let an identity refusal
        // become a general `BackendUnavailable` and then a Native offer.
        let policy_assessment = aura_permission_engine::policy::assess(policy).ok();
        // "Could this candidate have run this call at all", asked without readiness. An
        // inapplicable candidate is not evidence about the operation: a Workspace-mode
        // backend offered for a call that declares network cannot answer for it, so its
        // own installation state must not decide the call's outcome either.
        let applicable = |descriptor: &BackendDescriptor| {
            policy_assessment.as_ref().is_some_and(|assessment| {
                descriptor.supports_capabilities(
                    &aura_permission_engine::policy::required_for_backend(
                        assessment,
                        policy.operation,
                        descriptor.class,
                    ),
                    policy.operation,
                )
            })
        };
        let policy_admits_restricted =
            restricted.iter().any(|backend| applicable(&backend.descriptor()));
        let binding_mismatch = restricted.iter().find_map(|backend| {
            let descriptor = backend.descriptor();
            (applicable(&descriptor)
                && aura_backend_windows_user::WindowsUserBackend::descriptor_is_identity_boundary(
                    &descriptor,
                ))
            .then(|| {
                aura_backend_windows_user::WindowsUserBackend::descriptor_readiness_reason(
                    &descriptor,
                )
                .unwrap_or_default()
                .to_owned()
            })
            .filter(|reason| !reason.is_empty())
        });
        // A call the policy refuses keeps the policy's own reason, whichever
        // backend was inspected, so the refusal never stops naming the rule that
        // actually stopped it.
        if policy_assessment.is_none() {
            return self.orchestrator.select(policy, &restricted, None).await;
        }
        // An identity boundary is not "this backend is unavailable". While the
        // policy would have run the call on a restricted backend, it must not
        // become a reason to offer a broader one: a Native approval would let the
        // user authorize work that the installation is actively refusing to run
        // under its own Core identity.
        //
        // The refusal is tied to the candidate that reported it. A candidate this call
        // could not have used says nothing about the call, and letting it answer would
        // turn a healthy installation into a refusal simply because an unrelated one
        // was offered first.
        if policy_admits_restricted && let Some(reason) = binding_mismatch {
            return Err(BackendError::PreparationRejected { reason });
        }
        match self.orchestrator.select(policy, &restricted, None).await? {
            Selection::Denied { reason: PolicyDenyReason::BackendUnavailable } => {
                let Some(native) = native else {
                    return Ok(Selection::Denied { reason: PolicyDenyReason::BackendUnavailable });
                };
                let id = native.descriptor().id;
                self.orchestrator.select(policy, &[native], Some(&id)).await
            }
            selected => Ok(selected),
        }
    }
}
