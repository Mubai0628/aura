pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    generator.subschema_for::<aura_web_domain::WebSearchInput>();
    generator.subschema_for::<aura_web_domain::WebSearchResult>();
    generator.subschema_for::<aura_web_domain::WebFetchInput>();
    generator.subschema_for::<aura_web_domain::WebFetchResult>();
    generator.subschema_for::<aura_web_domain::WebReadInput>();
    generator.subschema_for::<aura_web_domain::WebReadResult>();
    generator.subschema_for::<aura_web_domain::WebDocument>();
    generator.subschema_for::<aura_web_domain::HttpRequestInput>();
    generator.subschema_for::<aura_web_domain::HttpRequestResult>();
    generator.subschema_for::<aura_web_domain::HttpDownloadInput>();
    generator.subschema_for::<aura_web_domain::HttpDownloadResult>();
    generator.subschema_for::<aura_permission_domain::NetworkSessionConsentView>();
    generator.subschema_for::<aura_permission_domain::ListNetworkSessionConsentsInput>();
    generator.subschema_for::<aura_permission_domain::RevokeNetworkSessionConsentInput>();
    generator.subschema_for::<aura_tool_domain::ToolCredentialOriginsRequestV1>();
}
