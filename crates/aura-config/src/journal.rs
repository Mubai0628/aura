use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
};

use aura_domain_common::{CredentialRef, UtcTimestamp};
use aura_windows_platform::atomic_replace_file;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use ulid::Ulid;

use crate::{AuraPaths, ConfigModule};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfigTransactionState {
    Prepared,
    RegistryCommitted,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConfigTransactionTarget {
    pub owned_paths: Vec<PathBuf>,
    pub credential_refs: Vec<CredentialRef>,
    pub registry_modules: Vec<ConfigModule>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ConfigTransactionRecord {
    transaction_id: String,
    label: String,
    state: ConfigTransactionState,
    target: ConfigTransactionTarget,
    created_at: UtcTimestamp,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConfigTransactionRecoveryAction {
    CleanedPrepared { transaction_id: String },
    CompletedCommit { transaction_id: String },
}

pub trait ConfigTransactionRecovery {
    type Error: std::error::Error + Send + Sync + 'static;

    fn cleanup_prepared(
        &mut self,
        transaction_id: &str,
        target: &ConfigTransactionTarget,
    ) -> Result<(), Self::Error>;

    fn complete_committed(
        &mut self,
        transaction_id: &str,
        target: &ConfigTransactionTarget,
    ) -> Result<(), Self::Error>;
}

#[derive(Debug, Error)]
pub enum ConfigTransactionError {
    #[error("config transaction path is outside Aura managed storage: {0}")]
    PathOutsideManagedStorage(PathBuf),
    #[error("failed to {action} config transaction journal {path}: {source}")]
    Io { action: &'static str, path: PathBuf, source: std::io::Error },
    #[error("config transaction journal is invalid at {path}: {source}")]
    Invalid { path: PathBuf, source: serde_json::Error },
    #[error("config transaction recovery failed for {transaction_id}: {message}")]
    Recovery { transaction_id: String, message: String },
    #[error("config transaction handle is already finished")]
    AlreadyFinished,
}

#[derive(Clone, Debug)]
pub struct ConfigTransactionJournal {
    journal_root: PathBuf,
    local_root: PathBuf,
}

impl ConfigTransactionJournal {
    #[must_use]
    pub fn new(paths: &AuraPaths) -> Self {
        Self {
            journal_root: paths.config_transaction_dir.clone(),
            local_root: paths.local_root.clone(),
        }
    }

    pub fn prepare(
        &self,
        label: impl Into<String>,
        target: ConfigTransactionTarget,
    ) -> Result<ConfigTransactionHandle, ConfigTransactionError> {
        self.validate_target(&target)?;
        fs::create_dir_all(&self.journal_root).map_err(|source| ConfigTransactionError::Io {
            action: "create",
            path: self.journal_root.clone(),
            source,
        })?;
        let record = ConfigTransactionRecord {
            transaction_id: format!("ctx_{}", Ulid::new()),
            label: label.into(),
            state: ConfigTransactionState::Prepared,
            target,
            created_at: UtcTimestamp::now(),
        };
        let path = self.journal_root.join(format!("{}.json", record.transaction_id));
        write_record(&path, &record)?;
        Ok(ConfigTransactionHandle { path: Some(path), record })
    }

    pub fn recover_with<R>(
        &self,
        recovery: &mut R,
    ) -> Result<Vec<ConfigTransactionRecoveryAction>, ConfigTransactionError>
    where
        R: ConfigTransactionRecovery,
    {
        fs::create_dir_all(&self.journal_root).map_err(|source| ConfigTransactionError::Io {
            action: "create",
            path: self.journal_root.clone(),
            source,
        })?;
        let mut paths = fs::read_dir(&self.journal_root)
            .map_err(|source| ConfigTransactionError::Io {
                action: "scan",
                path: self.journal_root.clone(),
                source,
            })?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect::<Result<Vec<_>, std::io::Error>>()
            .map_err(|source| ConfigTransactionError::Io {
                action: "read scanned entry",
                path: self.journal_root.clone(),
                source,
            })?
            .into_iter()
            .filter(|path| path.extension().is_some_and(|extension| extension == "json"))
            .collect::<Vec<_>>();
        paths.sort();
        let mut actions = Vec::new();
        for path in paths {
            let bytes = fs::read(&path).map_err(|source| ConfigTransactionError::Io {
                action: "read",
                path: path.clone(),
                source,
            })?;
            let record: ConfigTransactionRecord = serde_json::from_slice(&bytes)
                .map_err(|source| ConfigTransactionError::Invalid { path: path.clone(), source })?;
            self.validate_target(&record.target)?;
            let action = match record.state {
                ConfigTransactionState::Prepared => {
                    recovery.cleanup_prepared(&record.transaction_id, &record.target).map_err(
                        |error| ConfigTransactionError::Recovery {
                            transaction_id: record.transaction_id.clone(),
                            message: error.to_string(),
                        },
                    )?;
                    ConfigTransactionRecoveryAction::CleanedPrepared {
                        transaction_id: record.transaction_id.clone(),
                    }
                }
                ConfigTransactionState::RegistryCommitted => {
                    recovery.complete_committed(&record.transaction_id, &record.target).map_err(
                        |error| ConfigTransactionError::Recovery {
                            transaction_id: record.transaction_id.clone(),
                            message: error.to_string(),
                        },
                    )?;
                    ConfigTransactionRecoveryAction::CompletedCommit {
                        transaction_id: record.transaction_id.clone(),
                    }
                }
            };
            fs::remove_file(&path).map_err(|source| ConfigTransactionError::Io {
                action: "remove recovered",
                path,
                source,
            })?;
            actions.push(action);
        }
        Ok(actions)
    }

    fn validate_target(
        &self,
        target: &ConfigTransactionTarget,
    ) -> Result<(), ConfigTransactionError> {
        for path in &target.owned_paths {
            if !path.is_absolute()
                || !path.starts_with(&self.local_root)
                || path
                    .components()
                    .any(|component| matches!(component, Component::ParentDir | Component::CurDir))
            {
                return Err(ConfigTransactionError::PathOutsideManagedStorage(path.clone()));
            }
        }
        Ok(())
    }
}

pub struct ConfigTransactionHandle {
    path: Option<PathBuf>,
    record: ConfigTransactionRecord,
}

impl ConfigTransactionHandle {
    #[must_use]
    pub fn transaction_id(&self) -> &str {
        &self.record.transaction_id
    }

    #[must_use]
    pub const fn state(&self) -> ConfigTransactionState {
        self.record.state
    }

    pub fn mark_registry_committed(&mut self) -> Result<(), ConfigTransactionError> {
        let path = self.path.as_ref().ok_or(ConfigTransactionError::AlreadyFinished)?;
        self.record.state = ConfigTransactionState::RegistryCommitted;
        write_record(path, &self.record)
    }

    pub fn finish(mut self) -> Result<(), ConfigTransactionError> {
        let path = self.path.take().ok_or(ConfigTransactionError::AlreadyFinished)?;
        fs::remove_file(&path).map_err(|source| ConfigTransactionError::Io {
            action: "finish",
            path,
            source,
        })
    }
}

fn write_record(
    path: &Path,
    record: &ConfigTransactionRecord,
) -> Result<(), ConfigTransactionError> {
    let parent = path.parent().ok_or_else(|| ConfigTransactionError::Io {
        action: "resolve parent for",
        path: path.to_path_buf(),
        source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing parent"),
    })?;
    fs::create_dir_all(parent).map_err(|source| ConfigTransactionError::Io {
        action: "create parent for",
        path: path.to_path_buf(),
        source,
    })?;
    let temporary = parent.join(format!(".journal-{}.tmp", Ulid::new()));
    let mut bytes = serde_json::to_vec_pretty(record)
        .map_err(|source| ConfigTransactionError::Invalid { path: path.to_path_buf(), source })?;
    bytes.push(b'\n');
    let mut file =
        OpenOptions::new().create_new(true).write(true).open(&temporary).map_err(|source| {
            ConfigTransactionError::Io {
                action: "create temporary",
                path: temporary.clone(),
                source,
            }
        })?;
    file.write_all(&bytes).and_then(|()| file.flush()).and_then(|()| file.sync_all()).map_err(
        |source| ConfigTransactionError::Io {
            action: "flush temporary",
            path: temporary.clone(),
            source,
        },
    )?;
    drop(file);
    atomic_replace_file(&temporary, path).map_err(|source| ConfigTransactionError::Io {
        action: "atomically replace",
        path: path.to_path_buf(),
        source: std::io::Error::other(source),
    })
}

#[cfg(test)]
mod tests {
    use std::{convert::Infallible, path::PathBuf};

    use tempfile::tempdir;

    use crate::AuraPaths;

    use super::{
        ConfigTransactionJournal, ConfigTransactionRecovery, ConfigTransactionRecoveryAction,
        ConfigTransactionTarget,
    };

    #[derive(Default)]
    struct Recorder {
        cleaned: Vec<String>,
        completed: Vec<String>,
    }

    impl ConfigTransactionRecovery for Recorder {
        type Error = Infallible;

        fn cleanup_prepared(
            &mut self,
            transaction_id: &str,
            _target: &ConfigTransactionTarget,
        ) -> Result<(), Self::Error> {
            self.cleaned.push(transaction_id.to_owned());
            Ok(())
        }

        fn complete_committed(
            &mut self,
            transaction_id: &str,
            _target: &ConfigTransactionTarget,
        ) -> Result<(), Self::Error> {
            self.completed.push(transaction_id.to_owned());
            Ok(())
        }
    }

    #[test]
    fn recovers_both_sides_of_the_registry_commit_boundary() {
        let root = tempdir().expect("temp root");
        let paths = AuraPaths::from_roots(root.path().join("roaming"), root.path().join("local"));
        paths.ensure_layout().expect("layout");
        let journal = ConfigTransactionJournal::new(&paths);
        let target = ConfigTransactionTarget {
            owned_paths: vec![paths.local_root.join("plugins/example")],
            credential_refs: Vec::new(),
            registry_modules: Vec::new(),
        };
        let prepared =
            journal.prepare("before-registry", target.clone()).expect("prepared transaction");
        let prepared_id = prepared.transaction_id().to_owned();
        std::mem::forget(prepared);
        let mut committed =
            journal.prepare("after-registry", target).expect("committed transaction");
        let committed_id = committed.transaction_id().to_owned();
        committed.mark_registry_committed().expect("registry committed marker");
        std::mem::forget(committed);

        let mut recorder = Recorder::default();
        let actions = journal.recover_with(&mut recorder).expect("recover");
        assert!(actions.contains(&ConfigTransactionRecoveryAction::CleanedPrepared {
            transaction_id: prepared_id.clone()
        }));
        assert!(actions.contains(&ConfigTransactionRecoveryAction::CompletedCommit {
            transaction_id: committed_id.clone()
        }));
        assert_eq!(recorder.cleaned, vec![prepared_id]);
        assert_eq!(recorder.completed, vec![committed_id]);
    }

    #[test]
    fn rejects_paths_outside_managed_storage() {
        let root = tempdir().expect("temp root");
        let paths = AuraPaths::from_roots(root.path().join("roaming"), root.path().join("local"));
        paths.ensure_layout().expect("layout");
        let result = ConfigTransactionJournal::new(&paths).prepare(
            "unsafe",
            ConfigTransactionTarget {
                owned_paths: vec![PathBuf::from(r"C:\outside")],
                credential_refs: Vec::new(),
                registry_modules: Vec::new(),
            },
        );
        assert!(result.is_err());
    }
}
