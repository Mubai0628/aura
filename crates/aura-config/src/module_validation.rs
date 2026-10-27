use crate::{ConfigModule, ConfigValidationError, ModuleConfigDocument};

pub(crate) fn validate_builtin_module_document(
    module: ConfigModule,
    document: &ModuleConfigDocument,
) -> Result<(), ConfigValidationError> {
    match module {
        ConfigModule::Tools => crate::tools::validate_tools_document(document),
        ConfigModule::Memory => crate::memory::settings_from_document(document).map(|_| ()),
        ConfigModule::Ui => crate::ui::settings_from_document(document).map(|_| ()),
        ConfigModule::Browser => crate::browser::settings_from_document(document).map(|_| ()),
        ConfigModule::Chrome => crate::chrome::settings_from_document(document).map(|_| ()),
        ConfigModule::Prompt => crate::prompt::validate_prompt_document(document),
        ConfigModule::Hooks => crate::hooks::validate_hooks_document(document),
        ConfigModule::Mcp => crate::mcp::validate_mcp_document(document),
        ConfigModule::Skills => crate::skills::validate_skills_document(document),
        ConfigModule::Plugins => crate::plugins::validate_plugins_document(document),
        _ => Ok(()),
    }
}
