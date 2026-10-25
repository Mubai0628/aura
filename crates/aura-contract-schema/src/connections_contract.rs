//! A provider's default connection and its backups.
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    generator.subschema_for::<aura_model_domain::ModelProviderConnectionsRequest>();
}
