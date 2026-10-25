use aura_tool_eval::budget::{BudgetStage, Ledger, Limits, real_ledger_path_in};

#[test]
fn every_tool_shares_one_real_ledger_outside_redirected_app_data() {
    let profile = tempfile::tempdir().expect("profile");
    let app_data = tempfile::tempdir().expect("app data");
    let path = real_ledger_path_in(profile.path(), Some(app_data.path()), BudgetStage::S09)
        .expect("fresh machine");
    assert!(path.starts_with(profile.path()) && !path.starts_with(app_data.path()));

    // A process that still sees an AppData ledger, such as one inside a packaged app,
    // must not start a second ledger beside it.
    let earlier = app_data.path().join("AuraToolEval").join("S09");
    std::fs::create_dir_all(&earlier).expect("earlier ledger directory");
    std::fs::write(earlier.join("ledger.db"), b"earlier ledger").expect("earlier ledger");
    let error = real_ledger_path_in(profile.path(), Some(app_data.path()), BudgetStage::S09)
        .expect_err("an unmoved AppData ledger refuses a new shared ledger");
    assert_eq!(error.to_string(), "eval.real_ledger_migration_required");

    // Once moved, both views open the same file.
    std::fs::write(&path, b"moved ledger").expect("moved ledger");
    assert_eq!(
        real_ledger_path_in(profile.path(), Some(app_data.path()), BudgetStage::S09)
            .expect("moved"),
        path
    );
    assert_eq!(
        real_ledger_path_in(profile.path(), None, BudgetStage::S09).expect("outside the package"),
        path
    );
}

/// Has its own 150,000,000-token budget. It gets its own ledger, and
/// `S09`'s ledger keeps the 50,000,000 cap it was accepted with.
#[test]
fn each_stage_keeps_its_own_ledger_and_cap() {
    let profile = tempfile::tempdir().expect("profile");
    let s09 = real_ledger_path_in(profile.path(), None, BudgetStage::S09).expect("S09");
    let s10 = real_ledger_path_in(profile.path(), None, BudgetStage::S10).expect("S10");
    assert_ne!(s09, s10);
    let expected = std::path::Path::new(".aura-tool-eval").join("S10").join("ledger.db");
    assert!(s10.ends_with(&expected), "{}", s10.display());
    let limits = |cumulative| Limits { cumulative, task: 1, request_input: 1, request_output: 1 };
    assert!(BudgetStage::S09.admits(&limits(50_000_000)));
    assert!(!BudgetStage::S09.admits(&limits(50_000_001)));
    assert!(BudgetStage::S10.admits(&limits(150_000_000)));
    assert!(!BudgetStage::S10.admits(&limits(150_000_001)));
    // A ledger keeps the cap it was created with, so a larger budget cannot widen it later.
    let path = profile.path().join("ledger.db");
    drop(Ledger::open(&path, limits(50_000_000)).expect("an S09-sized ledger"));
    let widened = Ledger::open(&path, limits(150_000_000)).err().expect("a widened cap is refused");
    assert_eq!(widened.to_string(), "eval.cumulative_policy_changed");
    assert!(Ledger::open(&profile.path().join("other.db"), limits(150_000_001)).is_err());
}
