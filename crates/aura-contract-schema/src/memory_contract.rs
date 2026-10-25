use aura_memory_domain::*;
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    generator.subschema_for::<MemorySearchInput>();
    generator.subschema_for::<MemorySearchResult>();
    generator.subschema_for::<MemoryReadInput>();
    generator.subschema_for::<MemoryRead>();
    generator.subschema_for::<MemoryRecentInput>();
    generator.subschema_for::<MemoryCreateInput>();
    generator.subschema_for::<MemoryUpdateInput>();
    generator.subschema_for::<MemoryLinkInput>();
    generator.subschema_for::<MemoryStateInput>();
    generator.subschema_for::<MemoryDeleteInput>();
    generator.subschema_for::<MemoryScopeInput>();
    generator.subschema_for::<MemoryRepairInput>();
    generator.subschema_for::<MemoryRepairResult>();
    generator.subschema_for::<MemoryMutationResult>();
    generator.subschema_for::<MemoryValidation>();
    generator.subschema_for::<MemoryMaintenanceOutput>();
    generator.subschema_for::<MemoryMaintenanceRun>();
    generator.subschema_for::<MemoryMaintenanceResult>();
    generator.subschema_for::<aura_config::MemorySettingsSnapshot>();
    generator.subschema_for::<aura_config::MemorySettingsUpdate>();
    generator.subschema_for::<MemoryDesktopRequest>();
    generator.subschema_for::<MemoryVaultRequest>();
    generator.subschema_for::<MemoryDesktopSnapshot>();
    generator.subschema_for::<MemoryDesktopRepairApply>();
    generator.subschema_for::<MemoryDesktopActionResult>();
    generator.subschema_for::<MemoryExportHandle>();
    generator.subschema_for::<MemoryExportEntryRequest>();
    generator.subschema_for::<MemoryExportEntry>();
    generator.subschema_for::<MemoryExportChunkRequest>();
    generator.subschema_for::<MemoryExportChunk>();
    generator.subschema_for::<MemoryExportCloseRequest>();
    // A ReadOnly session's memory answers, listed and revocable.
    generator.subschema_for::<aura_permission_domain::MemorySessionConsentView>();
    generator.subschema_for::<aura_permission_domain::ListMemorySessionConsentsInput>();
    generator.subschema_for::<aura_permission_domain::RevokeMemorySessionConsentInput>();
}
