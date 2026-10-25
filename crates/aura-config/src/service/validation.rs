use super::*;

pub(super) fn parse_and_validate(
    module: ConfigModule,
    bytes: &[u8],
    validator: &dyn ModuleConfigValidator,
) -> Result<ModuleConfigDocument, SettingsFacadeError> {
    let document = parse_document(module, bytes)
        .map_err(|source| SettingsFacadeError::Codec { module, source })?;
    validate_base_document(&document)
        .map_err(|source| SettingsFacadeError::Validation { module, source })?;
    validate_builtin_module_document(module, &document)
        .map_err(|source| SettingsFacadeError::Validation { module, source })?;
    validator
        .validate(module, &document)
        .map_err(|source| SettingsFacadeError::Validation { module, source })?;
    Ok(document)
}
