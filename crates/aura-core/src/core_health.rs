//! Core's health projection: whether it is ready, read-only or recovering, what it is busy with,
//! and what the user should know about its stores, including the
//! background store check's finding.
use super::*;

/// Damage the background check found after the Store opened.
pub(crate) fn store_damaged(state: &RuntimeState) -> bool {
    match &state.startup_mode {
        StartupMode::Ready(services) => services.broker.store_damaged(),
        StartupMode::RecoveryRequired { .. } => false,
    }
}

pub(crate) fn current_core_health_projection(state: &RuntimeState) -> ProjectionChange {
    let ready = state.ready.load(Ordering::Acquire);
    let recovery_required = matches!(&state.startup_mode, StartupMode::RecoveryRequired { .. });
    // The health projection is the **first real Core consumer** of the
    // configuration-degradation record. Before R2 the new health API had no
    // production consumer, so a runtime whose configuration was damaged still
    // reported `core.ready` and a caller had no way to learn which capability was
    // gone. The degraded set is reported here, by stable code and capability key,
    // so the UI can name what is unavailable without reading any path or body.
    let degraded = state.config_degradation.all();
    let degraded_code = state.config_degradation.summary();
    let ready_with_degradation = ready && !recovery_required && !degraded.is_empty();
    // A capability loss does **not** make Core read-only: unrelated work, status,
    // stop and repair must all stay reachable. Only a recovery-required or
    // not-yet-ready runtime is read-only.
    let read_only = !ready || recovery_required;
    let (status_code, status_message) = if recovery_required {
        (
            "core.recovery_required",
            "Aura 的数据需要恢复，智能体已停止运行；现有内容只能查看，原因记录在本机日志里。"
                .to_owned(),
        )
    } else if ready && store_damaged(state) {
        (
            "core.store_check_failed",
            "Aura 的数据库检查发现了问题，原因记在本机日志里。现有内容仍可查看和使用；建议先备份 Aura 的数据文件夹，下次启动会再检查一遍。"
                .to_owned(),
        )
    } else if ready_with_degradation {
        // The summary names capabilities and stable codes only — no path, no
        // document body, nothing a caller supplied. The caller learns *what* is
        // unavailable; repairing it is the user-recovery flow's job.
        (
            "core.ready_degraded",
            format!(
                "Aura Core 已就绪；部分依赖配置的能力暂不可用（{degraded_code}），请到设置中查看恢复选项。"
            ),
        )
    } else if ready {
        ("core.ready", "Aura Core 已就绪。".to_owned())
    } else {
        ("core.starting", "Aura Core 正在启动。".to_owned())
    };
    ProjectionChange::CoreHealth {
        runtime_instance_id: state.options.runtime_instance_id,
        projection_scope: ProjectionScope::Global,
        revision: Revision::new(state.core_health_revision.load(Ordering::Acquire)),
        payload: CoreHealthProjection {
            state: if recovery_required {
                CoreLifecycleState::RecoveryRequired
            } else if ready {
                CoreLifecycleState::Ready
            } else {
                CoreLifecycleState::Starting
            },
            read_only,
            safe_mode: state.options.safe_mode,
            restart_attempt: 0,
            status_code: status_code.to_owned(),
            status_message,
            busy: busy::current(),
        },
    }
}
