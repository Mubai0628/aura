//! The 重试 of the credentials deleted model settings left in Windows Credential Manager.
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    generator.subschema_for::<aura_model_domain::ModelCredentialCleanupRetryRequest>();
    generator.subschema_for::<aura_model_domain::ModelCredentialCleanupRetryResult>();
}
