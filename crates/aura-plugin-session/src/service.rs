//! The calls. Each checks the plugin before anything is read or done, then goes the way the
//! user's own action goes: a message through the composer's queue and its Hooks, a prompt command
//! as the composer expands it, a stop through the composer's stop, a setting as the plugin page
//! sets it.
use crate::channels::expanded;
use crate::checks::{
    Standing, checked_key, checked_notice, checked_status, checked_text, message_command,
    permitted, unusable,
};
use crate::own::own_prompt;
use crate::panel::panel_site;
use aura_agent_application::agent_application::AgentApplication;
use aura_agent_domain::*;
use aura_core_broker::agent_store::StoreStep;
use aura_domain_common::*;
use aura_extension_service::commands::command_catalog;
use aura_extension_service::plugins::{PluginRuntime, ReadyPlugin};
use aura_persistence_core::PersistenceCoreError;
use aura_persistence_v2::agent::{inbox, plugin_session, session, turns, under_way};
use aura_plugins::PluginCapability;
use serde_json::Value;
use std::sync::Arc;

/// Whether a call starts Agent work, and so waits for what the user's new work waits for.
#[must_use]
pub fn starts_work(method: &str) -> bool {
    matches!(method, "agent.plugin_session.send" | "agent.plugin_session.run")
}

pub struct PluginSessions {
    agent: Arc<AgentApplication>,
    plugins: Arc<PluginRuntime>,
}

impl PluginSessions {
    #[must_use]
    pub fn new(agent: Arc<AgentApplication>, plugins: Arc<PluginRuntime>) -> Self {
        Self { agent, plugins }
    }

    /// One call from Main, by its route. `command` is the call's own invocation.
    pub async fn dispatch(
        &self,
        method: &str,
        command: Option<CommandInvocationId>,
        params: Value,
    ) -> anyhow::Result<Value> {
        Ok(match method {
            "agent.plugin_session.state" => {
                serde_json::to_value(self.state(serde_json::from_value(params)?).await?)?
            }
            "agent.plugin_session.events" => {
                serde_json::to_value(self.events(serde_json::from_value(params)?).await?)?
            }
            "agent.plugin_session.send" => {
                let command =
                    command.ok_or_else(|| anyhow::anyhow!("ipc.command_invocation_id_required"))?;
                serde_json::to_value(self.send(command, serde_json::from_value(params)?).await?)?
            }
            "agent.plugin_session.run" => {
                let command =
                    command.ok_or_else(|| anyhow::anyhow!("ipc.command_invocation_id_required"))?;
                serde_json::to_value(self.run(command, serde_json::from_value(params)?).await?)?
            }
            "agent.plugin_session.settings" => {
                serde_json::to_value(self.settings(serde_json::from_value(params)?).await?)?
            }
            "agent.plugin_session.set_setting" => {
                serde_json::to_value(self.set_setting(serde_json::from_value(params)?).await?)?
            }
            "agent.plugin_session.stop" => {
                serde_json::to_value(self.stop(serde_json::from_value(params)?).await?)?
            }
            "agent.plugin_session.panel" => {
                serde_json::to_value(self.panel(serde_json::from_value(params)?).await?)?
            }
            "agent.plugin_session.log" => {
                serde_json::to_value(self.log(serde_json::from_value(params)?).await?)?
            }
            "agent.plugin_session.notify" => {
                serde_json::to_value(self.notify(serde_json::from_value(params)?).await?)?
            }
            "agent.plugin_session.status" => {
                serde_json::to_value(self.status(serde_json::from_value(params)?).await?)?
            }
            _ => anyhow::bail!("ipc.route_unknown"),
        })
    }

    /// The conversation's public state.
    pub async fn state(&self, request: PluginSessionRequest) -> anyhow::Result<PluginSessionState> {
        self.check(&request.plugin, PluginCapability::SessionRead).await?;
        let session_id = request.session_id;
        let title = self.title(session_id)?;
        let model_profile_id = self
            .agent
            .execution
            .persistence
            .session_model_binding(session_id)?
            .map(|binding| binding.model_profile_id);
        let model_name = model_profile_id.and_then(|id| {
            let config = self.agent.p05.model_config.snapshot().ok()?;
            config.registry.models.get(&id).map(|model| model.display_name.clone())
        });
        let (activity, usage, todo, context, cursor) = self
            .agent
            .store
            .read(StoreStep::operation("plugin_session.state"), move |db| {
                let activity = under_way::sessions(db)?
                    .sessions
                    .into_iter()
                    .find(|activity| activity.session_id == session_id);
                Ok((
                    activity,
                    plugin_session::usage(db, session_id)?,
                    plugin_session::todo(db, session_id)?,
                    plugin_session::context_use(db, session_id)?,
                    plugin_session::head(db)?,
                ))
            })
            .await?;
        let (activity, waiting_for) = match activity {
            None => (PluginSessionActivity::Idle, None),
            Some(found) if found.waiting_for.is_some() => {
                (PluginSessionActivity::Waiting, found.waiting_for)
            }
            Some(_) => (PluginSessionActivity::Running, None),
        };
        Ok(PluginSessionState {
            session_id,
            title,
            model_profile_id,
            model_name,
            activity,
            waiting_for,
            usage,
            todo,
            context,
            cursor,
        })
    }

    /// What happened after the cursor; the first read only says where now is.
    pub async fn events(
        &self,
        request: PluginSessionEventsRequest,
    ) -> anyhow::Result<PluginSessionEvents> {
        self.check(&request.plugin, PluginCapability::SessionEvents).await?;
        let session_id = request.session_id;
        self.title(session_id)?;
        Ok(self
            .agent
            .store
            .read(StoreStep::operation("plugin_session.events"), move |db| match request.after {
                Some(after) => {
                    plugin_session::events(db, session_id, after, plugin_session::EVENTS_WINDOW)
                }
                None => Ok(PluginSessionEvents {
                    events: Vec::new(),
                    cursor: plugin_session::head(db)?,
                    more: false,
                }),
            })
            .await?)
    }

    /// Sends a message for the user, marked as the plugin's. With a key, the same key again is
    /// the same message and never a second turn.
    pub async fn send(
        &self,
        command: CommandInvocationId,
        request: PluginSessionSendRequest,
    ) -> anyhow::Result<PluginSessionSent> {
        self.check(&request.plugin, PluginCapability::SessionSend).await?;
        let key = request.dedupe_key.as_deref();
        self.deliver(command, &request.plugin, request.session_id, &request.text, key).await
    }

    /// Runs one of the plugin's own prompt commands in the conversation: its template, with the
    /// arguments where it says, sent as the plugin's message as `send` sends one. It runs only
    /// those commands, so it is no way to send other text.
    pub async fn run(
        &self,
        command: CommandInvocationId,
        request: PluginCommandRunRequest,
    ) -> anyhow::Result<PluginSessionSent> {
        self.check(&request.plugin, PluginCapability::Commands).await?;
        let (agent, plugins) = (Arc::clone(&self.agent), Arc::clone(&self.plugins));
        let (plugin, name) = (request.plugin.clone(), request.name.clone());
        // The catalog reads command folders, off the reactor.
        let template = tokio::task::spawn_blocking(move || {
            let user_root = agent.settings.paths().config_root.join("commands");
            let disabled = agent.settings.disabled_commands();
            let catalog = command_catalog(&user_root, &disabled.keys, None, Some(&plugins));
            own_prompt(&catalog.commands, &plugin, &name)
        })
        .await
        .map_err(|error| anyhow::anyhow!("plugin_session.interrupted: {error}"))??;
        let text = expanded(&template, request.arguments.trim());
        let key = request.dedupe_key.as_deref();
        self.deliver(command, &request.plugin, request.session_id, &text, key).await
    }

    /// The values of the plugin's text settings.
    pub async fn settings(
        &self,
        request: PluginSettingsRequest,
    ) -> anyhow::Result<PluginSettingValues> {
        self.with_plugin(&request.plugin, |ready| {
            let manifest = &ready.package.manifest;
            let declared = Standing::Ready(&manifest.declared.capabilities);
            permitted(&manifest.name, declared, PluginCapability::Settings)?;
            Ok(PluginSettingValues { values: ready.text_values() })
        })
        .await
    }

    /// Sets one of the plugin's text settings as the plugin page sets it; its service starts again
    /// with the new value.
    pub async fn set_setting(
        &self,
        request: PluginSettingSetRequest,
    ) -> anyhow::Result<PluginSettingValues> {
        self.check(&request.plugin, PluginCapability::Settings).await?;
        let plugins = Arc::clone(&self.plugins);
        let values = tokio::task::spawn_blocking(move || {
            plugins.set_text_setting(&request.plugin, &request.key, &request.value)
        })
        .await
        .map_err(|error| anyhow::anyhow!("plugin_session.interrupted: {error}"))??;
        Ok(PluginSettingValues { values })
    }

    /// `text` for the user in the conversation, marked as the plugin's: queued as the composer
    /// queues a message. With a key, the same key again is the same message.
    async fn deliver(
        &self,
        command: CommandInvocationId,
        plugin: &str,
        session_id: SessionId,
        text: &str,
        key: Option<&str>,
    ) -> anyhow::Result<PluginSessionSent> {
        checked_text(text)?;
        let command = match key {
            Some(key) => message_command(plugin, session_id, checked_key(key)?),
            None => command,
        };
        self.title(session_id)?;
        let submit = AgentSubmitRequest::new(session_id, text, AgentInputLane::NextTurn);
        let asked = submit.clone();
        let sent_before = self
            .agent
            .store
            .read(StoreStep::operation("plugin_session.sent_before"), move |db| {
                match inbox::submitted(db, command, &asked) {
                    Err(PersistenceCoreError::Conflict) => Ok(None),
                    other => other.map(Some),
                }
            })
            .await?;
        let Some(sent_before) = sent_before else {
            anyhow::bail!(
                "plugin_session.dedupe_key_reused: 这个去重键已经用在另一条消息上，换一个再发"
            );
        };
        if sent_before {
            return Ok(PluginSessionSent { message_id: None, queued: false, repeated: true });
        }
        let sender = MessageSender { plugin: plugin.to_owned(), from: None };
        // The submission's future is large; boxed, it is not carried inline by every call above.
        let submitted = Box::pin(self.agent.submit_sent(command, submit, Some(sender)));
        let (inbox, message) = submitted.await?;
        Ok(PluginSessionSent {
            message_id: message,
            queued: message.is_some_and(|id| inbox.messages.iter().any(|m| m.message_id == id)),
            repeated: message.is_none(),
        })
    }

    /// Stops the turn under way, as the composer's stop does.
    pub async fn stop(
        &self,
        request: PluginSessionRequest,
    ) -> anyhow::Result<PluginSessionStopped> {
        self.check(&request.plugin, PluginCapability::SessionControl).await?;
        let session_id = request.session_id;
        self.title(session_id)?;
        let run = self
            .agent
            .store
            .read(StoreStep::operation("plugin_session.stop"), move |db| {
                session::load(db, session_id)?
                    .active_turn
                    .map(|turn| turns::load(db, turn).map(|turn| turn.agent_run_id))
                    .transpose()
            })
            .await?;
        let Some(agent_run_id) = run else {
            return Ok(PluginSessionStopped { stopped: false });
        };
        self.agent.cancel(&AgentRunRequest { session_id, agent_run_id }).await?;
        Ok(PluginSessionStopped { stopped: true })
    }

    /// What Main needs to show the plugin's panel in this conversation.
    pub async fn panel(&self, request: PluginSessionRequest) -> anyhow::Result<PluginSessionPanel> {
        let panel = self.with_plugin(&request.plugin, |ready| panel_site(&ready.package)).await?;
        self.title(request.session_id)?;
        Ok(panel)
    }

    /// Keeps a problem Main saw in the plugin's panel in the plugin's log.
    pub async fn log(&self, request: PluginPanelLogRequest) -> anyhow::Result<PluginPanelLogged> {
        anyhow::ensure!(
            !request.text.trim().is_empty(),
            "plugin_session.log_empty: 没有要记的内容"
        );
        self.title(request.session_id)?;
        let plugins = Arc::clone(&self.plugins);
        self.with_plugin(&request.plugin, move |ready| {
            let manifest = &ready.package.manifest;
            let part =
                manifest.declared.panel.as_ref().map_or("panel", |panel| panel.entry.as_str());
            plugins.log(&manifest.name, PluginLogSource::Panel, part, &request.text);
            Ok(PluginPanelLogged {})
        })
        .await
    }

    /// A notice for the user, as Main shows it. It belongs to no conversation.
    pub async fn notify(&self, request: PluginNoticeRequest) -> anyhow::Result<PluginNotice> {
        self.check(&request.plugin, PluginCapability::Notify).await?;
        let text = checked_notice(&request.text)?;
        Ok(PluginNotice { plugin: request.plugin, text, tone: request.tone })
    }

    /// The plugin's status entry, as Main shows it; without text it is cleared.
    pub async fn status(&self, request: PluginStatusRequest) -> anyhow::Result<PluginStatus> {
        self.check(&request.plugin, PluginCapability::Status).await?;
        let text = checked_status(request.text.as_deref())?;
        Ok(PluginStatus { plugin: request.plugin, text, tone: request.tone })
    }

    /// The plugin may use `needed`.
    async fn check(&self, plugin: &str, needed: PluginCapability) -> anyhow::Result<()> {
        self.with_plugin(plugin, move |ready| {
            let manifest = &ready.package.manifest;
            permitted(&manifest.name, Standing::Ready(&manifest.declared.capabilities), needed)
        })
        .await
    }

    /// `then` with the plugin as it is trusted now, when it can be used. Reading the plugins can
    /// read their folders, so it runs off the reactor.
    async fn with_plugin<T: Send + 'static>(
        &self,
        plugin: &str,
        then: impl FnOnce(&ReadyPlugin) -> anyhow::Result<T> + Send + 'static,
    ) -> anyhow::Result<T> {
        let plugins = Arc::clone(&self.plugins);
        let name = plugin.to_owned();
        tokio::task::spawn_blocking(move || {
            let ready = plugins.ready();
            match ready.iter().find(|ready| ready.package.manifest.name == name) {
                Some(found) => then(found),
                None => Err(unusable(
                    &name,
                    plugins.state_of(&name).map_or(Standing::Missing, Standing::Installed),
                )),
            }
        })
        .await
        .map_err(|error| anyhow::anyhow!("plugin_session.interrupted: {error}"))?
    }

    /// The conversation's title, which also says it exists.
    fn title(&self, session: SessionId) -> anyhow::Result<String> {
        self.agent
            .execution
            .persistence
            .workspace_session_projection()?
            .sessions
            .into_iter()
            .find(|found| found.session_id == session)
            .map(|found| found.title)
            .ok_or_else(|| anyhow::anyhow!("plugin_session.session_missing: 没有这个会话"))
    }
}
