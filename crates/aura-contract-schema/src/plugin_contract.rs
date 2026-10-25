//! Plugins: Settings → 插件.
use aura_plugins::*;
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    generator.subschema_for::<PluginLayout>();
    generator.subschema_for::<PluginState>();
    generator.subschema_for::<PluginLimits>();
    generator.subschema_for::<PluginFile>();
    generator.subschema_for::<PluginSkillItem>();
    generator.subschema_for::<PluginCommandItem>();
    generator.subschema_for::<PluginServerKind>();
    generator.subschema_for::<PluginSignIn>();
    generator.subschema_for::<PluginServerItem>();
    generator.subschema_for::<PluginHookItem>();
    generator.subschema_for::<PluginCounts>();
    generator.subschema_for::<PluginPendingView>();
    generator.subschema_for::<PluginView>();
    generator.subschema_for::<PluginRegistryView>();
    generator.subschema_for::<PluginPreview>();
    generator.subschema_for::<PluginListRequest>();
    generator.subschema_for::<PluginRequest>();
    generator.subschema_for::<PluginImportRequest>();
    generator.subschema_for::<PluginTrustRequest>();
    generator.subschema_for::<PluginChangeRequest>();
    generator.subschema_for::<PluginSetEnabledRequest>();
    generator.subschema_for::<PluginSelectDirectoryRequest>();
    generator.subschema_for::<PluginSelectedDirectory>();
    generator.subschema_for::<PluginSelectArchiveRequest>();
    generator.subschema_for::<PluginSelectedArchive>();
    generator.subschema_for::<PluginDirectoryOpened>();
}
