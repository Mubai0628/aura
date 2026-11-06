//! The `computer.*` tools and the routes of computer control: programs on the user's own
//! desktop.
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    generator.subschema_for::<aura_desktop_domain::ComputerAppsInput>();
    generator.subschema_for::<aura_desktop_domain::ComputerAppsResult>();
    generator.subschema_for::<aura_desktop_domain::ComputerOpenInput>();
    generator.subschema_for::<aura_desktop_domain::ComputerOpenResult>();
    generator.subschema_for::<aura_desktop_domain::ComputerSnapshotInput>();
    generator.subschema_for::<aura_desktop_domain::ComputerSnapshotResult>();
    generator.subschema_for::<aura_desktop_domain::ComputerScreenshotInput>();
    generator.subschema_for::<aura_desktop_domain::ComputerScreenshotResult>();
    generator.subschema_for::<aura_desktop_domain::ComputerActInput>();
    generator.subschema_for::<aura_desktop_domain::ComputerActResult>();
    generator.subschema_for::<aura_desktop_domain::ComputerInputInput>();
    generator.subschema_for::<aura_desktop_domain::ComputerInputResult>();
    generator.subschema_for::<aura_desktop_domain::ComputerWindowInput>();
    generator.subschema_for::<aura_desktop_domain::ComputerWindowResult>();
    generator.subschema_for::<aura_desktop_domain::ComputerSessionRequest>();
    generator.subschema_for::<aura_desktop_domain::ComputerStopRequest>();
    generator.subschema_for::<aura_desktop_domain::ComputerControllingResult>();
    generator.subschema_for::<aura_desktop_domain::ComputerAlwaysAllowRequest>();
    generator.subschema_for::<aura_desktop_domain::ComputerCatalogRequest>();
    generator.subschema_for::<aura_desktop_domain::ComputerCatalogResult>();
    generator.subschema_for::<aura_desktop_domain::ComputerIconsRequest>();
    generator.subschema_for::<aura_desktop_domain::ComputerIconsResult>();
    generator.subschema_for::<aura_config::ComputerControlSettingsSnapshot>();
    generator.subschema_for::<aura_config::ComputerControlSettingsUpdate>();
}
