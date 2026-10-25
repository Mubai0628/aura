//! History is one resource shared by facades, not one resource per facade.
//! Serialize the comparison and publication by the directory's physical identity.
//! This coordinates Aura writers in this process; arbitrary external editors do
//! not participate. Native replacement guards and errors remain in force.
use super::{
    ConfigModule, SettingsFacadeError, parse_document, prune_history, read_config_file,
    write_history_file,
};
use aura_windows_platform::open_verified_publish_parent;
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    sync::{Arc, Mutex, OnceLock, Weak},
};

fn owner(path: &Path) -> Result<Arc<Mutex<()>>, SettingsFacadeError> {
    let parent = open_verified_publish_parent(path)
        .map_err(|source| io_error(path, "bind history owner", source))?;
    let identity = &parent.info().identity;
    let key = format!("{}:{:?}", identity.volume_serial_number, identity.file_id);
    static OWNERS: OnceLock<Mutex<BTreeMap<String, Weak<Mutex<()>>>>> = OnceLock::new();
    let mut owners = OWNERS.get_or_init(Mutex::default).lock().map_err(|_| {
        io_error(path, "claim history owner", std::io::Error::other("history owner unavailable"))
    })?;
    owners.retain(|_, entry| entry.strong_count() > 0);
    let owner =
        owners.get(&key).and_then(Weak::upgrade).unwrap_or_else(|| Arc::new(Mutex::new(())));
    owners.insert(key, Arc::downgrade(&owner));
    Ok(owner)
}

pub(super) fn record(
    history_dir: &Path,
    module: ConfigModule,
    revision: u64,
    bytes: &[u8],
) -> Result<(), SettingsFacadeError> {
    record_observed(history_dir, module, revision, bytes, || {})
}

fn record_observed(
    history_dir: &Path,
    module: ConfigModule,
    revision: u64,
    bytes: &[u8],
    compared: impl FnOnce(),
) -> Result<(), SettingsFacadeError> {
    fs::create_dir_all(history_dir)
        .map_err(|source| io_error(history_dir, "create history directory", source))?;
    let owner = owner(history_dir)?;
    let _guard = owner.lock().map_err(|_| {
        io_error(
            history_dir,
            "hold history owner",
            std::io::Error::other("history owner unavailable"),
        )
    })?;
    // This entry is immutable. CREATE_NEW is the authority; exists() is not a reservation.
    let version = history_dir.join(format!(
        "version-{}-r{revision}.{}",
        ulid::Ulid::new(),
        module.extension()
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&version)
        .map_err(|source| io_error(&version, "create history version", source))?;
    file.write_all(bytes)
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all())
        .map_err(|source| io_error(&version, "flush history version", source))?;
    drop(file);

    let lkg = history_dir.join(format!("last-known-good.{}", module.extension()));
    match read_config_file(module, &lkg) {
        Ok(current) if current == bytes => return prune_history(history_dir),
        Ok(current) => {
            if parse_document(module, &current).is_ok_and(|doc| doc.revision > revision) {
                return prune_history(history_dir);
            }
        }
        Err(SettingsFacadeError::Io { source, .. })
            if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error), // unreadable is not absent
    }
    compared(); // exact internal window, no global test switch
    write_history_file(&lkg, bytes)?;
    prune_history(history_dir)
}

fn io_error(path: &Path, action: &'static str, source: std::io::Error) -> SettingsFacadeError {
    SettingsFacadeError::Io { action, path: path.to_owned(), source }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};
    fn bytes(revision: u64) -> Vec<u8> {
        let mut doc = crate::ModuleConfigDocument::empty();
        doc.revision = revision;
        crate::encode_document(ConfigModule::Tools, &doc)
            .expect("obtain the actual isolated configuration fact")
    }

    #[test]
    fn newer_writer_cannot_enter_the_real_compare_publish_window() {
        let dir = tempfile::tempdir().expect("obtain the actual isolated configuration fact");
        record(dir.path(), ConfigModule::Tools, 1, &bytes(1))
            .expect("obtain the actual isolated configuration fact");
        let (arrived_tx, arrived_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let path = dir.path().to_owned();
        let old = std::thread::spawn(move || {
            record_observed(&path, ConfigModule::Tools, 2, &bytes(2), || {
                arrived_tx.send(()).expect("obtain the actual isolated configuration fact");
                release_rx
                    .recv_timeout(Duration::from_secs(5))
                    .expect("obtain the actual isolated configuration fact");
            })
        });
        arrived_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("obtain the actual isolated configuration fact");
        let competing_owner =
            owner(dir.path()).expect("obtain the actual isolated configuration fact");
        assert!(
            competing_owner.try_lock().is_err(),
            "same physical history must coordinate through publication"
        );
        let path = dir.path().to_owned();
        let newer = std::thread::spawn(move || record(&path, ConfigModule::Tools, 99, &bytes(99)));
        release_tx.send(()).expect("obtain the actual isolated configuration fact");
        old.join()
            .expect("obtain the actual isolated configuration fact")
            .expect("obtain the actual isolated configuration fact");
        newer
            .join()
            .expect("obtain the actual isolated configuration fact")
            .expect("obtain the actual isolated configuration fact");
        assert_eq!(
            fs::read(dir.path().join("last-known-good.json"))
                .expect("obtain the actual isolated configuration fact"),
            bytes(99)
        );
    }

    #[test]
    fn a_held_history_does_not_serialize_an_unrelated_directory() {
        let a = tempfile::tempdir().expect("obtain the actual isolated configuration fact");
        let b = tempfile::tempdir().expect("obtain the actual isolated configuration fact");
        let first = owner(a.path()).expect("obtain the actual isolated configuration fact");
        let _held = first.lock().expect("obtain the actual isolated configuration fact");
        record(b.path(), ConfigModule::Tools, 4, &bytes(4))
            .expect("obtain the actual isolated configuration fact");
        assert_eq!(
            fs::read(b.path().join("last-known-good.json"))
                .expect("obtain the actual isolated configuration fact"),
            bytes(4)
        );
    }
}
