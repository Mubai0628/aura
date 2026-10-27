//! Domain-owned tool families register their contracts through one assembly point.
#[path = "chrome_contract.rs"]
mod chrome_contract;

pub(super) fn register(generator: &mut schemars::SchemaGenerator) {
    super::network_catalog::register(generator);
    super::git_fixtures::register(generator);
    super::package_contract::register(generator);
    super::memory_contract::register(generator);
    super::hook_contract::register(generator);
    super::mcp_contract::register(generator);
    super::plugin_contract::register(generator);
    super::skills_contract::register(generator);
    super::browser_contract::register(generator);
    chrome_contract::register(generator);
    super::media_contract::register(generator);
    super::connections_contract::register(generator);
}
