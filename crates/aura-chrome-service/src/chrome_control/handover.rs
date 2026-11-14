//! The user's Chrome tabs for Aura's @ menu, and a tab the user hands to a conversation with @:
//! it is read once for the message it goes with, then shared with the conversation as the side
//! panel's 共享给 Aura would share it, until the user stops sharing it.
use super::shares::origin_of;
use super::{ChromeControl, ChromeRequestError};
use aura_chrome_domain::commands::{
    ChromeCommand, ChromeOpenedTab, ChromePageReport, ChromeShareScope, ChromeTabsReport,
};
use aura_chrome_domain::protocol::CoreMessage;
use aura_chrome_domain::{ChromeHandedTab, ChromeTabChoice, ChromeTabChoices};
use aura_domain_common::SessionId;
use std::time::Duration;

/// How long the extension has to list the tabs; the user is waiting on the menu.
const LIST_TIMEOUT: Duration = Duration::from_secs(5);
/// How long reading a page may take, as for `chrome.snapshot`.
const READ_TIMEOUT: Duration = Duration::from_secs(20);
/// As much of the page as `chrome.snapshot` reads by default.
const READ_CHARS: u32 = 12_000;

impl ChromeControl {
    /// The user's ordinary tabs; an extension too old to list them is named as the issue.
    pub async fn tab_choices(&self) -> ChromeTabChoices {
        if !self.connected() {
            return ChromeTabChoices { connected: false, tabs: Vec::new(), issue: None };
        }
        let (tabs, issue) = match self.listed().await {
            Ok(tabs) => (tabs, None),
            Err(issue) => (Vec::new(), Some(issue)),
        };
        ChromeTabChoices {
            connected: true,
            tabs: tabs
                .into_iter()
                .map(|tab| ChromeTabChoice { tab_id: tab.tab_id, title: tab.title, url: tab.url })
                .collect(),
            issue,
        }
    }

    /// The tabs the extension lists now, without a page Aura may not have.
    async fn listed(&self) -> Result<Vec<ChromeOpenedTab>, String> {
        let command =
            serde_json::to_value(ChromeCommand::Tabs).map_err(|error| error.to_string())?;
        let report = match self.request(command, None, LIST_TIMEOUT).await {
            Ok(value) => serde_json::from_value::<ChromeTabsReport>(value)
                .map_err(|_| "Chrome 扩展列出的标签页无法读取。".to_owned())?,
            Err(ChromeRequestError::Failed { code, .. }) if code == "chrome.unsupported" => {
                return Err("Chrome 里的 Aura 扩展需要更新，才能在这里选标签页。".to_owned());
            }
            Err(ChromeRequestError::Failed { message, .. }) => return Err(message),
            Err(ChromeRequestError::TimedOut) => {
                return Err("Chrome 没有及时列出标签页，请重试。".to_owned());
            }
            Err(ChromeRequestError::NotConnected | ChromeRequestError::Disconnected) => {
                return Err("Chrome 没有连接 Aura。".to_owned());
            }
        };
        Ok(report
            .tabs
            .into_iter()
            .filter(|tab| !aura_security_contract::chrome_origin_blocked(&tab.url))
            .collect())
    }

    /// The user handed `tab_id` to the conversation with @: the page is read on the site the tab
    /// is on now, and only then is the tab shared, so a page that cannot be read shares nothing.
    pub async fn hand_over_tab(
        &self,
        session: SessionId,
        tab_id: u32,
    ) -> anyhow::Result<ChromeHandedTab> {
        anyhow::ensure!(self.connected(), "chrome.not_connected: Chrome 没有连接 Aura。");
        anyhow::ensure!(
            self.inner
                .session_choices()
                .iter()
                .any(|choice| choice.session_id == session.to_string()),
            "chrome.no_session: 这个会话已不存在。"
        );
        let tabs = self
            .listed()
            .await
            .map_err(|issue| anyhow::anyhow!("chrome.tabs_unavailable: {issue}"))?;
        let tab = tabs.into_iter().find(|tab| tab.tab_id == tab_id).ok_or_else(|| {
            anyhow::anyhow!("chrome.tab_gone: 这个标签页已经关闭，或不能交给 Aura。")
        })?;
        let read = ChromeCommand::Snapshot { tab_id, max_chars: READ_CHARS };
        let page = match self
            .request(serde_json::to_value(read)?, Some(origin_of(&tab.url)), READ_TIMEOUT)
            .await
        {
            Ok(value) => serde_json::from_value::<ChromePageReport>(value)
                .map_err(|_| anyhow::anyhow!("chrome.unreadable: Chrome 扩展读到的页面无法读取。")),
            Err(error) => Err(read_failed(error)),
        };
        let page = match page {
            Ok(page) if !aura_security_contract::chrome_origin_blocked(&page.url) => page,
            Ok(_) => {
                self.release_unshared(tab_id);
                anyhow::bail!("chrome.tab_gone: 这个标签页已经关闭，或不能交给 Aura。");
            }
            Err(error) => {
                self.release_unshared(tab_id);
                return Err(error);
            }
        };
        let tab = self
            .inner
            .lock()
            .shares
            .add(session, tab_id, &page.url, &page.title, false, ChromeShareScope::Session)
            .reference
            .clone();
        self.inner.broadcast_shares();
        Ok(ChromeHandedTab {
            tab,
            title: page.title,
            url: page.url,
            snapshot: page.snapshot,
            snapshot_truncated: page.snapshot_truncated,
        })
    }

    /// The extension leaves a tab it read for nothing, unless a share still uses it.
    fn release_unshared(&self, tab_id: u32) {
        let state = self.inner.lock();
        if state.shares.views(|_| String::new()).iter().any(|view| view.tab_id == tab_id) {
            return;
        }
        if let (Some(live), Ok(command)) = (
            state.live.as_ref(),
            serde_json::to_value(ChromeCommand::Release { tab_ids: vec![tab_id] }),
        ) {
            // Fire and forget: id 0 is never a waiting command.
            let _ = live.outgoing.try_send(CoreMessage::Command { id: 0, command, origin: None });
        }
    }
}

fn read_failed(error: ChromeRequestError) -> anyhow::Error {
    match error {
        ChromeRequestError::Failed { code, message } => anyhow::anyhow!("{code}: {message}"),
        ChromeRequestError::TimedOut => {
            anyhow::anyhow!("chrome.timed_out: Chrome 没有及时读完这个页面，请重试。")
        }
        ChromeRequestError::NotConnected | ChromeRequestError::Disconnected => {
            anyhow::anyhow!("chrome.not_connected: Chrome 没有连接 Aura。")
        }
    }
}
