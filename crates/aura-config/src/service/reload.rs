//! Explicit reloads and observational watcher reloads share the same domain admission.
use super::*;

impl SettingsFacade {
    /// Accepts an external edit for one domain, or re-opens a degraded one.
    ///
    /// A degraded domain has no service at all, so the first step is to try to
    /// open it again: if the file is now readable and valid this is a real
    /// recovery, and the domain becomes usable without restarting the process.
    /// The original failure stays on the record.
    ///
    /// A single-domain reload **cannot bypass an unresolved named
    /// bundle**. Before R2 a bundle whose convergence failed degraded its
    /// domains but the very next `reload` re-opened one of them from whatever
    /// bytes happened to be on disk, marking it available again while the
    /// unresolved journal was still there. The rule now is that the domain stays
    /// held until the *bundle* is judged, because one domain of a half-published
    /// bundle cannot be re-derived in isolation — its file may be neither the
    /// before nor the after image.
    ///
    /// The refusal is a real error rather than `Ok(false)`: a caller that asked
    /// for a reload of a held domain must not be told that nothing changed.
    pub fn reload(&self, module: ConfigModule) -> Result<bool, SettingsFacadeError> {
        self.reload_input(module, true)
    }

    /// A watcher observes current bytes; recovery publication requires an explicit action.
    pub(crate) fn reload_for_watch(
        &self,
        module: ConfigModule,
    ) -> Result<bool, SettingsFacadeError> {
        self.reload_input(module, false)
    }

    fn reload_input(
        &self,
        module: ConfigModule,
        allow_recovery: bool,
    ) -> Result<bool, SettingsFacadeError> {
        self.ensure_not_held_by_recovery(module)?;
        // An ordinary reload considers only its **own** domain's
        // transaction. Before this it returned `Ok(false)` — a silent skip —
        // whenever *any* transaction was live, so a bundle on `Commands`/`Browser`
        // silently stopped `Ui` from reloading. A transaction that does not name
        // this domain is irrelevant here.
        let covered = self
            .transactions
            .lock()
            .map_err(|_| SettingsFacadeError::LockPoisoned("config bundle transactions"))?
            .conflict_for(&[module])
            .map(|reservation| reservation.transaction_id.clone());
        if let Some(transaction_id) = covered {
            // The domain is mid-transaction: its file may be in either image, so a
            // reload cannot decide. This is reported, not silently skipped, and it
            // names the transaction that owns the domain.
            return Err(SettingsFacadeError::BundlePending { transaction_id });
        }
        // A domain re-open is a real exclusive operation on that one
        // domain. It claims only this domain and holds the claim across the
        // bounded open, so a second re-open of the *same* domain cannot run
        // concurrently — but every other domain is free.
        let _claim = self.claim_one(module, DomainClaim::Exclusive)?;
        let service = {
            let services = self
                .services
                .lock()
                .map_err(|_| SettingsFacadeError::LockPoisoned("config domain services"))?;
            services.get(&module).map(Arc::clone)
        };
        let Some(service) = service else {
            return self.retry_degraded_domain(module, allow_recovery);
        };
        match service.reload() {
            Ok(Some(updated)) => {
                self.health.record_snapshot(module, updated.source, updated.document.revision);
                self.publish(ConfigEvent {
                    module,
                    kind: ConfigEventKind::Changed,
                    revision: updated.document.revision,
                    message: "External configuration edit was accepted.".to_owned(),
                    occurred_at: UtcTimestamp::now(),
                })?;
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(error) => {
                let revision = service.snapshot()?.document.revision;
                // A rejected edit leaves the domain serving its previous valid
                // snapshot, so the domain stays ready; only the attempt and the
                // real failure are recorded.
                self.health.record_write_failure(module, &error);
                self.publish(ConfigEvent {
                    module,
                    kind: ConfigEventKind::Rejected,
                    revision,
                    message:
                        "External configuration edit was rejected; the previous valid snapshot remains active."
                            .to_owned(),
                    occurred_at: UtcTimestamp::now(),
                })?;
                Err(error)
            }
        }
    }

    /// Re-opens a domain that failed at startup.
    ///
    /// The domain map is behind its own mutex rather than being frozen at
    /// construction, because a domain that was unavailable at startup has to be
    /// able to become available later without restarting the process. The lock is
    /// held only to swap one entry, never across I/O.
    ///
    /// Explicit and watcher reloads both apply the same component admission
    /// (hold check plus the one-owner claim) that an ordinary save uses. Only
    /// an explicit reload may publish recovery; a watcher only reads current input.
    fn retry_degraded_domain(
        &self,
        module: ConfigModule,
        allow_recovery: bool,
    ) -> Result<bool, SettingsFacadeError> {
        let validator: Arc<dyn ModuleConfigValidator> = self
            .validators
            .get(&module)
            .cloned()
            .unwrap_or_else(|| Arc::new(BaseModuleConfigValidator));
        let opened = if allow_recovery {
            ModuleConfigService::open(&self.paths, module, validator)
        } else {
            ModuleConfigService::open_live_input(&self.paths, module, validator)
                .map(|service| (service, None))
        };
        match opened {
            Ok((service, recovery)) => {
                let current = service.snapshot()?;
                let source = current.source;
                let revision = current.document.revision;
                self.services
                    .lock()
                    .map_err(|_| SettingsFacadeError::LockPoisoned("config domain services"))?
                    .insert(module, Arc::new(service));
                self.health.record_recovery(module, source, revision);
                if let Some(event) = recovery {
                    self.publish(event)?;
                }
                self.publish(ConfigEvent {
                    module,
                    kind: ConfigEventKind::Recovered,
                    revision,
                    message: "The configuration domain was re-opened successfully.".to_owned(),
                    occurred_at: UtcTimestamp::now(),
                })?;
                Ok(true)
            }
            Err(error) => {
                self.health.record_failure(module, &error);
                Err(error)
            }
        }
    }
}

impl ModuleConfigService {
    /// Reads only the supplied input. Invalid or absent files remain degraded.
    fn open_live_input(
        paths: &AuraPaths,
        module: ConfigModule,
        validator: Arc<dyn ModuleConfigValidator>,
    ) -> Result<Self, SettingsFacadeError> {
        let path = paths.config_path(module);
        let history_dir = paths.history_path(module);
        let bytes = read_config_file(module, &path)?;
        let document = parse_and_validate(module, &bytes, validator.as_ref())?;
        record_valid_history(&history_dir, module, document.revision, &bytes)?;
        let current = Arc::new(snapshot(module, document, &bytes, ConfigSource::File));
        Ok(Self { history_dir, module, path, snapshot: RwLock::new(current), validator })
    }
}
