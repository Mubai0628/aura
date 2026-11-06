//! Approvals a chat decides: which of an approval's decisions a chat may give, what the chat is
//! told when one is handed to it, the one each plugin has handed out and those waiting behind it,
//! and the rules a decision from a chat must meet. Kept in memory only, like the replies owed.
use aura_agent_domain::{PluginChat, PluginChatKind};
use aura_domain_common::{ApprovalRequestId, SessionId};
use aura_security_contract::ApprovalDecisionKind;
use std::collections::{BTreeMap, VecDeque};
use std::time::{Duration, Instant};

/// How long after an approval is handed to a chat, or the permission modes are listed in it, a
/// decision from that chat is taken.
pub const REMOTE_DECISION_DELAY: Duration = Duration::from_secs(3);
/// The most approvals one plugin's chats wait for at once; the oldest goes first.
pub const REMOTE_APPROVALS_MAX: usize = 100;

/// A decision a chat may give.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoteDecision {
    Approve,
    ApproveSession,
    Deny,
}

impl RemoteDecision {
    /// Its name in the `approval` message.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::ApproveSession => "approve_session",
            Self::Deny => "deny",
        }
    }

    /// The command a chat gives it with.
    #[must_use]
    pub const fn command(self) -> &'static str {
        match self {
            Self::Approve => "approve",
            Self::ApproveSession => "approve-session",
            Self::Deny => "deny",
        }
    }

    #[must_use]
    pub const fn kind(self) -> ApprovalDecisionKind {
        match self {
            Self::Approve => ApprovalDecisionKind::AllowOnce,
            Self::ApproveSession => ApprovalDecisionKind::AllowSession,
            Self::Deny => ApprovalDecisionKind::Deny,
        }
    }

    /// What the chat is told once it is recorded.
    #[must_use]
    pub const fn done(self) -> &'static str {
        match self {
            Self::Approve => "已批准（仅此一次）。",
            Self::ApproveSession => "已批准（本会话）。",
            Self::Deny => "已拒绝。",
        }
    }
}

/// The decisions a chat may give for an approval that allows `allowed`: allowing once or for the
/// conversation, and refusing. An approval that may only be allowed once is of the riskiest kind,
/// and one that may only be allowed for the turn or the workspace is not decided from a chat:
/// a chat may only refuse those.
#[must_use]
pub fn remote_decisions(allowed: &[ApprovalDecisionKind]) -> Vec<RemoteDecision> {
    use ApprovalDecisionKind::{AllowOnce, AllowSession, Deny};
    let once_only = allowed.iter().all(|decision| matches!(decision, AllowOnce | Deny));
    let mut decisions = Vec::new();
    if allowed.contains(&AllowOnce) && !once_only {
        decisions.push(RemoteDecision::Approve);
    }
    if allowed.contains(&AllowSession) {
        decisions.push(RemoteDecision::ApproveSession);
    }
    if allowed.contains(&Deny) {
        decisions.push(RemoteDecision::Deny);
    }
    decisions
}

/// What a chat without buttons is told: the action, what it does, and how to answer.
#[must_use]
pub fn handed_text(short: &str, title: &str, detail: &str, decisions: &[RemoteDecision]) -> String {
    let mut text = format!("Aura 请你决定（编号 {short}）：{title}");
    if !detail.is_empty() {
        text.push('\n');
        text.push_str(detail);
    }
    let answers: Vec<String> = decisions
        .iter()
        .map(|decision| match decision {
            RemoteDecision::Approve => "/approve 批准这一次".to_owned(),
            RemoteDecision::ApproveSession => "/approve-session 本会话都允许".to_owned(),
            RemoteDecision::Deny => "/deny 拒绝".to_owned(),
        })
        .collect();
    if !decisions.iter().any(|decision| *decision != RemoteDecision::Deny) {
        text.push_str("\n这条只能在电脑上的 Aura 里批准。");
    }
    if !answers.is_empty() {
        text.push_str("\n回复 ");
        text.push_str(&answers.join("，"));
        text.push('。');
    }
    text
}

/// An approval of a turn a chat is owed the reply of.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatApproval {
    pub approval: ApprovalRequestId,
    pub session: SessionId,
    pub chat: PluginChat,
}

/// The approval a plugin handed to one of its chats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handed {
    /// The number the chat names it by.
    pub short: String,
    pub approval: ChatApproval,
    pub decisions: Vec<RemoteDecision>,
    pub at: Instant,
}

#[derive(Default)]
struct PluginApprovals {
    out: Option<Handed>,
    waiting: VecDeque<ChatApproval>,
    numbered: u64,
    /// When each chat was last shown the permission modes.
    listed: BTreeMap<String, Instant>,
}

/// Each plugin's approval out to one of its chats and those waiting behind it, the oldest first.
#[derive(Default)]
pub struct RemoteApprovals {
    plugins: BTreeMap<String, PluginApprovals>,
}

fn chat_key(chat: &PluginChat) -> String {
    let kind = match chat.kind {
        PluginChatKind::Private => "private",
        PluginChatKind::Group => "group",
    };
    [chat.channel.as_str(), kind, chat.id.as_str()].join("\u{1f}")
}

impl RemoteApprovals {
    /// The approval waits to be handed to its chat, unless it is out or waiting already. Only a
    /// private chat is handed one.
    pub fn offer(&mut self, plugin: &str, approval: ChatApproval) {
        if approval.chat.kind != PluginChatKind::Private {
            return;
        }
        let entry = self.plugins.entry(plugin.to_owned()).or_default();
        let known =
            entry.out.as_ref().is_some_and(|out| out.approval.approval == approval.approval)
                || entry.waiting.iter().any(|waiting| waiting.approval == approval.approval);
        if known {
            return;
        }
        if entry.waiting.len() >= REMOTE_APPROVALS_MAX {
            entry.waiting.pop_front();
        }
        entry.waiting.push_back(approval);
    }

    /// Every approval out or waiting, to read which are still undecided.
    #[must_use]
    pub fn all(&self) -> Vec<(String, ChatApproval)> {
        let mut all = Vec::new();
        for (plugin, entry) in &self.plugins {
            let out = entry.out.iter().map(|out| &out.approval);
            for approval in out.chain(entry.waiting.iter()) {
                all.push((plugin.clone(), approval.clone()));
            }
        }
        all
    }

    /// The approval waits for nobody now: it was decided, it expired, or its turn is gone.
    pub fn closed(&mut self, approval: ApprovalRequestId) {
        for entry in self.plugins.values_mut() {
            if entry.out.as_ref().is_some_and(|out| out.approval.approval == approval) {
                entry.out = None;
            }
            entry.waiting.retain(|waiting| waiting.approval != approval);
        }
    }

    /// The approval each plugin with none out would hand out next.
    #[must_use]
    pub fn next(&self) -> Vec<(String, ChatApproval)> {
        self.plugins
            .iter()
            .filter(|(_, entry)| entry.out.is_none())
            .filter_map(|(plugin, entry)| Some((plugin.clone(), entry.waiting.front()?.clone())))
            .collect()
    }

    /// The approval went out to its chat; the number the chat names it by. None while another of
    /// the plugin's is out, or when it no longer waits.
    pub fn handed(
        &mut self,
        plugin: &str,
        approval: ApprovalRequestId,
        decisions: Vec<RemoteDecision>,
        now: Instant,
    ) -> Option<String> {
        let entry = self.plugins.get_mut(plugin)?;
        if entry.out.is_some() {
            return None;
        }
        let at = entry.waiting.iter().position(|waiting| waiting.approval == approval)?;
        let approval = entry.waiting.remove(at)?;
        entry.numbered += 1;
        let short = entry.numbered.to_string();
        entry.out = Some(Handed { short: short.clone(), approval, decisions, at: now });
        Some(short)
    }

    /// The approval out to the chat, if any.
    #[must_use]
    pub fn out_to(&self, plugin: &str, chat: &PluginChat) -> Option<Handed> {
        let out = self.plugins.get(plugin)?.out.as_ref()?;
        (chat_key(&out.approval.chat) == chat_key(chat)).then(|| out.clone())
    }

    /// The chat was shown the permission modes.
    pub fn listed(&mut self, plugin: &str, chat: &PluginChat, now: Instant) {
        let entry = self.plugins.entry(plugin.to_owned()).or_default();
        entry.listed.insert(chat_key(chat), now);
    }

    #[must_use]
    pub fn listed_at(&self, plugin: &str, chat: &PluginChat) -> Option<Instant> {
        self.plugins.get(plugin)?.listed.get(&chat_key(chat)).copied()
    }

    /// The plugin's service stopped: the approval out may never have reached its chat, so it goes
    /// out again first once a service runs again.
    pub fn service_ended(&mut self, plugin: &str) {
        if let Some(entry) = self.plugins.get_mut(plugin)
            && let Some(out) = entry.out.take()
        {
            entry.waiting.push_front(out.approval);
        }
    }

    /// The plugin's service is gone for good.
    pub fn forget(&mut self, plugin: &str) {
        self.plugins.remove(plugin);
    }
}

/// Why a chat's decision or wider permission mode is not taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    /// The plugin's 远程批准 is off.
    Off,
    Group,
    NoneOut,
    /// The chat named another approval than the one out to it.
    Other {
        short: String,
    },
    TooSoon,
    /// The approval does not allow this decision from a chat.
    NotOffered {
        offered: Vec<RemoteDecision>,
    },
    /// The permission modes were not listed in the chat before.
    NotListed,
    ListedJustNow,
}

impl Refused {
    #[must_use]
    pub fn text(&self) -> String {
        match self {
            Self::Off => "这个插件没有打开远程批准；在 Aura 的 设置 → 插件 里可以打开。".to_owned(),
            Self::Group => "审批和放宽权限只能在私聊里决定。".to_owned(),
            Self::NoneOut => "现在没有等这个聊天决定的审批。".to_owned(),
            Self::Other { short } => format!("现在等你决定的是编号 {short} 的审批。"),
            Self::TooSoon => "审批交给你后 3 秒内的回复不算，请再发一次。".to_owned(),
            Self::NotOffered { offered } => {
                let commands: Vec<String> =
                    offered.iter().map(|decision| format!("/{}", decision.command())).collect();
                if offered.iter().all(|decision| *decision == RemoteDecision::Deny) {
                    format!("这条审批只能在电脑上的 Aura 里批准；可以回 {}。", commands.join("、"))
                } else {
                    format!("这条审批可以回 {}。", commands.join("、"))
                }
            }
            Self::NotListed => "先用 /permission 看一下列表，3 秒后再选。".to_owned(),
            Self::ListedJustNow => "列表发出后 3 秒内的选择不算，请再发一次。".to_owned(),
        }
    }
}

/// The approval a chat's decision is for, when it meets every rule: the plugin's 远程批准 is on,
/// the chat is a private one, an approval is out to it (the one numbered `typed`, when it names
/// one) for at least [`REMOTE_DECISION_DELAY`], and that approval allows the decision from a chat.
pub fn check_decision(
    on: bool,
    chat: &PluginChat,
    out: Option<Handed>,
    typed: &str,
    wanted: RemoteDecision,
    now: Instant,
) -> Result<Handed, Refused> {
    if !on {
        return Err(Refused::Off);
    }
    if chat.kind != PluginChatKind::Private {
        return Err(Refused::Group);
    }
    let out = out.ok_or(Refused::NoneOut)?;
    if !typed.is_empty() && typed != out.short {
        return Err(Refused::Other { short: out.short });
    }
    if now.duration_since(out.at) < REMOTE_DECISION_DELAY {
        return Err(Refused::TooSoon);
    }
    if !out.decisions.contains(&wanted) {
        return Err(Refused::NotOffered { offered: out.decisions });
    }
    Ok(out)
}

/// Whether a chat may widen the permission mode now: the plugin's 远程批准 is on, the chat is a
/// private one, and the modes were listed in it at least [`REMOTE_DECISION_DELAY`] before.
pub fn check_widening(
    on: bool,
    chat: &PluginChat,
    listed: Option<Instant>,
    now: Instant,
) -> Result<(), Refused> {
    if !on {
        return Err(Refused::Off);
    }
    if chat.kind != PluginChatKind::Private {
        return Err(Refused::Group);
    }
    let listed = listed.ok_or(Refused::NotListed)?;
    if now.duration_since(listed) < REMOTE_DECISION_DELAY {
        return Err(Refused::ListedJustNow);
    }
    Ok(())
}
