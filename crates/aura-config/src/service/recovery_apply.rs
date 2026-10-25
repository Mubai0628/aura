//! Preview-bound recovery writes, scoped ownership and whole-set native compensation.
use super::*;
impl SettingsFacade {
    #[cfg(test)]
    pub(super) fn apply_bound_bundle_for_crash_test(
        &self,
        plan: &crate::recovery::BundleRecoveryPlan,
        action: crate::recovery::ConfigRecoveryAction,
    ) -> ! {
        let _ = self.apply_bound_bundle(plan, action, |index| {
            if index == 1 {
                std::process::exit(77);
            }
        });
        panic!("publication did not reach the real second-member window");
    }
    pub fn apply_domain_recovery(
        &self,
        plan: &crate::recovery::DomainRecoveryPlan,
        action: crate::recovery::ConfigRecoveryAction,
    ) -> Result<crate::recovery::RecoveryOutcome, crate::recovery::ConfigRecoveryError> {
        self.apply_domain_observed(plan, action, || {})
    }

    fn apply_domain_observed(
        &self,
        plan: &crate::recovery::DomainRecoveryPlan,
        action: crate::recovery::ConfigRecoveryAction,
        before_publish: impl FnOnce(),
    ) -> Result<crate::recovery::RecoveryOutcome, crate::recovery::ConfigRecoveryError> {
        use crate::recovery::{ConfigRecoveryAction, RecoveryOutcome, codes};

        let module = plan.module;
        // The same component admission every other write uses.
        self.ensure_not_held_by_recovery(module).map_err(|source| {
            crate::recovery::ConfigRecoveryError::Apply { module, source: Box::new(source) }
        })?;
        if action == ConfigRecoveryAction::Retry {
            if !plan.allows(action) {
                return Err(crate::recovery::ConfigRecoveryError::Unavailable {
                    module,
                    reason_code: codes::ACTION_UNAVAILABLE,
                });
            }
            let recovered = self.reload(module).map_err(|source| {
                crate::recovery::ConfigRecoveryError::Apply { module, source: Box::new(source) }
            })?;
            let current = self.snapshot(module).ok();
            return Ok(RecoveryOutcome {
                module,
                action,
                applied: recovered || current.is_some(),
                revision: current.as_ref().map(|s| s.document.revision),
                source: current.as_ref().map(|s| s.source),
                reason_code: None,
            });
        }
        let _claim = self.claim_one(module, DomainClaim::Exclusive).map_err(|source| {
            crate::recovery::ConfigRecoveryError::Apply { module, source: Box::new(source) }
        })?;
        let bound = self.bound_domain_recovery(module)?;
        if bound.plan.preview_id != plan.preview_id {
            return Err(crate::recovery::ConfigRecoveryError::Unavailable {
                module,
                reason_code: codes::PREVIEW_STALE,
            });
        }
        let fresh = &bound.plan;
        let option =
            fresh.option(action).ok_or(crate::recovery::ConfigRecoveryError::Unavailable {
                module,
                reason_code: codes::ACTION_NOT_OFFERED,
            })?;
        if !option.available {
            return Err(crate::recovery::ConfigRecoveryError::Unavailable {
                module,
                reason_code: codes::ACTION_UNAVAILABLE,
            });
        }

        let source = match action {
            ConfigRecoveryAction::RestorePreviousValid => ConfigSource::LastKnownGood,
            ConfigRecoveryAction::ResetToDefault => ConfigSource::Default,
            ConfigRecoveryAction::Retry => unreachable!("returned above"),
        };
        let (document, bytes) = bound.candidate(self, action, None)?;
        before_publish();
        let snapshot =
            self.install_bound_recovery(module, document, bytes, source, bound.original)?;
        Ok(RecoveryOutcome {
            module,
            action,
            applied: true,
            revision: Some(snapshot.document.revision),
            source: Some(source),
            reason_code: None,
        })
    }

    fn install_bound_recovery(
        &self,
        module: ConfigModule,
        document: ModuleConfigDocument,
        bytes: Vec<u8>,
        source: ConfigSource,
        original: recovery_publication::Input,
    ) -> Result<Arc<ConfigSnapshot>, crate::recovery::ConfigRecoveryError> {
        let apply_error = |source| crate::recovery::ConfigRecoveryError::Apply {
            module,
            source: Box::new(source),
        };
        original.preserve(self, module).map_err(apply_error)?;
        let journal =
            recovery_journal::Record::create(&self.paths, [(module, &original, &document)])
                .map_err(apply_error)?;
        let publication =
            original.publish(&self.paths.config_path(module), &bytes).and_then(|file| {
                file.commit().map_err(|error| {
                    recovery_publication::publication_error(&self.paths.config_path(module), error)
                })
            });
        if let Err(primary) = publication {
            if recovery_journal::uncertain(&primary)
                || remove_settled_bundle_journal(&journal).is_err()
            {
                self.hold_failed_recovery(&[module]);
            }
            return Err(apply_error(primary));
        }
        let settled = (|| {
            record_valid_history(
                &self.paths.history_path(module),
                module,
                document.revision,
                &bytes,
            )
            .map_err(apply_error)?;
            let installed = Arc::new(snapshot(module, document, &bytes, source));
            let mut services = self.services.lock().map_err(|_| {
                apply_error(SettingsFacadeError::LockPoisoned("config domain services"))
            })?;
            if let Some(service) = services.get(&module) {
                service.replace(Arc::clone(&installed)).map_err(apply_error)?;
            } else {
                services.insert(
                    module,
                    Arc::new(ModuleConfigService {
                        history_dir: self.paths.history_path(module),
                        module,
                        path: self.paths.config_path(module),
                        snapshot: RwLock::new(Arc::clone(&installed)),
                        validator: self.validator_for(module),
                    }),
                );
            }
            drop(services);
            self.publish(ConfigEvent {
                module,
                kind: ConfigEventKind::Recovered,
                revision: installed.document.revision,
                message: "The configuration domain was recovered by user action.".to_owned(),
                occurred_at: UtcTimestamp::now(),
            })
            .map_err(apply_error)?;
            // Publication, history and live activation must all settle before responsibility
            // is deleted or health becomes Ready. Any failure leaves the durable record usable.
            remove_settled_bundle_journal(&journal).map_err(apply_error)?;
            self.health.record_recovery(module, installed.source, installed.document.revision);
            self.drop_recovery_hold(module);
            Ok(installed)
        })();
        if settled.is_err() {
            self.hold_failed_recovery(&[module]);
        }
        settled
    }

    pub fn apply_bundle_recovery(
        &self,
        plan: &crate::recovery::BundleRecoveryPlan,
        action: crate::recovery::ConfigRecoveryAction,
    ) -> Result<Vec<crate::recovery::RecoveryOutcome>, crate::recovery::ConfigRecoveryError> {
        self.apply_bound_bundle(plan, action, |_| {})
    }

    fn apply_bound_bundle(
        &self,
        plan: &crate::recovery::BundleRecoveryPlan,
        action: crate::recovery::ConfigRecoveryAction,
        mut before_publish: impl FnMut(usize),
    ) -> Result<Vec<crate::recovery::RecoveryOutcome>, crate::recovery::ConfigRecoveryError> {
        use crate::recovery::{ConfigRecoveryAction, ConfigRecoveryError, codes};
        let unavailable = |reason_code| ConfigRecoveryError::Unavailable {
            module: ConfigModule::Aura,
            reason_code,
        };
        if plan.domains.is_empty() || action == ConfigRecoveryAction::Retry || !plan.allows(action)
        {
            return Err(unavailable(codes::ACTION_NOT_AVAILABLE_FOR_BUNDLE));
        }
        let modules: Vec<_> = plan.domains.iter().map(|p| p.module).collect();
        let _claim = self
            .claim_set(modules.iter().map(|m| (*m, DomainClaim::Exclusive)))
            .map_err(|_| unavailable(codes::ACTION_NOT_AVAILABLE_FOR_BUNDLE))?;
        let fresh = self.plan_bundle_recovery(&modules)?;
        if fresh.preview_id != plan.preview_id || !fresh.allows(action) {
            return Err(unavailable(codes::PREVIEW_STALE));
        }
        let superseded = recovery_reset::superseded(self, &modules).map_err(|source| {
            ConfigRecoveryError::Apply { module: modules[0], source: Box::new(source) }
        })?;
        if superseded.is_some() && action != ConfigRecoveryAction::ResetToDefault {
            return Err(unavailable(codes::ACTION_NOT_AVAILABLE_FOR_BUNDLE));
        }
        let originals =
            recovery_reset::hold_originals(&self.paths, superseded.as_deref().unwrap_or_default())
                .map_err(|_| unavailable(codes::PREVIEW_STALE))?;
        let mut staged = Vec::new();
        for domain in &fresh.domains {
            let bound = self.bound_domain_in_set(domain.module, superseded.is_some())?;
            if bound.plan.preview_id != domain.preview_id {
                return Err(unavailable(codes::PREVIEW_STALE));
            }
            let joint = if action == ConfigRecoveryAction::RestorePreviousValid {
                Some(
                    fresh
                        .consistent_set
                        .iter()
                        .find(|entry| entry.0 == domain.module)
                        .ok_or_else(|| unavailable(codes::NO_CONSISTENT_BUNDLE_VERSION))?,
                )
            } else {
                None
            };
            let (document, next) = bound.candidate(self, action, joint)?;
            bound.original.preserve(self, domain.module).map_err(|source| {
                ConfigRecoveryError::Apply { module: domain.module, source: Box::new(source) }
            })?;
            staged.push((domain.module, bound.original, document, next));
        }
        let resetting_held = superseded.is_some();
        let journal = recovery_journal::Record::create_with_superseded(
            &self.paths,
            staged.iter().map(|(module, original, document, _)| (*module, original, document)),
            superseded.unwrap_or_default(),
        )
        .map_err(|source| ConfigRecoveryError::Apply {
            module: modules[0],
            source: Box::new(source),
        })?;
        // Retain the native publication handles until every member has been published.
        // Abort uses those exact objects; it never writes previous bytes over a competitor.
        let mut published: Vec<(ConfigModule, aura_windows_platform::HandleBoundPublishedFile)> =
            Vec::new();
        for (index, (module, original, _document, next)) in staged.iter().enumerate() {
            before_publish(index);
            let path = self.paths.config_path(*module);
            match original.publish(&path, next) {
                Ok(file) => published.push((*module, file)),
                Err(source) => {
                    let mut failed = recovery_journal::uncertain(&source);
                    for (_, file) in published.into_iter().rev() {
                        failed |= file.abort().is_err();
                    }
                    if failed {
                        self.hold_failed_recovery(&modules);
                        return Err(unavailable(codes::PARTIAL_APPLICATION_ROLLBACK_FAILED));
                    }
                    if remove_settled_bundle_journal(&journal).is_err() {
                        self.hold_failed_recovery(&modules);
                    }
                    return Err(ConfigRecoveryError::Apply {
                        module: *module,
                        source: Box::new(source),
                    });
                }
            }
        }
        // A commit failure compensates only publications still owned by this set. Completed
        // members are checked against their returned identity + bytes before compensation.
        let mut committed = Vec::new();
        let mut pending = published.into_iter();
        while let Some((module, file)) = pending.next() {
            match file.commit() {
                Ok(info) => committed.push((module, info.identity)),
                Err(error) => {
                    let mut failed =
                        error.location() == aura_windows_platform::PublishLocation::Unconfirmable;
                    for (_, file) in pending {
                        failed |= file.abort().is_err();
                    }
                    for (done, identity) in committed.into_iter().rev() {
                        let (_, original, _, next) =
                            staged.iter().find(|entry| entry.0 == done).expect("set member");
                        failed |= original
                            .restore_after(&self.paths.config_path(done), &identity, next)
                            .is_err();
                    }
                    if failed {
                        self.hold_failed_recovery(&modules);
                        return Err(unavailable(codes::PARTIAL_APPLICATION_ROLLBACK_FAILED));
                    }
                    if remove_settled_bundle_journal(&journal).is_err() {
                        self.hold_failed_recovery(&modules);
                    }
                    return Err(ConfigRecoveryError::Apply {
                        module,
                        source: Box::new(recovery_publication::publication_error(
                            &self.paths.config_path(module),
                            error,
                        )),
                    });
                }
            }
        }
        let settled = staged
            .iter()
            .map(|(module, _, document, bytes)| {
                self.finish_domain_recovery(*module, action, document, bytes)
            })
            .collect::<Result<Vec<_>, _>>();
        drop(originals);
        match settled {
            Ok(outcomes) => {
                if let Err(source) = recovery_journal::record_joint(self, &journal)
                    .and_then(|()| remove_settled_bundle_journal(&journal))
                {
                    self.hold_failed_recovery(&modules);
                    return Err(ConfigRecoveryError::Apply {
                        module: modules[0],
                        source: Box::new(source),
                    });
                }
                if resetting_held {
                    self.recover_pending_bundle_transactions().map_err(|source| {
                        ConfigRecoveryError::Apply { module: modules[0], source: Box::new(source) }
                    })?;
                }
                Ok(outcomes)
            }
            Err(error) => {
                self.hold_failed_recovery(&modules);
                Err(error)
            }
        }
    }

    fn hold_failed_recovery(&self, modules: &[ConfigModule]) {
        let error = SettingsFacadeError::BundleUnresolved {
            transaction_id: "user-recovery-compensation".into(),
        };
        if let Ok(mut held) = self.recovery.lock() {
            for module in modules {
                if !held.held.contains(module) {
                    held.held.push(*module);
                }
            }
        }
        for module in modules {
            self.health.mark_degraded(*module, &error);
        }
    }
}

#[cfg(test)]
mod tests;
