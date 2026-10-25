//! Read-only recovery planning; the private material binds what an eventual write may consume.
use super::*;
impl SettingsFacade {
    pub fn plan_domain_recovery(
        &self,
        module: ConfigModule,
    ) -> Result<crate::recovery::DomainRecoveryPlan, crate::recovery::ConfigRecoveryError> {
        Ok(self.bound_domain_recovery(module)?.plan)
    }
    pub(super) fn bound_domain_recovery(
        &self,
        module: ConfigModule,
    ) -> Result<recovery_publication::BoundPlan, crate::recovery::ConfigRecoveryError> {
        self.bound_domain_in_set(module, false)
    }
    pub(super) fn bound_domain_in_set(
        &self,
        module: ConfigModule,
        whole_held_set: bool,
    ) -> Result<recovery_publication::BoundPlan, crate::recovery::ConfigRecoveryError> {
        use crate::recovery::{
            ConfigRecoveryAction, DomainRecoveryPlan, RecoverableVersion, RecoveryOption, codes,
            cross_domain_references, loss_of, next_revision_for, recorded_versions,
            validate_recorded_version,
        };

        let plan_error = |source: SettingsFacadeError| crate::recovery::ConfigRecoveryError::Plan {
            module,
            source: Box::new(source),
        };

        let original = recovery_publication::Input::capture(self, module);
        let health = self.health.health(module);
        let validator = self.validator_for(module);
        let current_snapshot = self.snapshot(module).ok();
        let current_document =
            current_snapshot.as_ref().map(|snapshot| snapshot.document.as_ref().clone());

        // Is an unresolved bundle still holding this domain? If so, recovery of the
        // domain in isolation is refused: R2's rule is that one domain of a
        // half-published bundle cannot be re-derived alone.
        let held = {
            let disposition =
                self.recovery.lock().map_err(|_| crate::recovery::ConfigRecoveryError::Plan {
                    module,
                    source: Box::new(SettingsFacadeError::LockPoisoned(
                        "config recovery disposition",
                    )),
                })?;
            disposition.held.contains(&module) && !whole_held_set
        };

        // The recorded versions, re-validated under the CURRENT validator.
        let versions = recorded_versions(&self.paths, module).map_err(plan_error)?;
        let mut recoverable = Vec::with_capacity(versions.len());
        let mut best_usable: Option<(u64, crate::document::ModuleConfigDocument)> = None;
        for version in &versions {
            match validate_recorded_version(module, version, validator.as_ref()) {
                Ok(document) => {
                    recoverable.push(RecoverableVersion {
                        revision: version.revision,
                        passes_current_validation: true,
                        rejection_code: None,
                    });
                    if best_usable.is_none() {
                        best_usable = Some((version.revision, document));
                    }
                }
                Err(_) => recoverable.push(RecoverableVersion {
                    revision: version.revision,
                    passes_current_validation: false,
                    rejection_code: Some("config.recovery.version_no_longer_valid".to_owned()),
                }),
            }
        }

        // The revision the recovery result would carry.
        //
        // A degraded domain reports **no** revision in its health — that is R2's
        // honest "I am serving nothing" — so the served revision cannot be the
        // basis. The basis is the highest revision this domain has ever committed:
        // the live file's own revision when it can be read, otherwise the highest
        // recorded version. Using health alone would compute `0 + 1 = 1` and then
        // write revision 1 over a domain whose history already reached 2.
        let committed_revision = self.highest_committed_revision(module).map_err(|source| {
            crate::recovery::ConfigRecoveryError::Plan { module, source: Box::new(source) }
        })?;
        let next_revision_value = next_revision_for(module, committed_revision).map_err(|_| {
            crate::recovery::ConfigRecoveryError::Unavailable {
                module,
                reason_code: codes::NO_CURRENT_SNAPSHOT,
            }
        })?;

        let default = crate::recovery::validated_default(module, validator.as_ref())
            .ok()
            .map(|value| value.0);
        let mut options = Vec::with_capacity(3);
        // Retry is always offered: it changes nothing on disk and is the action a
        // user tries first when they have just repaired a file by hand.
        if held {
            options.push(RecoveryOption::unavailable_for_plan(
                ConfigRecoveryAction::Retry,
                codes::HELD_BY_UNRESOLVED_BUNDLE,
                "an unresolved configuration bundle still includes this domain, so the domain \
                 cannot be re-read in isolation until the bundle is judged",
            ));
        } else {
            options.push(RecoveryOption::available_for_plan(ConfigRecoveryAction::Retry));
        }

        if held {
            options.push(RecoveryOption::unavailable_for_plan(
                ConfigRecoveryAction::RestorePreviousValid,
                codes::HELD_BY_UNRESOLVED_BUNDLE,
                "an unresolved configuration bundle still includes this domain",
            ));
            options.push(RecoveryOption::unavailable_for_plan(
                ConfigRecoveryAction::ResetToDefault,
                codes::HELD_BY_UNRESOLVED_BUNDLE,
                "an unresolved configuration bundle still includes this domain",
            ));
        } else {
            match &best_usable {
                Some((revision, _)) => options.push(RecoveryOption::available_with_detail(
                    ConfigRecoveryAction::RestorePreviousValid,
                    format!("revision {revision} still passes the current validation"),
                )),
                None if versions.is_empty() => options.push(RecoveryOption::unavailable_for_plan(
                    ConfigRecoveryAction::RestorePreviousValid,
                    codes::NO_HISTORY,
                    "this domain has no recorded valid version, so there is no previous \
                     configuration to restore; use the scoped default reset instead",
                )),
                None => options.push(RecoveryOption::unavailable_for_plan(
                    ConfigRecoveryAction::RestorePreviousValid,
                    codes::NO_VALID_PREVIOUS_VERSION,
                    "every recorded version of this domain fails the current validation, so no \
                     previous configuration can be restored; use the scoped default reset instead",
                )),
            }
            let default_detail = crate::recovery::validated_default(module, validator.as_ref())
                .map_or_else(
                    |reason| {
                        format!("the domain default does not pass current validation: {reason}")
                    },
                    |_| "the domain default passes the current validation".to_owned(),
                );
            if crate::recovery::validated_default(module, validator.as_ref()).is_ok() {
                options.push(RecoveryOption::available_with_detail(
                    ConfigRecoveryAction::ResetToDefault,
                    default_detail,
                ));
            } else {
                options.push(RecoveryOption::unavailable_for_plan(
                    ConfigRecoveryAction::ResetToDefault,
                    "config.recovery.default_fails_validation",
                    default_detail,
                ));
            }
        }

        // The references and the loss, computed against the version that WOULD be
        // applied, so the preview describes the real effect.
        let (references, loss) = match &best_usable {
            Some((_, document)) => {
                (cross_domain_references(document), loss_of(current_document.as_ref(), document))
            }
            None => (
                current_document.as_ref().map(cross_domain_references).unwrap_or_default(),
                vec!["no previous version is applicable".to_owned()],
            ),
        };

        // Binding includes the exact validated candidates, not just their revision labels.
        let preview_id = recovery_publication::preview_identity(
            module,
            &original,
            &health,
            committed_revision,
            &options,
            best_usable.as_ref().map(|(_, document)| document),
            default.as_ref(),
        )
        .map_err(plan_error)?;

        let plan = DomainRecoveryPlan {
            module,
            degraded: !health.is_ready(),
            fault_code: health.original_fault.map(|fault| fault.code().to_owned()),
            current_revision: health.revision,
            options,
            recoverable_versions: recoverable,
            next_revision: next_revision_value,
            references_needing_rebinding: references,
            loss,
            scope_known: true,
            preview_id,
        };
        Ok(recovery_publication::BoundPlan {
            plan,
            original,
            restore: best_usable.map(|(_, document)| document),
            default,
        })
    }
}

impl SettingsFacade {
    /// The highest revision this domain has ever committed.
    ///
    /// A degraded domain serves nothing, so it reports no revision — but
    /// recovery still has to know how far the domain's history has really reached,
    /// or it would write a revision the file already passed. The answer is the
    /// maximum of the validated live file's revision and the highest
    /// recorded version.
    ///
    /// An unreadable live file is not an error here: it is exactly the degraded
    /// case this entry exists for, and the recorded history answers instead.
    pub(super) fn highest_committed_revision(
        &self,
        module: ConfigModule,
    ) -> Result<Option<u64>, SettingsFacadeError> {
        let mut highest = None;
        let validator = self.validator_for(module);
        // A readable but invalid external input is not a committed domain version.
        let path = self.paths.config_path(module);
        if let Ok(bytes) = read_config_file(module, &path)
            && let Ok(document) = parse_and_validate(module, &bytes, validator.as_ref())
        {
            highest = Some(document.revision);
        }
        // Every recorded version, which exists only because it really validated.
        for version in crate::recovery::recorded_versions(&self.paths, module)? {
            highest =
                Some(highest.map_or(version.revision, |current| current.max(version.revision)));
        }
        Ok(highest)
    }
}
