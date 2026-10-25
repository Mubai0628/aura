//! 设置 → 网络与浏览器. The browser switch (default on), the sites a ReadOnly session opens
//! without asking, the sites never loaded, and the debugging switch (default off). Rules are
//! saved in their canonical form; a file Aura cannot trust never reads as the defaults, whose
//! empty block list would unblock what the user blocked.
use aura_config::{AuraPaths, BrowserSettings, ConfigModule, SettingsFacade};
use serde_json::json;
use std::sync::Arc;

fn facade() -> (tempfile::TempDir, AuraPaths, Arc<SettingsFacade>) {
    let temp = tempfile::tempdir().expect("TEMP");
    let paths = AuraPaths::from_roots(temp.path().join("config"), temp.path().join("data"));
    let facade = SettingsFacade::open(paths.clone()).expect("settings");
    (temp, paths, facade)
}

fn sites(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn the_browser_is_on_with_no_site_lists_and_debugging_off_by_default() {
    let (_temp, _paths, facade) = facade();
    let snapshot = facade.browser_settings();
    assert_eq!(
        snapshot.settings,
        BrowserSettings {
            enabled: true,
            allowed_sites: vec![],
            blocked_sites: vec![],
            debugging: false
        }
    );
    assert_eq!(snapshot.revision, Some(1));
    assert!(snapshot.issue.is_none());
}

#[test]
fn sites_are_saved_in_their_canonical_form_and_read_back_after_a_restart() {
    let (_temp, paths, facade) = facade();
    let chosen = BrowserSettings {
        enabled: true,
        allowed_sites: sites(&["Example.COM.", "https://docs.rs/serde", "localhost:5173"]),
        blocked_sites: sites(&["*.Ads.example.net"]),
        debugging: true,
    };
    facade.update_browser_settings(1, &chosen).expect("save");
    let saved = BrowserSettings {
        enabled: true,
        allowed_sites: sites(&["example.com", "docs.rs", "localhost:5173"]),
        blocked_sites: sites(&["*.ads.example.net"]),
        debugging: true,
    };
    assert_eq!(facade.browser_settings().settings, saved);
    assert_eq!(facade.browser_settings().revision, Some(2));
    drop(facade);
    let reopened = SettingsFacade::open(paths).expect("reopen");
    assert_eq!(reopened.browser_settings().settings, saved);
    assert_eq!(reopened.browser_settings().settings.allowed_rules().len(), 3);
    // A stale revision is refused and changes nothing.
    assert!(reopened.update_browser_settings(1, &BrowserSettings::default()).is_err());
    assert_eq!(reopened.browser_settings().settings, saved);
}

#[test]
fn a_bad_site_list_is_refused_with_what_is_wrong_and_nothing_changes() {
    let (_temp, _paths, facade) = facade();
    let cases = [
        (sites(&["exa mple.com"]), sites(&[]), "allowed_sites", "exa mple.com"),
        (sites(&["example.com", "EXAMPLE.com"]), sites(&[]), "allowed_sites", "重复"),
        (sites(&[]), sites(&["ftp://example.com"]), "blocked_sites", "http"),
        (sites(&["example.com"]), sites(&["example.com."]), "blocked_sites", "同时允许和阻止"),
        (vec!["a.com".to_owned(); 201], sites(&[]), "allowed_sites", "200"),
    ];
    for (allowed, blocked, field, words) in cases {
        let settings = BrowserSettings {
            allowed_sites: allowed,
            blocked_sites: blocked,
            ..BrowserSettings::default()
        };
        let error = facade.update_browser_settings(1, &settings).expect_err("refused").to_string();
        assert!(error.contains(field) && error.contains(words), "{error}");
        assert_eq!(facade.browser_settings().revision, Some(1));
    }
}

#[test]
fn values_written_around_the_settings_page_are_checked_too() {
    let (_temp, _paths, facade) = facade();
    for (field, value) in [
        ("enabled", json!("yes")),
        ("debugging", json!(1)),
        ("allowed_sites", json!("example.com")),
        ("blocked_sites", json!(["*"])),
    ] {
        assert!(
            facade
                .update(ConfigModule::Browser, 1, |values| {
                    values.insert(field.into(), value);
                })
                .is_err(),
            "{field} must be refused"
        );
        assert_eq!(facade.browser_settings().revision, Some(1));
    }
}

#[test]
fn a_file_changed_into_something_invalid_while_aura_runs_stops_the_browser_instead_of_unblocking() {
    let (_temp, paths, facade) = facade();
    let chosen = BrowserSettings {
        blocked_sites: sites(&["tracker.example"]),
        ..BrowserSettings::default()
    };
    facade.update_browser_settings(1, &chosen).expect("save");
    std::fs::write(
        paths.config_path(ConfigModule::Browser),
        b"schema_version = 1\nrevision = 3\nblocked_sites = 'tracker.example'\n",
    )
    .expect("external edit");
    let snapshot = facade.browser_settings();
    assert!(!snapshot.settings.enabled, "the browser must not run on settings it cannot read");
    assert!(snapshot.issue.as_deref().is_some_and(|issue| issue.contains("恢复上一版有效配置")));
}

#[test]
fn a_damaged_file_at_startup_gives_back_the_last_valid_lists() {
    let (_temp, paths, facade) = facade();
    let chosen = BrowserSettings {
        blocked_sites: sites(&["tracker.example"]),
        ..BrowserSettings::default()
    };
    facade.update_browser_settings(1, &chosen).expect("save");
    drop(facade);
    std::fs::write(
        paths.config_path(ConfigModule::Browser),
        b"schema_version = 1\nrevision = 3\nblocked_sites = ['*']\n",
    )
    .expect("external edit");
    let reopened = SettingsFacade::open(paths).expect("reopen");
    let snapshot = reopened.browser_settings();
    assert_eq!(snapshot.settings, chosen);
    assert!(snapshot.issue.is_none());
}
