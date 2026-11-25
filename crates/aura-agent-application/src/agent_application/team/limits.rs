//! The limits the user sets for sub-agents (设置 → 智能能力 → 多 Agent), and the watchdog's clock:
//! one pass over the running sub-agents every few seconds, by the application bound to the team.
use super::*;
use aura_agent_team::{
    AgentTeamLimits, AgentTeamPermissions, AgentTeamSettingsSetRequest, AgentTeamSettingsView,
    TeamHost, TeamPort, limits,
};
use std::{
    sync::{Mutex, Weak},
    time::Duration,
};

/// How often the watchdog looks.
const TICK: Duration = if cfg!(any(test, feature = "test-hooks")) {
    Duration::from_millis(100)
} else {
    Duration::from_secs(10)
};

impl AgentApplication {
    /// The limits and permissions now, their defaults and the settings revision.
    pub async fn team_settings(self: &Arc<Self>) -> anyhow::Result<AgentTeamSettingsView> {
        let ((limits, permissions, revision), enabled) = self
            .store
            .read(StoreStep::operation("agents.limits"), |db| {
                Ok((limits::settings(db)?, limits::enabled(db)?))
            })
            .await?;
        Ok(AgentTeamSettingsView {
            enabled,
            limits,
            defaults: AgentTeamLimits::DEFAULT,
            minimum: AgentTeamLimits::MINIMUM,
            maximum: AgentTeamLimits::MAXIMUM,
            permissions,
            permission_defaults: AgentTeamPermissions::DEFAULT,
            revision,
        })
    }

    /// Saves the limits, and the permissions when given, at the revision the page read. Sub-agents
    /// already working keep running under the new limits from the watchdog's next pass and their
    /// next command; the permissions are what new sub-agents start with.
    pub async fn set_team_settings(
        self: &Arc<Self>,
        request: AgentTeamSettingsSetRequest,
    ) -> anyhow::Result<AgentTeamSettingsView> {
        request.limits.check()?;
        let (chosen, permissions, expected, enabled) =
            (request.limits, request.permissions, request.expected_revision, request.enabled);
        let host = TeamApp(Arc::clone(self));
        host.write("agents.limits.set", move |tx| {
            limits::set_controls(tx, &chosen, permissions.as_ref(), enabled, expected)
        })
        .await
            .map_err(|error| match error.downcast_ref::<StoreError>() {
                Some(StoreError::Failed {
                    error: aura_persistence_core::PersistenceCoreError::Conflict,
                    ..
                }) => anyhow::anyhow!(
                    "agents.limits_changed: 多 Agent 的设置刚被改过，这次没有保存，请在最新的设置上再改。"
                ),
                _ => error,
            })?;
        self.wake_temporary_queues(None).await?;
        self.team_settings().await
    }

    /// Starts the watchdog once per team, on the runtime of the call that first set a sub-agent
    /// to work. It looks through whichever application is bound to the team then.
    pub(super) fn watch_team(&self) {
        if self.execution.team.start_watching() {
            drop(tokio::spawn(watch(Arc::downgrade(&self.execution.team))));
        }
    }

    /// One pass of the watchdog; a failure is logged once until it changes.
    pub(super) fn watch_once(self: Arc<Self>, elapsed: Duration) -> Boxed<'static, ()> {
        Box::pin(async move {
            let host = TeamApp(Arc::clone(&self));
            let team = Team::new(&host, self.execution.team.news());
            let outcome = team.watch(self.execution.team.watchdog(), elapsed).await;
            static LAST: Mutex<Option<String>> = Mutex::new(None);
            let mut last = LAST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match outcome {
                Ok(()) => *last = None,
                Err(error) => {
                    let reason = error.to_string();
                    if last.as_deref() != Some(reason.as_str()) {
                        append_core_log(
                            "agents.watch_failed",
                            serde_json::json!({"reason": reason.chars().take(300).collect::<String>()}),
                        );
                        *last = Some(reason);
                    }
                }
            }
        })
    }
}

async fn watch(port: Weak<TeamPort>) {
    let mut last = tokio::time::Instant::now();
    loop {
        tokio::time::sleep(TICK).await;
        let Some(port) = port.upgrade() else { return };
        let now = tokio::time::Instant::now();
        // A gap far longer than the tick is the machine asleep or Core held up, not work: it
        // counts as three ticks at most, so no sub-agent is stopped for time it never had.
        let elapsed = (now - last).min(TICK * 3);
        last = now;
        if let Some(tools) = port.tools() {
            tools.watch(elapsed).await;
        }
    }
}

#[cfg(test)]
#[path = "limits_tests.rs"]
mod tests;
