//! The committed execution strategy has one owner, and changing it really changes
//! the capacity the public execution path admits against.
//!
//! The settings value is not a page-local number: it is applied to the same global
//! capacity owner that dispatch acquires from. This test reads the applied fact
//! back from that owner instead of only checking that the save returned a new
//! revision, so a save that never reached dispatch would fail here.
use super::*;
use aura_tool_contract::{
    DEFAULT_MAX_CONCURRENT_TOOL_CALLS, DEFAULT_MAX_TOOL_RETRIES, DEFAULT_TOOL_TIMEOUT_MS,
};

#[tokio::test]
async fn a_saved_concurrency_limit_is_applied_to_the_global_capacity_owner() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    // An untouched configuration starts from the committed default.
    assert_eq!(
        services.agent_required().execution.global_tool_capacity().0,
        DEFAULT_MAX_CONCURRENT_TOOL_CALLS
    );

    let view = services.agent_required().tool_settings().expect("tool settings");
    let updated = services
        .agent_required()
        .update_tool_settings(
            CommandInvocationId::new(),
            aura_tool_contract::AgentToolSettingsUpdate {
                expected_revision: view.revision,
                fixed_tool_ids: view.fixed_tool_ids.clone(),
                max_dynamic_tool_schemas: view.max_dynamic_tool_schemas,
                max_concurrent_tool_calls: 7,
                default_timeout_ms: DEFAULT_TOOL_TIMEOUT_MS,
                max_retries: DEFAULT_MAX_TOOL_RETRIES,
                use_recommended_fixed_tool_ids: false,
                network_proxy: None,
                web_service: None,
            },
        )
        .await
        .expect("save");
    assert_eq!(updated.max_concurrent_tool_calls, 7);
    // The value the caller saved is the value dispatch admits against.
    assert_eq!(services.agent_required().execution.global_tool_capacity().0, 7);
}

/// A save made against a revision that has already moved cannot move the capacity
/// back.
///
/// The review of this work suspected that the revision check and the capacity
/// application were not serialized, so a late save built from an older read could
/// restore an older concurrency limit. This pins what really happens: the
/// revision-checked write rejects the stale submission before any capacity is
/// applied, and the applied capacity always comes from the committed settings, so
/// the value dispatch admits against stays the one that was actually committed.
#[tokio::test]
async fn a_stale_revision_cannot_move_the_applied_capacity_back() {
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let stale = services.agent_required().tool_settings().expect("tool settings");

    let saved = services
        .agent_required()
        .update_tool_settings(
            CommandInvocationId::new(),
            aura_tool_contract::AgentToolSettingsUpdate {
                expected_revision: stale.revision,
                fixed_tool_ids: stale.fixed_tool_ids.clone(),
                max_dynamic_tool_schemas: stale.max_dynamic_tool_schemas,
                max_concurrent_tool_calls: 7,
                default_timeout_ms: DEFAULT_TOOL_TIMEOUT_MS,
                max_retries: DEFAULT_MAX_TOOL_RETRIES,
                use_recommended_fixed_tool_ids: false,
                network_proxy: None,
                web_service: None,
            },
        )
        .await
        .expect("save");
    assert_eq!(services.agent_required().execution.global_tool_capacity().0, 7);

    // The same read the first save was built from, submitted late with a different
    // capacity: it is the old revision, so it must be refused instead of applied.
    let late = services
        .agent_required()
        .update_tool_settings(
            CommandInvocationId::new(),
            aura_tool_contract::AgentToolSettingsUpdate {
                expected_revision: stale.revision,
                fixed_tool_ids: stale.fixed_tool_ids.clone(),
                max_dynamic_tool_schemas: stale.max_dynamic_tool_schemas,
                max_concurrent_tool_calls: 2,
                default_timeout_ms: DEFAULT_TOOL_TIMEOUT_MS,
                max_retries: DEFAULT_MAX_TOOL_RETRIES,
                use_recommended_fixed_tool_ids: false,
                network_proxy: None,
                web_service: None,
            },
        )
        .await;
    let error = late.expect_err("a stale revision must be refused");
    assert!(
        format!("{error}").contains("tools.configuration_revision_conflict"),
        "unexpected error: {error}"
    );
    assert_eq!(
        services.agent_required().execution.global_tool_capacity().0,
        7,
        "a refused save must not change the capacity dispatch admits against"
    );
    let current = services.agent_required().tool_settings().expect("tool settings");
    assert_eq!(current.revision, saved.revision, "the committed revision must not move");
    assert_eq!(current.max_concurrent_tool_calls, 7);
}

/// A late reader that applies its snapshot after a newer save must not win.
///
/// This is the interleaving the review asked for, made deterministic instead of
/// timing-dependent: a reader holds the committed snapshot at revision N, a newer save
/// commits N+1 and applies it, and only then does the older reader reach the apply
/// step. Both the applied revision and the capacity the dispatcher admits against must
/// stay at N+1 — the older snapshot must be unable to move either back.
///
/// The apply step is called directly because that is exactly where the old reader
/// would have been. The serialization added there is what makes the two agree, so a
/// test that only re-read the settings through the public getter would never reach the
/// interleaving at all.
#[tokio::test]
async fn a_late_reader_cannot_apply_an_older_capacity() {
    use aura_tool_contract::ToolSettingsSnapshotV1;
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    let (held, _fixed) =
        services.agent_required().tool_configuration().expect("committed configuration");
    // What the late reader is holding: the snapshot as it was before the save.
    let held_snapshot = ToolSettingsSnapshotV1 { max_concurrent_tool_calls: 2, ..held.clone() };

    // A newer save commits and applies its own capacity while the reader still holds
    // the older snapshot.
    let saved = services
        .agent_required()
        .update_tool_settings(
            CommandInvocationId::new(),
            aura_tool_contract::AgentToolSettingsUpdate {
                expected_revision: held.revision,
                fixed_tool_ids: _fixed.to_vec(),
                max_dynamic_tool_schemas: held.max_dynamic_tool_schemas,
                max_concurrent_tool_calls: 7,
                default_timeout_ms: held.default_timeout_ms,
                max_retries: held.max_retries,
                use_recommended_fixed_tool_ids: false,
                network_proxy: None,
                web_service: None,
            },
        )
        .await
        .expect("save");
    assert_eq!(services.agent_required().execution.global_tool_capacity().0, 7);

    // The late reader now reaches the apply step with its older snapshot.
    services.agent_required().apply_committed_strategy(&held_snapshot).expect("late apply");

    assert_eq!(
        services.agent_required().execution.global_tool_capacity().0,
        7,
        "a late reader must not push an older capacity back"
    );
    assert_eq!(
        services.agent_required().tool_settings().expect("tool settings").revision,
        saved.revision,
        "and it must not move the committed revision back either"
    );
}

/// Several appliers of the same two revisions end on the newest capacity.
///
/// The sequential case above cannot show what happens when the readers really run at
/// the same time, which is the shape the suspicion had: more than one apply step can
/// pass a watermark check before any of them writes. This drives that concurrency
/// directly — the newer revision is applied first so the watermark is already N+1, then
/// a set of threads applies the older and newer snapshots together — and requires the
/// newest capacity to be the one left in force.
#[tokio::test]
async fn concurrent_appliers_end_on_the_newest_capacity() {
    use aura_tool_contract::ToolSettingsSnapshotV1;
    let host = crate::r03_tests::Fixture::new();
    let runtime = host.start().await;
    let crate::StartupMode::Ready(services) = &runtime.startup_mode else { panic!("Core ready") };
    for attempt in 0..8 {
        let (base, _fixed) =
            services.agent_required().tool_configuration().expect("committed configuration");
        let older = ToolSettingsSnapshotV1 { max_concurrent_tool_calls: 2, ..base.clone() };
        let newer = ToolSettingsSnapshotV1 {
            revision: Revision::new(base.revision.value() + 1),
            max_concurrent_tool_calls: 7,
            ..base.clone()
        };
        services.agent_required().apply_committed_strategy(&newer).expect("newer apply");
        let agent = services.agent_required();
        std::thread::scope(|scope| {
            for index in 0..8 {
                let snapshot = if index % 2 == 0 { older.clone() } else { newer.clone() };
                let agent = std::sync::Arc::clone(&agent);
                scope.spawn(move || {
                    let _ = agent.apply_committed_strategy(&snapshot);
                });
            }
        });
        assert_eq!(
            services.agent_required().execution.global_tool_capacity().0,
            7,
            "attempt {attempt}: an applier that lost the race must not decide the capacity"
        );
    }
}
