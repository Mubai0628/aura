//! Integration tests of `aura-config` in one test program, one module per behaviour.

mod l01_network_proxy;
mod l01_network_proxy_recovery;
mod l05_memory;
mod l07_hooks;
mod l08_mcp;
mod l09_skills;
mod l10_8_ui_settings;
mod l13_browser_settings;
mod l15_skill_choices;
mod l17_plugin_theme;
mod l18_computer_settings;
mod p02_config;
mod s05_domain_independence;
mod s05_health_regressions;
mod s05_secret_isolation;
mod s05r2_recovery_responsibility;
mod s05r3_history_write_contention;
mod s05r_domain_ownership;
mod s05r_user_recovery;
mod system_proxy_default;
mod unpublished_bundle_release;
