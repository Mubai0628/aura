//! The approvals of the turns the plugins' chats are owed replies of, handed to those chats and
//! decided there. Each second, once the replies were read on, each plugin whose user turned on
//! its 远程批准 hands one approval at a time to the private chat whose turn waits for it, and the
//! next once that one was decided, expired or its turn is gone. `/approve`, `/approve-session`
//! and `/deny` decide the one out to the chat under the rules of `channels::approvals`, as the
//! composer's buttons do, and who decided is kept on the call.
use super::bridge::{failed, with_ready};
use super::commands::{Asked, Done, said};
use super::driver::PluginServices;
use super::messages::{message_parts, sender_of};
use super::sessions::ServiceSessions;
use crate::channels::{
    ChatApproval, RemoteDecision, check_decision, handed_text, remote_decisions,
};
use crate::checks::command_of;
use aura_core_broker::agent_store::StoreStep;
use aura_domain_common::ApprovalRequestId;
use aura_execution_contract::{ExecutionApprovalRequest, ExecutionApprovalResolutionRequest};
use aura_persistence_core::StoreResult;
use aura_persistence_v2::agent::{approval, approver};
use aura_persistence_v2::security_approval;
use aura_persistence_v2::tool::activity;
use aura_plugin_host::{ApprovalOffer, ChatAddress, ServiceError, ToService};
use aura_plugins::PluginCapability;
use aura_security_contract::{ApprovalDecisionKind, ApprovalState};
use aura_tool_projection::AgentToolActivityRequest;
use rusqlite::Connection;
use std::time::Instant;

/// The most of the call's explanation, and of what it acts on, a chat is shown.
const PURPOSE_MAX_CHARS: usize = 200;
const TARGET_MAX_CHARS: usize = 300;

/// What an approval asks, as the conversation shows its call.
struct Asking {
    title: String,
    detail: String,
    allowed: Vec<ApprovalDecisionKind>,
}

fn clipped(text: &str, max: usize) -> String {
    let mut chars = text.chars();
    let mut clipped: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        clipped.push('…');
    }
    clipped
}

/// What the approval asks, while it waits for an answer: the call's title and target as its row
/// shows them, or the action's own name when its row cannot be read.
fn asking(db: &Connection, waiting: &ChatApproval) -> StoreResult<Option<Asking>> {
    let request =
        ExecutionApprovalRequest { session_id: waiting.session, approval_id: waiting.approval };
    let view = approval::view(db, &request)?;
    if view.approval.state != ApprovalState::Pending {
        return Ok(None);
    }
    let shown = approver::call_of(db, waiting.session, waiting.approval).ok().and_then(|call| {
        let request = AgentToolActivityRequest {
            session_id: waiting.session,
            before_call_id: None,
            limit: 1,
            call_ids: Some(vec![call]),
        };
        let page = activity::page(db, &request).ok()?;
        page.calls.into_iter().next().map(|entry| entry.display)
    });
    let content = &view.operation.content;
    let title =
        shown.as_ref().map_or_else(|| content.action_kind.clone(), |shown| shown.title.clone());
    let purpose = content.purpose.as_deref().map(str::trim).unwrap_or_default();
    let target = shown.and_then(|shown| shown.target).unwrap_or_default();
    let detail = [clipped(purpose, PURPOSE_MAX_CHARS), clipped(target.trim(), TARGET_MAX_CHARS)]
        .into_iter()
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    Ok(Some(Asking { title, detail, allowed: view.approval.allowed_decisions }))
}

/// Whether the approval still waits for an answer; one that is gone does not.
fn pending(db: &Connection, approval: ApprovalRequestId) -> bool {
    security_approval::load(db, approval).is_ok_and(|found| found.state == ApprovalState::Pending)
}

impl PluginServices {
    /// Hands each plugin's next approval to its chat, when none of the plugin's is out, its user
    /// lets its chats decide them and the chat's channel is connected.
    pub(super) async fn hand_approvals(&self) {
        let sessions = self.bridge().sessions();
        let (all, next) = match sessions.approvals.lock() {
            Ok(approvals) => (approvals.all(), approvals.next()),
            Err(_) => return,
        };
        if all.is_empty() {
            return;
        }
        let Some(agent) = sessions.reading() else { return };
        let read = agent
            .store
            .read(StoreStep::operation("plugin_session.approvals"), move |db| {
                let closed: Vec<ApprovalRequestId> = all
                    .iter()
                    .map(|(_, waiting)| waiting.approval)
                    .filter(|approval| !pending(db, *approval))
                    .collect();
                let asked: Vec<(String, ChatApproval, Asking)> = next
                    .into_iter()
                    .filter(|(_, waiting)| !closed.contains(&waiting.approval))
                    .filter_map(|(plugin, waiting)| {
                        let asking = asking(db, &waiting).ok().flatten()?;
                        Some((plugin, waiting, asking))
                    })
                    .collect();
                Ok((closed, asked))
            })
            .await;
        let Ok((closed, asked)) = read else { return };
        for approval in closed {
            sessions.closed(approval);
        }
        for (plugin, waiting, asking) in asked {
            let on =
                with_ready(&self.plugins, &plugin, PluginCapability::SessionControl, |ready| {
                    Ok(ready.setting.remote.remote_approval)
                });
            let channel = &waiting.chat.channel;
            let connected =
                self.bridge().channels(&plugin).iter().any(|shown| shown.id == *channel);
            if !on.unwrap_or(false) || !connected {
                continue;
            }
            let decisions = remote_decisions(&asking.allowed);
            let handed = sessions.approvals.lock().ok().and_then(|mut approvals| {
                approvals.handed(&plugin, waiting.approval, decisions.clone(), Instant::now())
            });
            let Some(short) = handed else { continue };
            let text = handed_text(&short, &asking.title, &asking.detail, &decisions);
            let offer = ApprovalOffer {
                id: short,
                title: asking.title,
                detail: asking.detail,
                decisions: decisions.iter().map(|decision| decision.name().to_owned()).collect(),
            };
            let chat = &waiting.chat;
            let message = ToService::Approval {
                channel: chat.channel.clone(),
                chat: ChatAddress { kind: chat.kind, id: chat.id.clone() },
                approval: offer,
                text,
            };
            // A service that could not take it is ended; it goes out again first to the one
            // started after it.
            if self.host().deliver(&plugin, &message).is_err()
                && let Ok(mut approvals) = sessions.approvals.lock()
            {
                approvals.service_ended(&plugin);
            }
        }
    }
}

impl ServiceSessions {
    /// The approval waits for nobody now.
    pub(super) fn closed(&self, approval: ApprovalRequestId) {
        if let Ok(mut approvals) = self.approvals.lock() {
            approvals.closed(approval);
        }
    }

    /// `/approve`, `/approve-session` and `/deny`: the approval out to the chat is decided as with
    /// the composer's buttons, and who decided is kept on its call.
    pub(super) async fn decide(
        &self,
        asked: &Asked<'_>,
        wanted: RemoteDecision,
    ) -> Result<Done, ServiceError> {
        let (plugin, message) = (asked.granted.plugin.as_str(), asked.message);
        let out = self
            .approvals
            .lock()
            .ok()
            .and_then(|approvals| approvals.out_to(plugin, &message.chat));
        let on = asked.granted.remote.remote_approval;
        let now = Instant::now();
        let out = match check_decision(on, &message.chat, out, asked.arguments, wanted, now) {
            Ok(out) => out,
            Err(refused) => return Ok(said(refused.text(), asked.current)),
        };
        let (session_id, approval_id) = (out.approval.session, out.approval.approval);
        let agent = self.agent(true)?;
        let request = ExecutionApprovalRequest { session_id, approval_id };
        let view = agent.approval(request).await.map_err(|error| failed(&error))?;
        if view.approval.state != ApprovalState::Pending {
            self.closed(approval_id);
            return Ok(said("这条审批已经处理过了。", asked.current));
        }
        let request = ExecutionApprovalResolutionRequest {
            session_id,
            approval_id,
            expected_revision: view.approval.revision,
            decision: wanted.kind(),
        };
        // The same message from the platform again is the same decision.
        let command = command_of("aura plugin chat approval v1", &message_parts(plugin, message));
        let decider = Some(sender_of(plugin, message));
        match agent.resolve_approval_by(command, request, decider).await {
            Ok(_) => {
                self.closed(approval_id);
                Ok(said(wanted.done(), asked.current))
            }
            Err(error) => Ok(said(format!("没有做成：{}", failed(&error).message), asked.current)),
        }
    }
}
