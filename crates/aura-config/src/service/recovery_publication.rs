//! Private recovery material and native conditional publication. No caller can substitute
//! a later history entry or a later file for the content the user confirmed.
use super::*;
use aura_windows_platform::{
    HandleBoundPublishedFile, HandlePublishError, NativeFileIdentity, handle_bound_create_absent,
    handle_bound_replace_checked, open_held_for_read, open_verified_publish_parent,
};

pub(super) struct BoundPlan {
    pub plan: crate::recovery::DomainRecoveryPlan,
    pub original: Input,
    pub restore: Option<ModuleConfigDocument>,
    pub default: Option<ModuleConfigDocument>,
}

impl BoundPlan {
    pub fn candidate(
        &self,
        facade: &SettingsFacade,
        action: crate::recovery::ConfigRecoveryAction,
        joint: Option<&(ConfigModule, u64, String)>,
    ) -> Result<(ModuleConfigDocument, Vec<u8>), crate::recovery::ConfigRecoveryError> {
        use crate::recovery::{ConfigRecoveryAction, ConfigRecoveryError, codes};
        let module = self.plan.module;
        let unavailable = |reason_code| ConfigRecoveryError::Unavailable { module, reason_code };
        let mut document = match action {
            ConfigRecoveryAction::RestorePreviousValid => match joint {
                Some((_, revision, hash)) => {
                    let validator = facade.validator_for(module);
                    let versions = crate::recovery::recorded_versions(&facade.paths, module)
                        .map_err(|source| ConfigRecoveryError::Apply {
                            module,
                            source: Box::new(source),
                        })?;
                    let mut chosen = None;
                    for version in versions.iter().filter(|version| version.revision == *revision) {
                        if let Ok(document) = crate::recovery::validate_recorded_version(
                            module,
                            version,
                            validator.as_ref(),
                        ) && let Ok(bytes) = encode(module, &document)
                            && ContentHash::blake3(&bytes).to_string() == *hash
                        {
                            chosen = Some(document);
                            break;
                        }
                    }
                    chosen.ok_or_else(|| unavailable(codes::NO_CONSISTENT_BUNDLE_VERSION))?
                }
                None => self
                    .restore
                    .clone()
                    .ok_or_else(|| unavailable(codes::NO_VALID_PREVIOUS_VERSION))?,
            },
            ConfigRecoveryAction::ResetToDefault => {
                self.default.clone().ok_or_else(|| unavailable(codes::DEFAULT_FAILS_VALIDATION))?
            }
            ConfigRecoveryAction::Retry => {
                return Err(unavailable(codes::ACTION_NOT_AVAILABLE_FOR_BUNDLE));
            }
        };
        document.revision = self.plan.next_revision;
        let validator = facade.validator_for(module);
        validator
            .validate(module, &document)
            .and_then(|()| validate_builtin_module_document(module, &document))
            .map_err(|source| ConfigRecoveryError::Apply {
                module,
                source: Box::new(SettingsFacadeError::Validation { module, source }),
            })?;
        let bytes = encode(module, &document)
            .map_err(|source| ConfigRecoveryError::Apply { module, source: Box::new(source) })?;
        Ok((document, bytes))
    }
}

pub(super) enum Input {
    Present { identity: NativeFileIdentity, bytes: Vec<u8> },
    Absent,
    Unreadable(String),
}

impl Input {
    pub fn capture(facade: &SettingsFacade, module: ConfigModule) -> Self {
        let path = facade.paths.config_path(module);
        Self::capture_path(&path)
    }

    pub fn capture_path(path: &Path) -> Self {
        let result = (|| {
            let opened = open_held_for_read(path)?;
            if opened.info().is_directory || opened.info().is_reparse_point {
                return Err(std::io::Error::other("configuration input is not a plain file"));
            }
            let mut bytes = Vec::new();
            opened.file().try_clone()?.take(SETTINGS_FILE_MAX_BYTES + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 > SETTINGS_FILE_MAX_BYTES {
                return Err(std::io::Error::other(
                    "configuration input exceeds bounded recovery read",
                ));
            }
            Ok((opened.info().identity.clone(), bytes))
        })();
        match result {
            Ok((identity, bytes)) => Self::Present { identity, bytes },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::Absent,
            Err(error) => {
                Self::Unreadable(format!("{:?}:{:?}", error.kind(), error.raw_os_error()))
            }
        }
    }

    fn binding(&self) -> serde_json::Value {
        match self {
            Self::Present { identity, bytes } => serde_json::json!({
                "state": "present", "volume": identity.volume_serial_number.to_string(),
                "file_id": identity.file_id, "hash": ContentHash::blake3(bytes),
            }),
            Self::Absent => serde_json::json!({"state":"absent"}),
            Self::Unreadable(reason) => serde_json::json!({"state":"unreadable", "reason":reason}),
        }
    }

    pub fn preserve(
        &self,
        facade: &SettingsFacade,
        module: ConfigModule,
    ) -> Result<(), SettingsFacadeError> {
        let Self::Present { bytes, .. } = self else {
            return match self {
                Self::Absent => Ok(()),
                _ => Err(io_error(
                    &facade.paths.config_path(module),
                    "bind recovery input",
                    "the original input cannot be read; it is retained",
                )),
            };
        };
        let contains_secret = parse_document(module, bytes).map_or_else(
            |_| contains_secret_field_marker(bytes),
            |document| document_contains_secret_fields(&document),
        );
        // Existing confidentiality policy: never export a secret-bearing original to plain history.
        if contains_secret {
            return Ok(());
        }
        let directory = facade.paths.history_path(module).join("invalid");
        fs::create_dir_all(&directory).map_err(|source| SettingsFacadeError::Io {
            action: "create recovery original directory",
            path: directory.clone(),
            source,
        })?;
        let path = directory.join(format!("invalid-{}.{}", Ulid::new(), module.extension()));
        let mut file =
            OpenOptions::new().write(true).create_new(true).open(&path).map_err(|source| {
                SettingsFacadeError::Io {
                    action: "preserve recovery original",
                    path: path.clone(),
                    source,
                }
            })?;
        file.write_all(bytes).and_then(|()| file.flush()).and_then(|()| file.sync_all()).map_err(
            |source| SettingsFacadeError::Io { action: "flush recovery original", path, source },
        )
    }

    pub fn publish(
        &self,
        path: &Path,
        bytes: &[u8],
    ) -> Result<HandleBoundPublishedFile, SettingsFacadeError> {
        let parent = open_verified_publish_parent(
            path.parent()
                .ok_or_else(|| io_error(path, "bind recovery parent", "parent is missing"))?,
        )
        .map_err(|source| SettingsFacadeError::Io {
            action: "bind recovery parent",
            path: path.into(),
            source,
        })?;
        let leaf = path
            .file_name()
            .ok_or_else(|| io_error(path, "bind recovery file", "name is missing"))?;
        let result = match self {
            Self::Present { identity, bytes: original } => handle_bound_replace_checked(
                &parent,
                leaf,
                identity,
                &ContentHash::blake3(original).hex,
                bytes,
            ),
            Self::Absent => handle_bound_create_absent(&parent, leaf, bytes),
            Self::Unreadable(_) => {
                return Err(io_error(
                    path,
                    "publish recovery",
                    "the original input is unconfirmed; no replacement attempted",
                ));
            }
        };
        result.map_err(|error| publication_error(path, error))
    }

    pub fn restore_after(
        &self,
        path: &Path,
        published: &NativeFileIdentity,
        bytes: &[u8],
    ) -> Result<(), SettingsFacadeError> {
        match self {
            Self::Present { bytes: original, .. } => {
                let parent = open_verified_publish_parent(
                    path.parent().expect("managed config has a parent"),
                )
                .map_err(|source| SettingsFacadeError::Io {
                    action: "bind recovery compensation",
                    path: path.into(),
                    source,
                })?;
                handle_bound_replace_checked(
                    &parent,
                    path.file_name().expect("managed config has a name"),
                    published,
                    &ContentHash::blake3(bytes).hex,
                    original,
                )
                .and_then(|file| file.commit())
                .map(|_| ())
                .map_err(|error| publication_error(path, error))
            }
            Self::Absent => {
                let held = aura_windows_platform::open_held_for_move(path).map_err(|source| {
                    SettingsFacadeError::Io {
                        action: "hold created recovery file",
                        path: path.into(),
                        source,
                    }
                })?;
                let mut actual = Vec::new();
                held.file()
                    .try_clone()
                    .and_then(|file| {
                        file.take(SETTINGS_FILE_MAX_BYTES + 1).read_to_end(&mut actual)
                    })
                    .map_err(|source| SettingsFacadeError::Io {
                        action: "read created recovery file",
                        path: path.into(),
                        source,
                    })?;
                if held.info().identity != *published || actual != bytes {
                    return Err(io_error(
                        path,
                        "compensate recovery",
                        "a competitor changed the publication; it is retained",
                    ));
                }
                aura_windows_platform::handle_bound_delete(held.file())
                    .map_err(|error| publication_error(path, error))
            }
            Self::Unreadable(_) => {
                Err(io_error(path, "compensate recovery", "original was never confirmed"))
            }
        }
    }
}

pub(super) fn preview_identity(
    module: ConfigModule,
    original: &Input,
    health: &DomainHealth,
    committed: Option<u64>,
    options: &[crate::recovery::RecoveryOption],
    restore: Option<&ModuleConfigDocument>,
    default: Option<&ModuleConfigDocument>,
) -> Result<String, SettingsFacadeError> {
    let target_hash = |document: Option<&ModuleConfigDocument>| -> Result<Option<ContentHash>, SettingsFacadeError> {
        document.map(|document| encode(module, document).map(|bytes| ContentHash::blake3(&bytes))).transpose()
    };
    let facts = serde_json::json!({"version":2, "module":module.key(), "original":original.binding(),
        "revision":health.revision,"committed":committed,"options":options,
        "restore":target_hash(restore)?,"default":target_hash(default)?});
    let bytes = aura_domain_common::jcs::canonical_json_jcs(&facts)
        .map_err(|error| io_error(Path::new(module.key()), "encode recovery binding", error))?;
    Ok(ContentHash::blake3(&bytes).to_string())
}

pub(super) fn publication_error(path: &Path, error: HandlePublishError) -> SettingsFacadeError {
    SettingsFacadeError::Io {
        action: "publish bound recovery",
        path: path.into(),
        source: std::io::Error::other(error),
    }
}
fn io_error(
    path: &Path,
    action: &'static str,
    error: impl std::fmt::Display,
) -> SettingsFacadeError {
    SettingsFacadeError::Io {
        action,
        path: path.into(),
        source: std::io::Error::other(error.to_string()),
    }
}
