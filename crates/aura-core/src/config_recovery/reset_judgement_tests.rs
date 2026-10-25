//! An ordinary whole-set confirmation must settle its own unknown-scope record.
use crate::r03_tests::{Fixture, success};
use crate::{StartupMode, config_degradation::ConfigDependentCapability};
use aura_config::{ConfigModule, SettingsFacade};
use serde_json::json;
use std::fs;

#[tokio::test]
async fn notice_reports_missing_consumers_without_misidentifying_the_damaged_domain() {
    let fixture = Fixture::new();
    drop(
        SettingsFacade::open(fixture.paths.clone())
            .expect("obtain the actual isolated recovery result"),
    );
    fs::remove_dir_all(fixture.paths.history_path(ConfigModule::Tools))
        .expect("obtain the actual isolated recovery result");
    fs::write(fixture.paths.config_path(ConfigModule::Tools), b"damaged tool configuration")
        .expect("obtain the actual isolated recovery result");
    let state = fixture.start().await;
    let StartupMode::Ready(services) = &state.startup_mode else {
        panic!("ready repair shell");
    };
    assert!(services.p05().is_err(), "the dependent service really is absent");
    assert!(services._settings.health(ConfigModule::Models).is_ready());
    let before = success(&state, "config.recovery.notice", json!({})).await;
    assert_eq!(before["affected_domains"], json!(["tools"]));
    assert!(
        before["unavailable_capabilities"]
            .as_array()
            .expect("obtain the actual isolated recovery result")
            .iter()
            .any(|key| key == "model_authority")
    );
    let plan = success(&state, "config.recovery.plan", json!({"domains": []})).await;
    assert_eq!(plan["modules"], json!(["tools"]));
    assert_eq!(
        plan["domains"].as_array().expect("obtain the actual isolated recovery result").len(),
        1,
        "multiple recorded causes must never make one file a multi-member recovery"
    );
    let applied = success(
        &state,
        "config.recovery.apply",
        json!({
            "domain": "tools", "domains": [], "action": "reset_to_default",
            "preview_id": plan["domains"][0]["preview_id"],
        }),
    )
    .await;
    assert_eq!(applied["outcomes"][0]["applied"], true);
    assert!(services.p05().is_ok(), "repair supplies the same live chain");
    let after = success(&state, "config.recovery.notice", json!({})).await;
    assert_eq!(after["unavailable_capabilities"], json!([]));
}

#[tokio::test]
async fn ordinary_full_reset_clears_only_its_own_unknown_bundle_judgement() {
    for unrelated_journal in [false, true] {
        let fixture = Fixture::new();
        drop(SettingsFacade::open(fixture.paths.clone()).expect("seed our configuration"));
        let transactions = &fixture.paths.config_transaction_dir;
        let bundles = transactions.join("module-bundles");
        fs::create_dir_all(&bundles).expect("obtain the actual isolated recovery result");
        let original = b"unreadable save-set original";
        fs::write(bundles.join("fault.json"), original)
            .expect("obtain the actual isolated recovery result");
        // This journal has a different owner and may describe credential/package
        // responsibility. Resetting config values does not establish its outcome.
        if unrelated_journal {
            fs::write(transactions.join("legacy.json"), b"unreadable separate transaction")
                .expect("obtain the actual isolated recovery result");
        }
        let state = fixture.start().await;
        let StartupMode::Ready(services) = &state.startup_mode else {
            panic!("view and recovery must stay reachable");
        };
        let before = success(&state, "config.recovery.notice", json!({})).await;
        assert_eq!(before["scope_known"], false);
        let plan = success(&state, "config.recovery.plan", json!({"domains": []})).await;
        assert_eq!(
            plan["modules"].as_array().expect("obtain the actual isolated recovery result").len(),
            15
        );
        let applied = success(
            &state,
            "config.recovery.apply",
            json!({"action": "reset_to_default", "domains": plan["modules"],
                "preview_id": plan["preview_id"]}),
        )
        .await;
        assert!(
            applied["outcomes"]
                .as_array()
                .expect("obtain the actual isolated recovery result")
                .iter()
                .all(|v| v["applied"] == true)
        );
        assert_eq!(
            fs::read(transactions.join("recovery-originals/fault.json"))
                .expect("obtain the actual isolated recovery result"),
            original
        );
        let after = success(&state, "config.recovery.notice", json!({})).await;
        assert_eq!(after["scope_known"], !unrelated_journal, "{after}");
        assert_eq!(after["affected_domains"], json!([]));
        assert_eq!(
            services
                .config_degradation
                .is_available(ConfigDependentCapability::ConfigTransactionRecovery),
            !unrelated_journal,
        );
        assert!(!services.config_degradation.all().iter().any(|fault|
            fault.fault_code == crate::config_degradation::BUNDLE_SCOPE_UNRESOLVED));
        if unrelated_journal {
            assert_eq!(
                fs::read(transactions.join("legacy.json"))
                    .expect("obtain the actual isolated recovery result"),
                b"unreadable separate transaction"
            );
            assert!(
                applied["unavailable_capabilities"]
                    .as_array()
                    .expect("obtain the actual isolated recovery result")
                    .iter()
                    .any(|v| v["capability"] == "config_transaction_recovery")
            );
        } else {
            assert!(
                applied["restored_capabilities"]
                    .as_array()
                    .expect("obtain the actual isolated recovery result")
                    .iter()
                    .any(|v| v == "config_transaction_recovery")
            );
        }
    }
}
