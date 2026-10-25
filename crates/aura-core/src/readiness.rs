use aura_domain_common::{Revision, ToolWireVersionV1};
use aura_execution_contract::BackendReadiness;
use aura_projection_contract::{
    BackendLifecycle, BackendStatus, CapabilityReadiness, CapabilityStatus, CoreHealth,
    ExtensionReadiness, ExtensionStatus, RuntimeReadiness,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[cfg(test)]
#[path = "readiness_tests.rs"]
mod tests;

struct ReadinessState {
    revision: u64,
    sidecars: BTreeMap<String, BackendStatus>,
    execution: BTreeMap<String, Arc<dyn aura_backend_api::ExecutionBackend>>,
    execution_epoch: u64,
    execution_fingerprint: Option<aura_domain_common::jcs::CanonicalSha256HashV1>,
}

#[cfg(test)]
pub(super) fn current(state: &crate::RuntimeState) -> RuntimeReadiness {
    match &state.startup_mode {
        crate::StartupMode::Ready(services) => {
            // The sidecar readiness set exists only when the tool-policy
            // chain was really built. When its configuration domain is damaged the
            // honest answer is the default store's own `Unavailable` backends —
            // never `Ready`, and never a panic on a control path.
            let core_health = if state.ready.load(std::sync::atomic::Ordering::Acquire) {
                CoreHealth::Ready
            } else {
                CoreHealth::Starting
            };
            match services.security_sidecars() {
                Ok(sidecars) => sidecars.readiness.snapshot(core_health),
                Err(_) => ReadinessStore::default().snapshot(CoreHealth::Starting),
            }
        }
        crate::StartupMode::RecoveryRequired { .. } => {
            ReadinessStore::default().snapshot(CoreHealth::RecoveryRequired)
        }
    }
}

pub(super) async fn current_nonblocking(state: &crate::RuntimeState) -> RuntimeReadiness {
    match &state.startup_mode {
        crate::StartupMode::Ready(services) => {
            let core_health = if state.ready.load(std::sync::atomic::Ordering::Acquire) {
                CoreHealth::Ready
            } else {
                CoreHealth::Starting
            };
            // The sandbox sidecar set exists only when the tool-policy
            // chain was really built. When its configuration domain is damaged
            // there is no sidecar readiness to report, and the honest answer is
            // the default store's own `Unavailable` backends — never `Ready`, and
            // never a panic on the control path the user's confirmed rule requires
            // to survive.
            //
            // `CoreHealth::Starting` is the right value here rather than
            // `RecoveryRequired`: Aura's data does not require recovery, and the
            // capability-level reason is reported by the CoreHealth projection's
            // `core.ready_degraded` status, which names the capability and its
            // stable code.
            match services.security_sidecars() {
                Ok(sidecars) => sidecars.readiness.snapshot_nonblocking(core_health).await,
                Err(_) => ReadinessStore::default().snapshot(CoreHealth::Starting),
            }
        }
        crate::StartupMode::RecoveryRequired { .. } => {
            ReadinessStore::default().snapshot(CoreHealth::RecoveryRequired)
        }
    }
}

#[derive(Clone)]
pub(crate) struct ReadinessStore(Arc<Mutex<ReadinessState>>);

impl Default for ReadinessStore {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(ReadinessState {
            revision: 1,
            execution: BTreeMap::new(),
            execution_epoch: 0,
            execution_fingerprint: None,
            sidecars: ["strict_appcontainer", "egress_proxy"]
                .into_iter()
                .map(|id| {
                    (
                        id.into(),
                        BackendStatus {
                            backend: id.into(),
                            lifecycle: BackendLifecycle::Dormant,
                            readiness: BackendReadiness::Unavailable {
                                reason: "backend.not_started".into(),
                            },
                        },
                    )
                })
                .collect(),
        })))
    }
}
impl ReadinessStore {
    pub(crate) fn register_backend(
        &self,
        id: aura_execution_contract::BackendId,
        backend: Arc<dyn aura_backend_api::ExecutionBackend>,
    ) -> Result<(), &'static str> {
        let id = id.0;
        if id.is_empty() || matches!(id.as_str(), "core_broker" | "core-broker") {
            return Err("backend.invalid_registration");
        }
        let mut state = self.0.lock().map_err(|_| "backend.registry_unavailable")?;
        if state.execution.contains_key(&id) {
            return Err("backend.duplicate_registration");
        }
        state.execution.insert(id, backend);
        state.execution_epoch = state.execution_epoch.saturating_add(1);
        state.revision = state.revision.saturating_add(1);
        Ok(())
    }
    pub(crate) fn update(&self, backend: &str, lifecycle: BackendLifecycle, reason: &str) {
        // A poisoned registry is reflected as unavailable, never fabricated Ready.
        if let Ok(mut state) = self.0.lock() {
            state.revision = state.revision.saturating_add(1);
            state.sidecars.insert(
                backend.into(),
                BackendStatus {
                    backend: backend.into(),
                    lifecycle,
                    readiness: if lifecycle == BackendLifecycle::Ready {
                        BackendReadiness::SetupRequired {
                            reason: "backend.enforcement_probe_required".into(),
                        }
                    } else {
                        BackendReadiness::Unavailable { reason: reason.into() }
                    },
                },
            );
        }
    }
    pub(crate) fn snapshot(&self, core_health: CoreHealth) -> RuntimeReadiness {
        let (revision, mut backends, descriptors) = loop {
            let (execution_epoch, registered) = match self.0.lock() {
                Ok(state) => (
                    state.execution_epoch,
                    state
                        .execution
                        .iter()
                        .map(|(id, backend)| (id.clone(), Arc::clone(backend)))
                        .collect::<Vec<_>>(),
                ),
                Err(_) => {
                    break (
                        1,
                        vec![BackendStatus {
                            backend: "registry".into(),
                            lifecycle: BackendLifecycle::Degraded,
                            readiness: BackendReadiness::Unavailable {
                                reason: "backend.registry_unavailable".into(),
                            },
                        }],
                        Vec::new(),
                    );
                }
            };
            // Descriptor readiness may verify an installed runtime. Keep that
            // work outside the registry lock and outside Backend registration.
            let descriptors = registered
                .into_iter()
                .map(|(registered_id, backend)| (registered_id, backend.descriptor()))
                .collect::<Vec<_>>();
            let fingerprint = aura_domain_common::jcs::canonical_sha256(&descriptors).ok();
            let Ok(mut state) = self.0.lock() else {
                break (
                    1,
                    vec![BackendStatus {
                        backend: "registry".into(),
                        lifecycle: BackendLifecycle::Degraded,
                        readiness: BackendReadiness::Unavailable {
                            reason: "backend.registry_unavailable".into(),
                        },
                    }],
                    Vec::new(),
                );
            };
            if state.execution_epoch != execution_epoch {
                continue;
            }
            if fingerprint != state.execution_fingerprint {
                state.revision = state.revision.saturating_add(1);
                state.execution_fingerprint = fingerprint;
            }
            break (
                state.revision,
                state.sidecars.values().cloned().collect::<Vec<_>>(),
                descriptors,
            );
        };
        let descriptors = descriptors
            .into_iter()
            .filter_map(|(registered_id, descriptor)| {
                if descriptor.id.0 == registered_id {
                    Some(descriptor)
                } else {
                    backends.push(BackendStatus {
                        backend: registered_id,
                        lifecycle: BackendLifecycle::Degraded,
                        readiness: BackendReadiness::Unavailable {
                            reason: "backend.registration_identity_mismatch".into(),
                        },
                    });
                    None
                }
            })
            .collect::<Vec<_>>();
        for descriptor in &descriptors {
            backends.push(BackendStatus {
                backend: descriptor.id.0.clone(),
                lifecycle: if descriptor.readiness == BackendReadiness::Ready {
                    BackendLifecycle::Ready
                } else {
                    BackendLifecycle::Degraded
                },
                readiness: descriptor.readiness.clone(),
            });
        }
        let ready = core_health == CoreHealth::Ready;
        backends.push(BackendStatus {
            backend: "core_broker".into(),
            lifecycle: if ready { BackendLifecycle::Ready } else { BackendLifecycle::Dormant },
            readiness: if ready {
                BackendReadiness::Ready
            } else {
                BackendReadiness::Unavailable { reason: "core.not_ready".into() }
            },
        });
        let mut capabilities = [
            "models.configuration",
            "workspace.files",
            "session.history",
            "settings.configuration",
        ]
        .into_iter()
        .map(|name| CapabilityStatus {
            capability: name.into(),
            readiness: if ready {
                CapabilityReadiness::Ready
            } else {
                CapabilityReadiness::Unavailable
            },
            reason_code: if ready { "service.ready" } else { "core.not_ready" }.into(),
        })
        .collect::<Vec<_>>();
        let shell = ready
            && descriptors.iter().any(|backend| {
                backend.readiness == BackendReadiness::Ready
                    && backend
                        .capabilities
                        .contains(&aura_security_contract::ResolvedCapability::ProcessExecute)
                    && backend.class != aura_security_contract::BackendClass::CoreBroker
            });
        capabilities.push(CapabilityStatus {
            capability: "process.execution".into(),
            readiness: if shell {
                CapabilityReadiness::Ready
            } else {
                CapabilityReadiness::Unavailable
            },
            reason_code: if shell { "backend.ready" } else { "backend.requires_readiness_check" }
                .into(),
        });
        RuntimeReadiness {
            schema_version: ToolWireVersionV1,
            revision: Revision::new(revision),
            core_health,
            capabilities,
            backends,
            extensions: vec![
                // Plugins are installed and run through Core's own services.
                ExtensionStatus {
                    extension: "plugin".into(),
                    readiness: if ready {
                        ExtensionReadiness::Ready
                    } else {
                        ExtensionReadiness::Disabled
                    },
                    reason_code: if ready { "extension.ready" } else { "core.not_ready" }.into(),
                },
                ExtensionStatus {
                    extension: "browser".into(),
                    readiness: ExtensionReadiness::Disabled,
                    reason_code: "extension.not_enabled".into(),
                },
            ],
        }
    }
    pub(crate) async fn snapshot_nonblocking(&self, core_health: CoreHealth) -> RuntimeReadiness {
        let fallback_health = core_health;
        let store = self.clone();
        match tokio::task::spawn_blocking(move || store.snapshot(core_health)).await {
            Ok(snapshot) => snapshot,
            Err(_) => {
                let mut snapshot = ReadinessStore::default().snapshot(fallback_health);
                snapshot.backends.push(BackendStatus {
                    backend: "registry".into(),
                    lifecycle: BackendLifecycle::Degraded,
                    readiness: BackendReadiness::Unavailable {
                        reason: "backend.readiness_worker_failed".into(),
                    },
                });
                snapshot
            }
        }
    }
}
