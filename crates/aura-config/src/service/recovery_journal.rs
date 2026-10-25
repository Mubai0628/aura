//! Durable responsibility for user-confirmed recovery. This record contains validated
//! configuration candidates and input bindings, never the damaged original's raw bytes.
//! A restart can complete only the confirmed set; a competing edit keeps the set held.
use super::*;
use aura_windows_platform::NativeFileIdentity;
use recovery_publication::Input;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    recovery_version: u8,
    transaction_id: String,
    created_at: UtcTimestamp,
    entries: Vec<Entry>,
    #[serde(default)]
    superseded: Vec<recovery_reset::JournalBinding>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    module: ConfigModule,
    before: Before,
    candidate: ModuleConfigDocument,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
enum Before {
    Present { volume: String, file_id: [u8; 16], hash: String },
    Absent,
}

impl Record {
    pub fn create<'a>(
        paths: &AuraPaths,
        entries: impl IntoIterator<Item = (ConfigModule, &'a Input, &'a ModuleConfigDocument)>,
    ) -> Result<recovery_reset::LiveRecord, SettingsFacadeError> {
        Self::create_with_superseded(paths, entries, Vec::new())
    }

    pub fn create_with_superseded<'a>(
        paths: &AuraPaths,
        entries: impl IntoIterator<Item = (ConfigModule, &'a Input, &'a ModuleConfigDocument)>,
        superseded: Vec<recovery_reset::JournalBinding>,
    ) -> Result<recovery_reset::LiveRecord, SettingsFacadeError> {
        let mut members = Vec::new();
        for (module, input, candidate) in entries {
            let before = match input {
                Input::Present { identity, bytes } => Before::Present {
                    volume: identity.volume_serial_number.to_string(),
                    file_id: identity.file_id,
                    hash: ContentHash::blake3(bytes).to_string(),
                },
                Input::Absent => Before::Absent,
                Input::Unreadable(_) => {
                    return Err(SettingsFacadeError::DomainUnavailable {
                        module,
                        message: "the recovery original is unreadable and is retained".into(),
                    });
                }
            };
            members.push(Entry { module, before, candidate: candidate.clone() });
        }
        let record = Self {
            recovery_version: 1,
            transaction_id: format!("recovery-{}", Ulid::new()),
            created_at: UtcTimestamp::now(),
            entries: members,
            superseded,
        };
        let directory = config_bundle_transaction_dir(paths);
        fs::create_dir_all(&directory).map_err(|source| SettingsFacadeError::Io {
            action: "create recovery responsibility directory",
            path: directory.clone(),
            source,
        })?;
        let path = directory.join(format!("{}.json", record.transaction_id));
        let bytes = serde_json::to_vec(&record)
            .map_err(|source| SettingsFacadeError::BundleJournal { path: path.clone(), source })?;
        let temporary = path.with_extension("new");
        let mut file =
            OpenOptions::new().write(true).create_new(true).open(&temporary).map_err(|source| {
                SettingsFacadeError::Io {
                    action: "create recovery responsibility",
                    path: path.clone(),
                    source,
                }
            })?;
        file.write_all(&bytes).and_then(|()| file.sync_all()).map_err(|source| {
            SettingsFacadeError::Io {
                action: "flush recovery responsibility",
                path: path.clone(),
                source,
            }
        })?;
        drop(file);
        let live = recovery_reset::LiveRecord::register(path.clone());
        fs::rename(&temporary, &path).map_err(|source| SettingsFacadeError::Io {
            action: "publish recovery responsibility",
            path,
            source,
        })?;
        Ok(live)
    }

    pub fn decode(bytes: &[u8], path: &Path) -> Result<Option<Self>, SettingsFacadeError> {
        let value: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|source| SettingsFacadeError::BundleJournal { path: path.into(), source })?;
        if value.get("recovery_version").is_none() {
            return Ok(None);
        }
        let record: Self = serde_json::from_value(value)
            .map_err(|source| SettingsFacadeError::BundleJournal { path: path.into(), source })?;
        if record.recovery_version != 1 || record.entries.is_empty() {
            return Err(SettingsFacadeError::BundleUnresolved {
                transaction_id: record.transaction_id,
            });
        }
        Ok(Some(record))
    }

    // The existing decoder validates and assigns the candidate set in one place.
    // Before-revision is intentionally unused for this private format: a corrupt
    // original has no revision; native identity + content binding governs recovery.
    pub fn candidate_set(&self) -> PreparedConfigBundle {
        PreparedConfigBundle {
            transaction_id: self.transaction_id.clone(),
            created_at: self.created_at,
            entries: self
                .entries
                .iter()
                .map(|entry| PreparedConfigBundleEntry {
                    module: entry.module,
                    before_revision: 0,
                    before_content_hash: match &entry.before {
                        Before::Present { hash, .. } => hash.clone(),
                        Before::Absent => String::new(),
                    },
                    candidate: entry.candidate.clone(),
                })
                .collect(),
        }
    }

    pub fn superseded(&self) -> &[recovery_reset::JournalBinding] {
        &self.superseded
    }

    pub fn converge(&self, paths: &AuraPaths) -> Result<(), SettingsFacadeError> {
        let _originals = recovery_reset::hold_originals(paths, &self.superseded)?;
        let mut inputs = Vec::new();
        // Judge every input before touching any. Candidate bytes already on disk
        // are legitimate completed members; otherwise the exact before must hold.
        for entry in &self.entries {
            let candidate = encode(entry.module, &entry.candidate)?;
            let current = Input::capture_path(&paths.config_path(entry.module));
            let complete = matches!(&current, Input::Present { bytes, .. } if *bytes == candidate);
            let matches_before = match (&entry.before, &current) {
                (Before::Absent, Input::Absent) => true,
                (Before::Present { volume, file_id, hash }, Input::Present { identity, bytes }) => {
                    identity
                        == &NativeFileIdentity {
                            volume_serial_number: volume.parse().map_err(|_| {
                                SettingsFacadeError::BundleRecoveryConflict { module: entry.module }
                            })?,
                            file_id: *file_id,
                        }
                        && ContentHash::blake3(bytes).to_string() == *hash
                }
                _ => false,
            };
            if !complete && !matches_before {
                return Err(SettingsFacadeError::BundleRecoveryConflict { module: entry.module });
            }
            inputs.push((entry.module, current, candidate, complete));
        }
        let mut published = Vec::new();
        for (module, input, candidate, complete) in &inputs {
            if *complete {
                continue;
            }
            match input.publish(&paths.config_path(*module), candidate) {
                Ok(file) => published.push(file),
                Err(primary) => {
                    // Abort only exact objects created by this convergence. The
                    // durable record survives either way, so no partial Ready.
                    for file in published.into_iter().rev() {
                        let _ = file.abort();
                    }
                    return Err(primary);
                }
            }
        }
        for file in published {
            file.commit().map_err(|error| {
                recovery_publication::publication_error(
                    &config_bundle_transaction_dir(paths),
                    error,
                )
            })?;
        }
        Ok(())
    }
}

pub(super) fn uncertain(error: &SettingsFacadeError) -> bool {
    match error {
        SettingsFacadeError::Io { source, .. } => source
            .get_ref()
            .and_then(|error| error.downcast_ref::<aura_windows_platform::HandlePublishError>())
            .is_some_and(|error| {
                error.location() == aura_windows_platform::PublishLocation::Unconfirmable
            }),
        _ => false,
    }
}

pub(super) fn record_joint(
    facade: &SettingsFacade,
    journal: &Path,
) -> Result<(), SettingsFacadeError> {
    let bytes = read_internal_file(journal)?;
    let Some(record) = Record::decode(&bytes, journal)? else {
        return Ok(());
    };
    let prepared = record.candidate_set();
    let mut updated = BTreeMap::new();
    for entry in &prepared.entries {
        let bytes = encode(entry.module, &entry.candidate)?;
        if read_config_file(entry.module, &facade.paths.config_path(entry.module))? != bytes {
            return Err(SettingsFacadeError::BundleRecoveryConflict { module: entry.module });
        }
        updated.insert(
            entry.module,
            Arc::new(snapshot(entry.module, entry.candidate.clone(), &bytes, ConfigSource::File)),
        );
    }
    facade.write_joint_commit_record(&prepared, &updated)?;
    recovery_reset::archive(&facade.paths, record.superseded())
}

#[cfg(test)]
mod tests;
