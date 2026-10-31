//! What Chrome's tools ask of Chrome control: the conversation's shares, the control lock, the
//! inspection made before admission, and the share list the extension's side panel shows.
use super::{ChromeControl, Inner};
use aura_chrome_domain::commands::{ChromeInspection, ChromeOpenedTab, ChromeSessionChoice};
use aura_chrome_domain::protocol::{CommandOutcome, CoreMessage};
use aura_chrome_domain::tools::ChromeTabV1;
use aura_domain_common::{SessionId, ToolCallId};
use std::collections::BTreeSet;
use std::sync::Arc;

/// The conversations a tab can be shared with.
pub trait ChromeSessions: Send + Sync {
    fn list(&self) -> Vec<ChromeSessionChoice>;
}

/// The share a call resolved to.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedTab {
    pub share_id: String,
    pub tab_id: u32,
    pub reference: String,
    pub url: String,
    pub title: String,
    pub origin: String,
    /// Whether the user confirmed the tab's site for this share.
    pub origin_confirmed: bool,
    pub opened_by_aura: bool,
}

impl ResolvedTab {
    fn of(share: &super::shares::Share) -> Self {
        let origin = share.origin();
        Self {
            share_id: share.share_id.clone(),
            tab_id: share.tab_id,
            reference: share.reference.clone(),
            url: share.url.clone(),
            title: share.title.clone(),
            origin_confirmed: share.pending_origin.is_none()
                && share.confirmed_origins.contains(&origin),
            origin,
            opened_by_aura: share.opened_by_aura,
        }
    }
}

/// What the first resolution of a call read, so that every later resolution of it reads the
/// same: the answer the user gives to a call must not change the call it answered.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CallSites {
    /// The sites the user confirmed for all the conversation's tabs.
    pub session: BTreeSet<String>,
    /// For `chrome.open`: the conversation's tab it uses again, and whether that tab was at the
    /// address already.
    pub reuse: Option<(u32, bool)>,
}

/// One conversation's hold on Chrome while one of its calls runs.
pub struct ControlGuard {
    inner: Arc<Inner>,
    session: SessionId,
    /// The Stop count when the call began: a Stop since then already released the lock, which
    /// may now be another conversation's.
    stops: u64,
}

impl Drop for ControlGuard {
    fn drop(&mut self) {
        let mut state = self.inner.lock();
        if state.stops != self.stops {
            return;
        }
        if let Some((session, count)) = state.holder.as_mut()
            && *session == self.session
        {
            *count = count.saturating_sub(1);
            if *count == 0 {
                state.holder = None;
            }
        }
    }
}

/// Inspections and pinned sites kept for admission; the oldest go first.
const INSPECTIONS_KEPT: usize = 256;

impl ChromeControl {
    pub fn set_sessions(&self, sessions: Arc<dyn ChromeSessions>) {
        *self.inner.sessions.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(sessions);
    }

    /// Whether an extension is connected and has proved its pairing.
    pub fn connected(&self) -> bool {
        self.inner.lock().live.as_ref().is_some_and(|live| live.authenticated)
    }

    pub fn has_shares(&self, session: SessionId) -> bool {
        !self.inner.lock().shares.tabs(session).is_empty()
    }

    pub fn tabs(&self, session: SessionId) -> Vec<ChromeTabV1> {
        self.inner.lock().shares.tabs(session)
    }

    pub fn resolve_tab(
        &self,
        session: SessionId,
        reference: Option<&str>,
    ) -> Result<ResolvedTab, String> {
        let state = self.inner.lock();
        state.shares.resolve(session, reference).map(ResolvedTab::of)
    }

    /// The conversation's share of this tab, while it lasts.
    pub fn session_tab(&self, session: SessionId, tab_id: u32) -> Option<ResolvedTab> {
        self.inner.lock().shares.of_session(session, tab_id).map(ResolvedTab::of)
    }

    /// The tab Aura opened for the conversation that is on `origin` now, to open it again in.
    pub fn opened_on(&self, session: SessionId, origin: &str) -> Option<ResolvedTab> {
        self.inner.lock().shares.opened_on(session, origin).map(ResolvedTab::of)
    }

    /// The tab becomes the conversation's current tab.
    pub fn make_current(&self, tab_id: u32) {
        self.inner.lock().shares.make_current(tab_id);
    }

    /// The sites the conversation confirmed for all its tabs, as Core read them.
    pub fn set_session_sites(&self, session: SessionId, sites: BTreeSet<String>) {
        if self.inner.lock().shares.set_session_sites(session, sites) {
            self.inner.broadcast_shares();
        }
    }

    pub fn pinned(&self, call: ToolCallId) -> Option<CallSites> {
        self.inner.lock().calls.get(&call).cloned()
    }

    pub fn pin(&self, call: ToolCallId, sites: CallSites) {
        let mut state = self.inner.lock();
        state.calls.insert(call, sites);
        while state.calls.len() > INSPECTIONS_KEPT {
            state.calls.pop_first();
        }
    }

    /// Chrome for this conversation, for as long as the guard lives. Another conversation's call
    /// fails at once, and so does every call while the user has a shared tab.
    pub fn hold(&self, session: SessionId) -> Result<ControlGuard, String> {
        let (holder, stops) =
            {
                let mut state = self.inner.lock();
                if state.shares.views(|_| String::new()).iter().any(|view| {
                    view.state == aura_chrome_domain::commands::ChromeShareState::WithUser
                }) {
                    return Err(
                        "chrome.with_user: 用户正在操作共享的标签页；等用户完成后再继续。".into()
                    );
                }
                let holder = match state.holder.as_mut() {
                    Some((other, _)) if *other != session => Some(*other),
                    Some((_, count)) => {
                        *count += 1;
                        None
                    }
                    None => {
                        state.holder = Some((session, 1));
                        None
                    }
                };
                (holder, state.stops)
            };
        if let Some(other) = holder {
            return Err(format!(
                "chrome.busy: Chrome 正由会话「{}」使用，请稍后再试。",
                self.inner.session_title(other)
            ));
        }
        Ok(ControlGuard { inner: Arc::clone(&self.inner), session, stops })
    }

    /// A tab Aura opened becomes this conversation's current share; its reference.
    pub fn add_opened(&self, session: SessionId, opened: &ChromeOpenedTab) -> String {
        let reference = self
            .inner
            .lock()
            .shares
            .add(
                session,
                opened.tab_id,
                &opened.url,
                &opened.title,
                true,
                aura_chrome_domain::commands::ChromeShareScope::Session,
            )
            .reference
            .clone();
        self.inner.broadcast_shares();
        reference
    }

    /// The call was authorized on this site; the share keeps it.
    pub fn confirm_origin(&self, tab_id: u32, origin: &str) {
        if self.inner.lock().shares.confirm(tab_id, origin) {
            self.inner.broadcast_shares();
        }
    }

    /// Where a tab is after a command, as the extension reported it.
    pub fn tab_changed(&self, tab_id: u32, url: &str, title: &str) {
        self.inner.lock().shares.changed(tab_id, url, title);
        self.inner.broadcast_shares();
    }

    /// The conversation's turn ended; the shares meant for that turn end now.
    pub fn turn_ended(&self, session: SessionId) {
        if !self.inner.lock().shares.end_turn(session).is_empty() {
            self.inner.broadcast_shares();
        }
    }

    pub fn remove_tab(&self, tab_id: u32) {
        self.inner.lock().shares.remove(tab_id);
        self.inner.broadcast_shares();
    }

    /// The user has the tab, for the reason the model gave, or has given it back.
    pub fn set_with_user(&self, tab_id: u32, reason: Option<&str>) {
        self.inner.lock().shares.set_with_user(tab_id, reason.map(str::to_owned));
        self.inner.broadcast_shares();
    }

    /// The conversation's shared tabs, for the line above its message box.
    pub fn session_shares(&self, session: SessionId) -> aura_chrome_domain::ChromeSessionShares {
        let connected = self.connected();
        aura_chrome_domain::ChromeSessionShares {
            connected,
            tabs: self.inner.lock().shares.shared_tabs(session),
        }
    }

    /// The user pressed 完成 in Aura for the tab the model handed them: the extension ends the
    /// wait as the user's "done".
    pub fn finish_assist(&self, session: SessionId) -> anyhow::Result<()> {
        let state = self.inner.lock();
        let tab_id = state
            .shares
            .with_user(session)
            .ok_or_else(|| anyhow::anyhow!("chrome.no_assist: 这个会话没有交给你操作的标签页。"))?;
        let live = state
            .live
            .as_ref()
            .filter(|live| live.authenticated)
            .ok_or_else(|| anyhow::anyhow!("chrome.not_connected: Chrome 没有连接 Aura。"))?;
        live.outgoing
            .try_send(CoreMessage::AssistDone { tab_id })
            .map_err(|_| anyhow::anyhow!("chrome.not_connected: Chrome 没有连接 Aura。"))
    }

    pub fn remember_inspection(&self, call: ToolCallId, inspection: ChromeInspection) {
        let mut state = self.inner.lock();
        state.inspections.insert(call, inspection);
        while state.inspections.len() > INSPECTIONS_KEPT {
            state.inspections.pop_first();
        }
    }

    pub fn inspection(&self, call: ToolCallId) -> Option<ChromeInspection> {
        self.inner.lock().inspections.get(&call).cloned()
    }

    /// Stops what Chrome is doing for one conversation: its shares end and the extension leaves
    /// those tabs; when it holds Chrome, its commands still out fail and the lock is released.
    /// Other conversations' shares and calls go on.
    pub fn stop_session(&self, session: SessionId) {
        let tab_ids = self.inner.stop_session(session);
        let state = self.inner.lock();
        if let Some(live) = state.live.as_ref() {
            // Fire and forget: id 0 is never a waiting command.
            let release = aura_chrome_domain::commands::ChromeCommand::Release { tab_ids };
            if let Ok(command) = serde_json::to_value(release) {
                let _ =
                    live.outgoing.try_send(CoreMessage::Command { id: 0, command, origin: None });
            }
        }
    }
}

impl Inner {
    pub(super) fn session_choices(&self) -> Vec<ChromeSessionChoice> {
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .map(|sessions| sessions.list())
            .unwrap_or_default()
    }

    pub(super) fn session_title(&self, session: SessionId) -> String {
        self.session_choices()
            .into_iter()
            .find(|choice| choice.session_id == session.to_string())
            .map_or_else(|| "未命名会话".to_owned(), |choice| choice.title)
    }

    /// Sends the side panel every share, after any change.
    pub(super) fn broadcast_shares(&self) {
        let choices = self.session_choices();
        let title = |session: SessionId| {
            choices
                .iter()
                .find(|choice| choice.session_id == session.to_string())
                .map_or_else(|| "未命名会话".to_owned(), |choice| choice.title.clone())
        };
        let state = self.lock();
        let shares = state.shares.views(title);
        if let Some(live) = state.live.as_ref().filter(|live| live.authenticated) {
            let _ = live.outgoing.try_send(CoreMessage::Shares { shares });
        }
    }

    /// Ends every share and fails every command still out; the tabs that were shared.
    pub(super) fn stop_all(&self) -> Vec<u32> {
        let tab_ids = {
            let mut state = self.lock();
            let tabs = state.shares.clear().into_iter().map(|share| share.tab_id).collect();
            state.release_lock();
            tabs
        };
        self.broadcast_shares();
        tab_ids
    }

    /// Ends the conversation's shares; when it holds Chrome, every command still out is its own
    /// and fails, and the lock is released. The tabs that were its shares.
    pub(super) fn stop_session(&self, session: SessionId) -> Vec<u32> {
        let tab_ids = {
            let mut state = self.lock();
            let tabs =
                state.shares.end_session(session).into_iter().map(|share| share.tab_id).collect();
            if state.holder.as_ref().is_some_and(|(holder, _)| *holder == session) {
                state.release_lock();
            }
            tabs
        };
        self.broadcast_shares();
        tab_ids
    }
}

impl super::State {
    /// Releases the lock and fails the commands still out; calls begun before this release
    /// nothing when they end.
    fn release_lock(&mut self) {
        self.holder = None;
        self.stops += 1;
        if let Some(live) = self.live.as_mut() {
            for (_, waiter) in std::mem::take(&mut live.pending) {
                let _ = waiter.send(CommandOutcome::Failed {
                    code: "chrome.stopped".into(),
                    message: "用户停止了 Aura 在 Chrome 里的操作。".into(),
                });
            }
        }
    }
}

/// The sidebar's conversations, for the extension to share a tab with; archived ones are left out.
pub struct PersistedSessions(pub Arc<aura_persistence::PersistenceCoordinator>);

impl ChromeSessions for PersistedSessions {
    fn list(&self) -> Vec<ChromeSessionChoice> {
        let Ok(projection) = self.0.workspace_session_projection() else { return Vec::new() };
        projection
            .sessions
            .iter()
            .filter(|session| session.state != "archived")
            .map(|session| ChromeSessionChoice {
                session_id: session.session_id.to_string(),
                title: if session.title.trim().is_empty() {
                    "新会话".to_owned()
                } else {
                    session.title.clone()
                },
                workspace: projection
                    .workspaces
                    .iter()
                    .find(|workspace| workspace.workspace_id == session.workspace_id)
                    .map(|workspace| workspace.display_name.clone())
                    .unwrap_or_default(),
            })
            .collect()
    }
}
