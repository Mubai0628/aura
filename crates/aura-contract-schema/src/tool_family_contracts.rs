//! Domain-owned tool families register their contracts through one assembly point.
#[path = "catalog_entries_contract.rs"]
mod catalog_entries_contract;
#[path = "chrome_contract.rs"]
mod chrome_contract;
#[path = "computer_contract.rs"]
mod computer_contract;
#[path = "credential_cleanup_contract.rs"]
mod credential_cleanup_contract;

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
    computer_contract::register(generator);
    super::media_contract::register(generator);
    super::connections_contract::register(generator);
    credential_cleanup_contract::register(generator);
    catalog_entries_contract::register(generator);
}
