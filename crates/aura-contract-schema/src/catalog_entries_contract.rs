//! The catalog entries a model's name matches, and the one the user links it to.
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    generator.subschema_for::<aura_model_domain::ModelProfileCatalogEntriesRequest>();
    generator.subschema_for::<aura_model_domain::ModelProfileCatalogEntriesView>();
}
