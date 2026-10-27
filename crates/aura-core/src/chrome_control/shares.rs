//! The tabs shared with each conversation. They live in memory only, so a restart of Core or of
//! Chrome ends them. A share keeps the sites the user confirmed for it; a tab that moves elsewhere
//! waits for the user before the model acts in it again.
use aura_chrome_domain::commands::{ChromeShareScope, ChromeShareState, ChromeShareView};
use aura_chrome_domain::tools::ChromeTabV1;
use aura_chrome_domain::{ChromeSharedTab, ChromeSharedTabState};
use aura_domain_common::SessionId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub(crate) struct Share {
    pub share_id: String,
    pub session_id: SessionId,
    pub tab_id: u32,
    /// The model's name for the tab within its conversation (t1, t2…).
    pub reference: String,
    pub url: String,
    pub title: String,
    pub confirmed_origins: BTreeSet<String>,
    pub opened_by_aura: bool,
    /// A site the tab moved to that the user has not confirmed for this share.
    pub pending_origin: Option<String>,
    /// What the model asked the user to do, while the user has the tab.
    pub with_user: Option<String>,
    pub scope: ChromeShareScope,
}

impl Share {
    pub(crate) fn origin(&self) -> String {
        origin_of(&self.url)
    }

    fn state(&self) -> ChromeShareState {
        if self.with_user.is_some() {
            ChromeShareState::WithUser
        } else if self.pending_origin.is_some() {
            ChromeShareState::AwaitingOrigin
        } else {
            ChromeShareState::Active
        }
    }
}

/// `scheme://host[:port]` of an address, lowercase; the address itself when it has no host.
pub(crate) fn origin_of(url: &str) -> String {
    let lower = url.trim().to_ascii_lowercase();
    let Some((scheme, rest)) = lower.split_once("://") else { return lower };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host = authority.rsplit('@').next().unwrap_or(authority);
    format!("{scheme}://{host}")
}

#[derive(Default)]
pub(crate) struct Shares {
    by_tab: BTreeMap<u32, Share>,
    next_reference: BTreeMap<SessionId, u32>,
    current: BTreeMap<SessionId, u32>,
    next_share: u64,
}

impl Shares {
    /// Shares `tab_id` with `session`; a tab shared with another conversation moves to this one.
    pub(crate) fn add(
        &mut self,
        session_id: SessionId,
        tab_id: u32,
        url: &str,
        title: &str,
        opened_by_aura: bool,
        scope: ChromeShareScope,
    ) -> &Share {
        if let Some(existing) = self.by_tab.get(&tab_id)
            && existing.session_id != session_id
        {
            self.remove(tab_id);
        }
        if !self.by_tab.contains_key(&tab_id) {
            self.next_share += 1;
            let number = self.next_reference.entry(session_id).or_default();
            *number += 1;
            let share = Share {
                share_id: format!("share-{}", self.next_share),
                session_id,
                tab_id,
                reference: format!("t{number}"),
                url: url.to_owned(),
                title: title.to_owned(),
                confirmed_origins: BTreeSet::from([origin_of(url)]),
                opened_by_aura,
                pending_origin: None,
                with_user: None,
                scope,
            };
            self.by_tab.insert(tab_id, share);
        }
        self.current.insert(session_id, tab_id);
        &self.by_tab[&tab_id]
    }

    pub(crate) fn remove(&mut self, tab_id: u32) -> Option<Share> {
        let share = self.by_tab.remove(&tab_id)?;
        if self.current.get(&share.session_id) == Some(&tab_id) {
            let next = self.by_tab.values().find(|other| other.session_id == share.session_id);
            match next.map(|other| other.tab_id) {
                Some(tab) => self.current.insert(share.session_id, tab),
                None => self.current.remove(&share.session_id),
            };
        }
        Some(share)
    }

    /// The conversation's turn ended: the shares meant for one turn end with it.
    pub(crate) fn end_turn(&mut self, session_id: SessionId) -> Vec<Share> {
        let ended = self
            .by_tab
            .values()
            .filter(|share| share.session_id == session_id && share.scope == ChromeShareScope::Turn)
            .map(|share| share.tab_id)
            .collect::<Vec<_>>();
        ended.into_iter().filter_map(|tab_id| self.remove(tab_id)).collect()
    }

    /// Every share ends, as when the user presses Stop.
    pub(crate) fn clear(&mut self) -> Vec<Share> {
        self.current.clear();
        std::mem::take(&mut self.by_tab).into_values().collect()
    }

    /// The tab moved or retitled itself; a site not confirmed for the share pauses it.
    pub(crate) fn changed(&mut self, tab_id: u32, url: &str, title: &str) -> Option<&Share> {
        let share = self.by_tab.get_mut(&tab_id)?;
        share.url = url.to_owned();
        share.title = title.to_owned();
        let origin = origin_of(url);
        share.pending_origin = (!share.confirmed_origins.contains(&origin)).then_some(origin);
        Some(share)
    }

    /// The user's answer about a site; `false` ends the share.
    pub(crate) fn answer(&mut self, tab_id: u32, origin: &str, allow: bool) -> bool {
        if !allow {
            return self.remove(tab_id).is_some();
        }
        let Some(share) = self.by_tab.get_mut(&tab_id) else { return false };
        share.confirmed_origins.insert(origin.to_owned());
        if share.pending_origin.as_deref() == Some(origin) {
            share.pending_origin = None;
        }
        true
    }

    pub(crate) fn set_with_user(&mut self, tab_id: u32, with_user: Option<String>) {
        if let Some(share) = self.by_tab.get_mut(&tab_id) {
            share.with_user = with_user;
        }
    }

    /// The share a model call names: its tab reference, or the conversation's current tab.
    pub(crate) fn resolve(
        &self,
        session_id: SessionId,
        reference: Option<&str>,
    ) -> Result<&Share, String> {
        let found = match reference {
            Some(reference) => self
                .by_tab
                .values()
                .find(|share| share.session_id == session_id && share.reference == reference),
            None => self.current.get(&session_id).and_then(|tab| self.by_tab.get(tab)),
        };
        found.ok_or_else(|| match reference {
            Some(reference) => format!(
                "chrome.no_tab: 本会话没有共享标签页 {reference}；用 chrome.tabs 查看可用的标签页。"
            ),
            None => "chrome.no_share: 用户还没有把 Chrome 标签页共享给本会话。请用户在 Chrome 的 Aura 侧边栏点“共享给 Aura”，或用 chrome.open 打开新标签页。".to_owned(),
        })
    }

    #[cfg(test)]
    pub(crate) fn of_tab(&self, tab_id: u32) -> Option<&Share> {
        self.by_tab.get(&tab_id)
    }

    /// The conversation's shares as Aura shows them, in reference order.
    pub(crate) fn shared_tabs(&self, session_id: SessionId) -> Vec<ChromeSharedTab> {
        let mut tabs = self
            .by_tab
            .values()
            .filter(|share| share.session_id == session_id)
            .map(|share| ChromeSharedTab {
                tab: share.reference.clone(),
                title: share.title.clone(),
                url: share.url.clone(),
                state: match share.state() {
                    ChromeShareState::Active => ChromeSharedTabState::Active,
                    ChromeShareState::AwaitingOrigin => ChromeSharedTabState::AwaitingOrigin,
                    ChromeShareState::WithUser => ChromeSharedTabState::WithUser,
                },
                this_turn_only: share.scope == ChromeShareScope::Turn,
                assist_reason: share.with_user.clone(),
            })
            .collect::<Vec<_>>();
        tabs.sort_by_key(|tab| tab.tab[1..].parse::<u32>().unwrap_or(u32::MAX));
        tabs
    }

    /// The conversation's tab the user has now, at the model's request.
    pub(crate) fn with_user(&self, session_id: SessionId) -> Option<u32> {
        self.by_tab
            .values()
            .find(|share| share.session_id == session_id && share.with_user.is_some())
            .map(|share| share.tab_id)
    }

    pub(crate) fn tabs(&self, session_id: SessionId) -> Vec<ChromeTabV1> {
        let current = self.current.get(&session_id).copied();
        let mut tabs = self
            .by_tab
            .values()
            .filter(|share| share.session_id == session_id)
            .map(|share| ChromeTabV1 {
                tab: share.reference.clone(),
                title: share.title.clone(),
                url: share.url.clone(),
                current: Some(share.tab_id) == current,
                opened_by_aura: share.opened_by_aura,
            })
            .collect::<Vec<_>>();
        tabs.sort_by_key(|tab| tab.tab[1..].parse::<u32>().unwrap_or(u32::MAX));
        tabs
    }

    pub(crate) fn views(&self, title: impl Fn(SessionId) -> String) -> Vec<ChromeShareView> {
        self.by_tab
            .values()
            .map(|share| ChromeShareView {
                tab_id: share.tab_id,
                session_id: share.session_id.to_string(),
                session_title: title(share.session_id),
                origin: share.pending_origin.clone().unwrap_or_else(|| share.origin()),
                state: share.state(),
                scope: share.scope,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shares_are_named_per_conversation_and_a_new_site_waits_for_the_user() {
        let mut shares = Shares::default();
        let (one, two) = (SessionId::new(), SessionId::new());
        assert_eq!(
            shares
                .add(one, 10, "https://a.example/x", "A", false, ChromeShareScope::Session)
                .reference,
            "t1"
        );
        assert_eq!(
            shares
                .add(one, 11, "https://b.example/", "B", true, ChromeShareScope::Session)
                .reference,
            "t2"
        );
        assert_eq!(
            shares
                .add(two, 12, "http://localhost:5173/", "C", false, ChromeShareScope::Session)
                .reference,
            "t1"
        );
        assert_eq!(shares.resolve(one, None).expect("current").tab_id, 11);
        assert_eq!(shares.resolve(one, Some("t1")).expect("t1").tab_id, 10);
        assert!(shares.resolve(two, Some("t2")).is_err(), "another conversation's tab");
        assert_eq!(
            shares.tabs(one).iter().map(|t| t.tab.as_str()).collect::<Vec<_>>(),
            ["t1", "t2"]
        );
        assert!(shares.tabs(two).iter().all(|tab| tab.tab == "t1"));
        let moved = shares.changed(10, "https://evil.example/login", "Login").expect("share");
        assert_eq!(moved.pending_origin.as_deref(), Some("https://evil.example"));
        assert!(
            shares.changed(10, "https://a.example/y", "A").expect("back").pending_origin.is_none()
        );
        shares.changed(10, "https://c.example/", "C");
        assert!(shares.answer(10, "https://c.example", true));
        assert!(shares.of_tab(10).expect("kept").pending_origin.is_none());
        assert!(shares.answer(11, "https://d.example", false), "refusing ends the share");
        assert!(shares.of_tab(11).is_none());
        assert_eq!(shares.resolve(one, None).expect("next current").tab_id, 10);
        assert_eq!(origin_of("https://user:pw@Host.Example:8443/p?q"), "https://host.example:8443");
    }

    #[test]
    fn a_turns_end_ends_only_that_conversations_shares_for_the_turn() {
        let mut shares = Shares::default();
        let (one, two) = (SessionId::new(), SessionId::new());
        shares.add(one, 1, "https://a.example/", "A", false, ChromeShareScope::Turn);
        shares.add(one, 2, "https://b.example/", "B", false, ChromeShareScope::Session);
        shares.add(two, 3, "https://c.example/", "C", false, ChromeShareScope::Turn);
        let ended = shares.end_turn(one);
        assert_eq!(ended.iter().map(|share| share.tab_id).collect::<Vec<_>>(), [1]);
        assert!(shares.of_tab(2).is_some() && shares.of_tab(3).is_some());
        assert_eq!(shares.resolve(one, None).expect("the conversation's share").tab_id, 2);
    }
}
