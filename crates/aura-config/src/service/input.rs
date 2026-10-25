//! Bounded input reads shared by configuration publication and Memory fail-closed checks.
use super::*;

pub(crate) fn read_config_file(
    module: ConfigModule,
    path: &Path,
) -> Result<Vec<u8>, SettingsFacadeError> {
    read_config_file_after_metadata(module, path, || {})
}

pub(super) fn read_config_file_after_metadata(
    module: ConfigModule,
    path: &Path,
    after_metadata: impl FnOnce(),
) -> Result<Vec<u8>, SettingsFacadeError> {
    let file = File::open(path).map_err(|source| SettingsFacadeError::Io {
        action: "open config file",
        path: path.to_path_buf(),
        source,
    })?;
    let metadata = file.metadata().map_err(|source| SettingsFacadeError::Io {
        action: "inspect config file",
        path: path.to_path_buf(),
        source,
    })?;
    ensure_config_size(module, metadata.len())?;
    after_metadata();

    let mut bytes = Vec::with_capacity(
        usize::try_from(metadata.len()).expect("bounded settings file size fits usize"),
    );
    file.take(SETTINGS_FILE_MAX_BYTES + 1).read_to_end(&mut bytes).map_err(|source| {
        SettingsFacadeError::Io { action: "read config file", path: path.to_path_buf(), source }
    })?;
    ensure_config_size(module, bytes.len() as u64)?;
    Ok(bytes)
}

pub(super) fn ensure_config_size(
    module: ConfigModule,
    size: u64,
) -> Result<(), SettingsFacadeError> {
    if size > SETTINGS_FILE_MAX_BYTES {
        return Err(SettingsFacadeError::FileTooLarge {
            module,
            limit_bytes: SETTINGS_FILE_MAX_BYTES,
        });
    }
    Ok(())
}
