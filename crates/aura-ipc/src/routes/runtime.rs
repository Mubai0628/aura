use super::*;
pub(super) const ROUTES: &[RouteDescriptor] = &[
    route!(
        "runtime.readiness",
        Runtime,
        Core,
        AuthenticatedMain,
        "Record<string, never>",
        "RuntimeReadiness",
        ReadOnly,
        None
    ),
    route!(
        "runtime.readiness",
        Runtime,
        Main,
        TrustedRenderer,
        "Record<string, never>",
        "RuntimeReadiness",
        ReadOnly,
        None
    ),
    // Main is quitting; Core records a normal exit so the next start
    // checks only the Store's structure first. Never replayed: a second ask records it again.
    route!(
        "runtime.prepare_exit",
        Runtime,
        Core,
        AuthenticatedMain,
        "Record<string, never>",
        "Record<string, boolean>",
        TransientInput,
        None
    ),
    route!(
        "runtime.command_outcome",
        Runtime,
        Core,
        AuthenticatedMain,
        "CommandOutcomeQuery",
        "CommandOutcome",
        ReadOnly,
        None
    ),
    route!(
        "projection.subscribe",
        Runtime,
        Core,
        AuthenticatedMain,
        "ProjectionSubscription",
        "Record<string, boolean>",
        ReadOnly,
        None
    ),
    route!(
        "projection.unsubscribe",
        Runtime,
        Core,
        AuthenticatedMain,
        "ProjectionUnsubscribeRequest",
        "Record<string, boolean>",
        ReadOnly,
        None
    ),
    route!(
        "projection.snapshot",
        Runtime,
        Core,
        AuthenticatedMain,
        "ProjectionSnapshotRequest",
        "ProjectionChange",
        ReadOnly,
        None
    ),
    route!(
        "aura.cancel",
        Runtime,
        Core,
        AuthenticatedMain,
        "CancellationRequest",
        "CancelDisposition",
        CommandInvocation,
        RequestLedger
    ),
    route!(
        "core.health.get",
        Runtime,
        Main,
        TrustedRenderer,
        "Record<string, never>",
        "ProjectionChange",
        ReadOnly,
        None
    ),
    // The user's configuration recovery entry.
    //
    // `notice` reports which configuration capability is unavailable, what new
    // calls are suspended, and what the user can do. `plan` produces the preview
    // (scope, references needing rebinding, actual loss) and writes nothing.
    // `apply` performs the previewed action, so the user's choice is the action
    // they were shown rather than one that merely happens to be applicable now.
    route!(
        "config.recovery.notice",
        Runtime,
        Core,
        AuthenticatedMain,
        "Record<string, never>",
        "RecoveryNotice",
        ReadOnly,
        None
    ),
    route!(
        "config.recovery.plan",
        Runtime,
        Core,
        AuthenticatedMain,
        "ConfigRecoveryPlanRequest",
        "BundleRecoveryPlan",
        ReadOnly,
        None
    ),
    route!(
        "config.recovery.apply",
        Runtime,
        Core,
        AuthenticatedMain,
        "ConfigRecoveryApplyRequest",
        "RecoveryReport",
        CommandInvocation,
        RequestLedger
    ),
    route!(
        "config.recovery.notice",
        Runtime,
        Main,
        TrustedRenderer,
        "Record<string, never>",
        "RecoveryNotice",
        ReadOnly,
        None
    ),
    route!(
        "config.recovery.plan",
        Runtime,
        Main,
        TrustedRenderer,
        "ConfigRecoveryPlanRequest",
        "BundleRecoveryPlan",
        ReadOnly,
        None
    ),
    route!(
        "config.recovery.apply",
        Runtime,
        Main,
        TrustedRenderer,
        "ConfigRecoveryApplyRequest",
        "RecoveryReport",
        CommandInvocation,
        RequestLedger
    ),
    route!(
        "workspace_session.get",
        Runtime,
        Main,
        TrustedRenderer,
        "Record<string, never>",
        "ProjectionChange",
        ReadOnly,
        None
    ),
    route!(
        "workspace_files.get",
        Runtime,
        Main,
        TrustedRenderer,
        "Record<string, never>",
        "ProjectionChange",
        ReadOnly,
        None
    ),
    route!(
        "projection.subscribe",
        Runtime,
        Main,
        TrustedRenderer,
        "ProjectionSubscription",
        "null",
        ReadOnly,
        None
    ),
    route!(
        "projection.unsubscribe",
        Runtime,
        Main,
        TrustedRenderer,
        "ProjectionUnsubscribeRequest",
        "null",
        ReadOnly,
        None
    ),
    route!(
        "projection.snapshot",
        Runtime,
        Main,
        TrustedRenderer,
        "ProjectionSnapshotRequest",
        "ProjectionChange",
        ReadOnly,
        None
    ),
    #[cfg(feature = "p08-contract-driver")]
    RouteDescriptor {
        test_only: true,
        ..route!(
            "internal.e2e.p08_contract_driver.run_vertical_suite",
            Runtime,
            Core,
            AuthenticatedMain,
            "Record<string, never>",
            "Record<string, unknown>",
            CommandInvocation,
            None
        )
    },
];
