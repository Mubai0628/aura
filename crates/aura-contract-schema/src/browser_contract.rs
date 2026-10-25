//! The built-in browser's tools and the commands Core sends its host (Electron Main).
pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    generator.subschema_for::<aura_browser_domain::BrowserOpenInput>();
    generator.subschema_for::<aura_browser_domain::BrowserNavigateInput>();
    generator.subschema_for::<aura_browser_domain::BrowserSnapshotInput>();
    generator.subschema_for::<aura_browser_domain::BrowserScreenshotInput>();
    generator.subschema_for::<aura_browser_domain::BrowserClickInput>();
    generator.subschema_for::<aura_browser_domain::BrowserTypeInput>();
    generator.subschema_for::<aura_browser_domain::BrowserPressInput>();
    generator.subschema_for::<aura_browser_domain::BrowserScrollInput>();
    generator.subschema_for::<aura_browser_domain::BrowserHoverInput>();
    generator.subschema_for::<aura_browser_domain::BrowserSelectInput>();
    generator.subschema_for::<aura_browser_domain::BrowserWaitInput>();
    generator.subschema_for::<aura_browser_domain::BrowserConsoleInput>();
    generator.subschema_for::<aura_browser_domain::BrowserCloseInput>();
    generator.subschema_for::<aura_browser_domain::BrowserPageResult>();
    generator.subschema_for::<aura_browser_domain::BrowserScreenshotResult>();
    generator.subschema_for::<aura_browser_domain::BrowserConsoleResult>();
    generator.subschema_for::<aura_browser_domain::BrowserCloseResult>();
    generator.subschema_for::<aura_browser_domain::BrowserRequestUserInput>();
    generator.subschema_for::<aura_browser_domain::BrowserAssistResult>();
    generator.subschema_for::<aura_browser_domain::BrowserEvaluateInput>();
    generator.subschema_for::<aura_browser_domain::BrowserEvaluateResult>();
    generator.subschema_for::<aura_browser_domain::BrowserNetworkInput>();
    generator.subschema_for::<aura_browser_domain::BrowserNetworkResult>();
    // An element's HTML, box and style, and the page's performance counters.
    generator.subschema_for::<aura_browser_domain::BrowserInspectInput>();
    generator.subschema_for::<aura_browser_domain::BrowserInspectResult>();
    generator.subschema_for::<aura_browser_domain::BrowserPerformanceInput>();
    generator.subschema_for::<aura_browser_domain::BrowserPerformanceResult>();
    generator.subschema_for::<aura_browser_domain::BrowserHostCancelV1>();
    generator.subschema_for::<aura_browser_domain::BrowserHostCommandV1>();
    generator.subschema_for::<aura_browser_domain::BrowserHostResultV1>();
    generator.subschema_for::<aura_browser_domain::BrowserHostResultAck>();
    generator.subschema_for::<aura_browser_domain::BrowserScreenshotReadRequest>();
    generator.subschema_for::<aura_browser_domain::BrowserScreenshotImageV1>();
}
