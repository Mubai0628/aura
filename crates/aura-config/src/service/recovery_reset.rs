//! Explicit whole-held-set reset and preservation of superseded responsibility.
//! Invalid journals are moved by verified handles only after the confirmed set lands.
use super::*;
use aura_windows_platform::{
    handle_bound_move_existing, open_held_for_move, open_verified_publish_parent,
};
use std::{ops::Deref, sync::OnceLock};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct JournalBinding {
    name: String,
    volume: String,
    file_id: [u8; 16],
    hash: String,
}
impl JournalBinding {
    fn matches(&self, input: &recovery_publication::Input) -> bool {
        matches!(input, recovery_publication::Input::Present { identity, bytes }
            if identity.volume_serial_number.to_string() == self.volume
                && identity.file_id == self.file_id
                && ContentHash::blake3(bytes).to_string() == self.hash)
    }
    fn valid_name(&self) -> bool {
        let path = Path::new(&self.name);
        path.components().count() == 1
            && matches!(path.components().next(), Some(std::path::Component::Normal(_)))
            && path.extension().is_some_and(|s| s == "json")
    }
}
fn active() -> &'static Mutex<BTreeSet<PathBuf>> {
    static ACTIVE: OnceLock<Mutex<BTreeSet<PathBuf>>> = OnceLock::new();
    ACTIVE.get_or_init(Mutex::default)
}
pub(super) struct LiveRecord(PathBuf);
impl LiveRecord {
    pub fn register(path: PathBuf) -> Self {
        active().lock().unwrap_or_else(|e| e.into_inner()).insert(path.clone());
        Self(path)
    }
}
impl Deref for LiveRecord {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}
impl Drop for LiveRecord {
    fn drop(&mut self) {
        active().lock().unwrap_or_else(|e| e.into_inner()).remove(&self.0);
    }
}
pub fn is_live(path: &Path) -> bool {
    active().lock().map_or(true, |paths| paths.contains(path))
}

pub fn superseded(
    facade: &SettingsFacade,
    modules: &[ConfigModule],
) -> Result<Option<Vec<JournalBinding>>, SettingsFacadeError> {
    let disposition = facade
        .recovery
        .lock()
        .map_err(|_| SettingsFacadeError::LockPoisoned("config recovery disposition"))?
        .clone();
    if disposition.held.is_empty() {
        return Ok(None);
    }
    let requested: BTreeSet<_> = modules.iter().copied().collect();
    let required: BTreeSet<_> = if disposition.scope_known {
        disposition.held.iter().copied().collect()
    } else {
        ConfigModule::ALL.into_iter().collect()
    };
    if !required.is_subset(&requested) {
        return Ok(None);
    }
    if facade
        .transactions
        .lock()
        .map_err(|_| SettingsFacadeError::LockPoisoned("config transactions"))?
        .conflict_for(modules)
        .is_some()
    {
        return Ok(None);
    }
    let mut facts = Vec::new();
    for path in scan_bundle_journals(&config_bundle_transaction_dir(&facade.paths))? {
        if is_live(&path) {
            return Ok(None);
        }
        let recovery_publication::Input::Present { identity, bytes } =
            recovery_publication::Input::capture_path(&path)
        else {
            return Ok(None); // Unreadable is not permission to remove a record.
        };
        facts.push(JournalBinding {
            name: path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| SettingsFacadeError::BundleUnresolved {
                    transaction_id: "unreadable journal name".into(),
                })?
                .to_owned(),
            volume: identity.volume_serial_number.to_string(),
            file_id: identity.file_id,
            hash: ContentHash::blake3(&bytes).to_string(),
        });
    }
    Ok(Some(facts))
}

pub fn preview(base: String, bindings: &[JournalBinding]) -> String {
    let bytes = serde_json::to_vec(&(base, bindings)).expect("closed binding types encode");
    format!("bundle:{}", ContentHash::blake3(&bytes))
}
fn archive_dir(paths: &AuraPaths) -> PathBuf {
    paths.config_transaction_dir.join("recovery-originals")
}
pub fn verify(paths: &AuraPaths, bindings: &[JournalBinding]) -> Result<(), SettingsFacadeError> {
    for binding in bindings {
        if !binding.valid_name() {
            return Err(SettingsFacadeError::BundleUnresolved {
                transaction_id: "invalid recovery journal name".into(),
            });
        }
        let path = config_bundle_transaction_dir(paths).join(&binding.name);
        let input = recovery_publication::Input::capture_path(&path);
        let matches = if matches!(input, recovery_publication::Input::Absent) {
            binding.matches(&recovery_publication::Input::capture_path(
                &archive_dir(paths).join(&binding.name),
            ))
        } else {
            binding.matches(&input)
        };
        if !matches {
            return Err(SettingsFacadeError::BundleUnresolved {
                transaction_id: binding.name.clone(),
            });
        }
    }
    Ok(())
}
pub fn hold_originals(
    paths: &AuraPaths,
    bindings: &[JournalBinding],
) -> Result<Vec<aura_windows_platform::VerifiedFile>, SettingsFacadeError> {
    verify(paths, bindings)?;
    let mut held_set = Vec::new();
    for binding in bindings {
        let live = config_bundle_transaction_dir(paths).join(&binding.name);
        let path = match recovery_publication::Input::capture_path(&live) {
            recovery_publication::Input::Absent => archive_dir(paths).join(&binding.name),
            _ => live,
        };
        let held = open_held_for_move(&path).map_err(|source| SettingsFacadeError::Io {
            action: "hold confirmed recovery responsibility",
            path: path.clone(),
            source,
        })?;
        let mut bytes = Vec::new();
        held.file()
            .try_clone()
            .and_then(|file| file.take(SETTINGS_FILE_MAX_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|source| SettingsFacadeError::Io {
                action: "read confirmed recovery responsibility",
                path,
                source,
            })?;
        if !binding.matches(&recovery_publication::Input::Present {
            identity: held.info().identity.clone(),
            bytes,
        }) {
            return Err(SettingsFacadeError::BundleUnresolved {
                transaction_id: binding.name.clone(),
            });
        }
        held_set.push(held);
    }
    Ok(held_set)
}
pub fn archive(paths: &AuraPaths, bindings: &[JournalBinding]) -> Result<(), SettingsFacadeError> {
    verify(paths, bindings)?;
    if bindings.is_empty() {
        return Ok(());
    }
    let directory = archive_dir(paths);
    fs::create_dir_all(&directory).map_err(|source| SettingsFacadeError::Io {
        action: "create recovery originals directory",
        path: directory.clone(),
        source,
    })?;
    let parent =
        open_verified_publish_parent(&directory).map_err(|source| SettingsFacadeError::Io {
            action: "hold recovery originals directory",
            path: directory.clone(),
            source,
        })?;
    for binding in bindings {
        let path = config_bundle_transaction_dir(paths).join(&binding.name);
        let held = match open_held_for_move(&path) {
            Ok(held) => held,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(SettingsFacadeError::Io {
                    action: "hold original recovery journal",
                    path,
                    source,
                });
            }
        };
        let mut bytes = Vec::new();
        held.file()
            .try_clone()
            .and_then(|file| file.take(SETTINGS_FILE_MAX_BYTES + 1).read_to_end(&mut bytes))
            .map_err(|source| SettingsFacadeError::Io {
                action: "read held recovery journal",
                path: path.clone(),
                source,
            })?;
        if !binding.matches(&recovery_publication::Input::Present {
            identity: held.info().identity.clone(),
            bytes,
        }) {
            return Err(SettingsFacadeError::BundleUnresolved {
                transaction_id: binding.name.clone(),
            });
        }
        handle_bound_move_existing(&held, &parent, std::ffi::OsStr::new(&binding.name))
            .map_err(|error| recovery_publication::publication_error(&path, error))?;
    }
    Ok(())
}

// A valid confirmed reset takes over exactly the records whose identity/content it
// bound. Unknown or edited records continue to impose protection.
pub fn superseded_paths(
    paths: &AuraPaths,
    validators: &BTreeMap<ConfigModule, Arc<dyn ModuleConfigValidator>>,
    journals: &[PathBuf],
) -> BTreeSet<PathBuf> {
    let mut hidden = BTreeSet::new();
    for path in journals {
        if is_live(path) {
            continue;
        }
        let Ok(bytes) = read_internal_file(path) else {
            continue;
        };
        let Ok(Some(record)) = recovery_journal::Record::decode(&bytes, path) else {
            continue;
        };
        if read_bundle_journal_domains(paths, validators, path).is_err()
            || verify(paths, record.superseded()).is_err()
        {
            continue;
        }
        for binding in record.superseded() {
            let original = config_bundle_transaction_dir(paths).join(&binding.name);
            if original != *path {
                hidden.insert(original);
            }
        }
    }
    hidden
}
