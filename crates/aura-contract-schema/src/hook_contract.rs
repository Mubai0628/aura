//! Hooks: Settings → Hook and the invocation records.
use aura_hooks::*;
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    generator.subschema_for::<HookEvent>();
    generator.subschema_for::<HookDefinition>();
    generator.subschema_for::<HookEntryView>();
    generator.subschema_for::<HookRegistryView>();
    generator.subschema_for::<HookListRequest>();
    generator.subschema_for::<HookPreviewRequest>();
    generator.subschema_for::<HookPreview>();
    generator.subschema_for::<HookTrustRequest>();
    generator.subschema_for::<HookSetEnabledRequest>();
    generator.subschema_for::<HookPathRequest>();
    generator.subschema_for::<HookMoveRequest>();
    generator.subschema_for::<HookInvocationView>();
    generator.subschema_for::<HookInvocationList>();
    generator.subschema_for::<HookInvocationListRequest>();
    generator.subschema_for::<HookTurnStatistics>();
    generator.subschema_for::<HookSelectDirectoryRequest>();
    generator.subschema_for::<HookSelectedDirectory>();
}
