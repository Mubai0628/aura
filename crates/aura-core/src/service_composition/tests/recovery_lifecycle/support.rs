use super::*;

/// Replaces one domain's configuration file with a directory, so its real `File::open` fails.
pub(super) fn break_domain(paths: &aura_config::AuraPaths, module: ConfigModule) {
    let path = paths.config_path(module);
    if path.is_file() {
        fs::remove_file(&path).expect("S05R3 fixture operation must succeed");
    }
    fs::create_dir_all(&path).expect("S05R3 fixture operation must succeed");
    fs::write(path.join("blocker.txt"), b"this domain cannot be opened")
        .expect("S05R3 fixture operation must succeed");
}

/// Repairs a domain by removing the blocking directory and writing a valid document.
pub(super) fn repair_domain(paths: &aura_config::AuraPaths, module: ConfigModule, revision: u64) {
    let path = paths.config_path(module);
    fs::remove_dir_all(&path).expect("S05R3 fixture operation must succeed");
    fs::write(
        &path,
        serde_json::json!({ "schema_version": 1, "revision": revision, "values": {} }).to_string(),
    )
    .expect("S05R3 fixture operation must succeed");
}

/// A preview taken now, and the confirmation of exactly that preview.
///
/// The fixture repairs a file externally while the real watcher remains active. If that watcher
/// serves the new revision between preview and confirmation, the old token is correctly refused.
/// Take a new readonly preview in that case; never bypass the check or replay an applied write.
pub(super) fn confirmed_preview(
    entry: &crate::config_recovery::ConfigRecoveryEntry,
    module: ConfigModule,
) -> aura_config::recovery::DomainRecoveryPlan {
    for _ in 0..8 {
        let preview = entry.plan_bundle(&[module]).expect("readonly preview");
        match entry.plan_domain_confirmed(module, &preview.domains[0].preview_id) {
            Ok(plan) => return plan,
            Err(aura_config::recovery::ConfigRecoveryError::Unavailable {
                reason_code, ..
            }) if reason_code == aura_config::recovery::codes::PREVIEW_STALE => {
                let next = entry.plan_bundle(&[module]).expect("refresh refused preview");
                eprintln!(
                    "external fixture repair advanced {:?} preview: {:?} -> {:?}",
                    module, preview.domains[0].current_revision, next.domains[0].current_revision
                );
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(error) => panic!("unexpected confirmation failure: {error:?}"),
        }
    }
    panic!("the live preview did not settle within the bounded fixture window")
}
