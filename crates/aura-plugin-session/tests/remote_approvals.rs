//! An approval as a chat decides it: the decisions a chat may give, what it is told, one approval
//! out to a plugin's chats at a time with the rest waiting in order, and the rules a decision or a
//! wider permission mode from a chat must meet.
use aura_agent_domain::{PluginChat, PluginChatKind};
use aura_domain_common::{ApprovalRequestId, SessionId};
use aura_plugin_session::{
    ChatApproval, REMOTE_APPROVALS_MAX, REMOTE_DECISION_DELAY, Refused, RemoteApprovals,
    RemoteDecision, check_decision, check_widening, handed_text, remote_decisions,
};
use aura_security_contract::ApprovalDecisionKind::{
    AllowForCurrentTurn, AllowOnce, AllowSession, AllowWorkspace, Deny,
};
use std::time::{Duration, Instant};

fn chat(kind: PluginChatKind, id: &str) -> PluginChat {
    PluginChat { channel: "qq-main".into(), kind, id: id.into() }
}

fn private(id: &str) -> PluginChat {
    chat(PluginChatKind::Private, id)
}

fn waiting(chat: PluginChat) -> ChatApproval {
    ChatApproval { approval: ApprovalRequestId::new(), session: SessionId::new(), chat }
}

#[test]
fn a_chat_may_allow_once_or_for_the_conversation_and_only_refuse_the_riskiest() {
    use RemoteDecision::{Approve, ApproveSession, Deny as Refuse};
    // A built-in tool's ordinary approval.
    let ordinary = remote_decisions(&[AllowOnce, AllowSession, AllowWorkspace, Deny]);
    assert_eq!(ordinary, [Approve, ApproveSession, Refuse]);
    let memory = remote_decisions(&[AllowOnce, AllowSession, Deny]);
    assert_eq!(memory, [Approve, ApproveSession, Refuse]);
    // Once only: deleting for good, writing outside the workspace, `/init`.
    assert_eq!(remote_decisions(&[AllowOnce, Deny]), [Refuse]);
    // A tool of an MCP server or a plugin is allowed for the turn, which a chat does not give.
    assert_eq!(remote_decisions(&[AllowForCurrentTurn, Deny]), [Refuse]);
    assert_eq!(
        [Approve, ApproveSession, Refuse].map(|decision| (decision.name(), decision.command())),
        [("approve", "approve"), ("approve_session", "approve-session"), ("deny", "deny")]
    );
    assert_eq!(
        [Approve, ApproveSession, Refuse].map(RemoteDecision::kind),
        [AllowOnce, AllowSession, Deny]
    );
    assert_eq!(
        [Approve, ApproveSession, Refuse].map(RemoteDecision::done),
        ["已批准（仅此一次）。", "已批准（本会话）。", "已拒绝。"]
    );
}

#[test]
fn a_chat_without_buttons_is_told_what_is_asked_and_how_to_answer() {
    use RemoteDecision::{Approve, ApproveSession, Deny as Refuse};
    let text = handed_text(
        "3",
        "写入文件",
        "写结果\nagent-result.txt",
        &[Approve, ApproveSession, Refuse],
    );
    assert_eq!(
        text,
        "Aura 请你决定（编号 3）：写入文件\n写结果\nagent-result.txt\n\
         回复 /approve 批准这一次，/approve-session 本会话都允许，/deny 拒绝。"
    );
    let text = handed_text("4", "删除文件", "", &[Refuse]);
    assert_eq!(
        text,
        "Aura 请你决定（编号 4）：删除文件\n这条只能在电脑上的 Aura 里批准。\n回复 /deny 拒绝。"
    );
}

#[test]
fn one_approval_is_out_to_a_plugins_chats_at_a_time_and_the_rest_wait_in_order() {
    let mut approvals = RemoteApprovals::default();
    let now = Instant::now();
    let (first, second) = (waiting(private("friend")), waiting(private("buddy")));
    approvals.offer("bot", first.clone());
    approvals.offer("bot", first.clone());
    approvals.offer("bot", waiting(chat(PluginChatKind::Group, "team")));
    approvals.offer("bot", second.clone());
    approvals.offer("other-bot", waiting(private("friend")));
    assert_eq!(approvals.all().len(), 3, "offered once each; a group chat is not handed one");
    let next: Vec<_> = approvals.next().into_iter().filter(|(plugin, _)| plugin == "bot").collect();
    assert_eq!(next, [("bot".to_owned(), first.clone())]);

    let decisions = vec![RemoteDecision::Approve, RemoteDecision::Deny];
    let short = approvals.handed("bot", first.approval, decisions.clone(), now);
    assert_eq!(short.as_deref(), Some("1"));
    assert!(approvals.next().iter().all(|(plugin, _)| plugin != "bot"), "one out at a time");
    let out = approvals.out_to("bot", &private("friend")).expect("out to friend");
    assert_eq!((out.short.as_str(), &out.approval, &out.decisions), ("1", &first, &decisions));
    assert_eq!(out.at, now);
    assert_eq!(approvals.out_to("bot", &private("buddy")), None);
    assert_eq!(approvals.out_to("bot", &chat(PluginChatKind::Group, "friend")), None);
    assert_eq!(approvals.out_to("other-bot", &private("friend")), None);
    assert_eq!(approvals.handed("bot", second.approval, decisions.clone(), now), None);

    // Decided on the computer, or here: the next one goes out, numbered on.
    approvals.closed(first.approval);
    assert_eq!(approvals.out_to("bot", &private("friend")), None);
    assert!(approvals.next().contains(&("bot".to_owned(), second.clone())));
    let short = approvals.handed("bot", second.approval, decisions.clone(), now);
    assert_eq!(short.as_deref(), Some("2"));

    // The service stopped: the one out goes out again first, ahead of one offered since.
    let third = waiting(private("friend"));
    approvals.offer("bot", third.clone());
    approvals.service_ended("bot");
    assert_eq!(approvals.out_to("bot", &private("buddy")), None);
    assert!(approvals.next().contains(&("bot".to_owned(), second.clone())));
    let short = approvals.handed("bot", second.approval, decisions.clone(), now);
    assert_eq!(short.as_deref(), Some("3"));
    approvals.closed(third.approval);
    assert_eq!(approvals.all().iter().filter(|(plugin, _)| plugin == "bot").count(), 1);
    approvals.forget("bot");
    assert!(approvals.all().iter().all(|(plugin, _)| plugin != "bot"));

    for _ in 0..=REMOTE_APPROVALS_MAX {
        approvals.offer("many", waiting(private("friend")));
    }
    let many = approvals.all().into_iter().filter(|(plugin, _)| plugin == "many").count();
    assert_eq!(many, REMOTE_APPROVALS_MAX, "the oldest goes first");
}

#[test]
fn a_decision_from_a_chat_meets_every_rule_or_is_not_taken() {
    use RemoteDecision::{Approve, ApproveSession, Deny as Refuse};
    let mut approvals = RemoteApprovals::default();
    let now = Instant::now();
    let asked = waiting(private("friend"));
    approvals.offer("bot", asked.clone());
    approvals.handed("bot", asked.approval, vec![Approve, Refuse], now);
    let out = || approvals.out_to("bot", &private("friend"));
    let later = now + REMOTE_DECISION_DELAY;
    let friend = private("friend");
    let group = chat(PluginChatKind::Group, "friend");
    assert_eq!(check_decision(false, &friend, out(), "", Approve, later), Err(Refused::Off));
    assert_eq!(check_decision(true, &group, out(), "", Approve, later), Err(Refused::Group));
    assert_eq!(check_decision(true, &friend, None, "", Approve, later), Err(Refused::NoneOut));
    assert_eq!(
        check_decision(true, &friend, out(), "2", Approve, later),
        Err(Refused::Other { short: "1".into() })
    );
    let soon = later - Duration::from_millis(1);
    assert_eq!(check_decision(true, &friend, out(), "", Approve, soon), Err(Refused::TooSoon));
    assert_eq!(
        check_decision(true, &friend, out(), "1", ApproveSession, later),
        Err(Refused::NotOffered { offered: vec![Approve, Refuse] })
    );
    let taken = check_decision(true, &friend, out(), "1", Approve, later).expect("taken");
    assert_eq!(taken.approval, asked);
    let refused = check_decision(true, &friend, out(), "", Refuse, later);
    assert_eq!(refused.map(|out| out.short), Ok("1".into()));

    assert_eq!(
        Refused::Off.text(),
        "这个插件没有打开远程批准；在 Aura 的 设置 → 插件 里可以打开。"
    );
    assert_eq!(Refused::Group.text(), "审批和放宽权限只能在私聊里决定。");
    assert_eq!(Refused::NoneOut.text(), "现在没有等这个聊天决定的审批。");
    assert_eq!(Refused::Other { short: "1".into() }.text(), "现在等你决定的是编号 1 的审批。");
    assert_eq!(Refused::TooSoon.text(), "审批交给你后 3 秒内的回复不算，请再发一次。");
    assert_eq!(
        Refused::NotOffered { offered: vec![Refuse] }.text(),
        "这条审批只能在电脑上的 Aura 里批准；可以回 /deny。"
    );
    assert_eq!(
        Refused::NotOffered { offered: vec![Approve, Refuse] }.text(),
        "这条审批可以回 /approve、/deny。"
    );
}

#[test]
fn a_chat_widens_the_permission_mode_only_a_while_after_it_was_shown_the_modes() {
    let mut approvals = RemoteApprovals::default();
    let now = Instant::now();
    let friend = private("friend");
    let later = now + REMOTE_DECISION_DELAY;
    assert_eq!(approvals.listed_at("bot", &friend), None);
    assert_eq!(check_widening(true, &friend, None, later), Err(Refused::NotListed));
    approvals.listed("bot", &friend, now);
    let listed = approvals.listed_at("bot", &friend);
    assert_eq!(listed, Some(now));
    assert_eq!(approvals.listed_at("bot", &private("buddy")), None);
    assert_eq!(approvals.listed_at("other-bot", &friend), None);
    assert_eq!(check_widening(false, &friend, listed, later), Err(Refused::Off));
    let group = chat(PluginChatKind::Group, "friend");
    assert_eq!(check_widening(true, &group, listed, later), Err(Refused::Group));
    let soon = later - Duration::from_millis(1);
    assert_eq!(check_widening(true, &friend, listed, soon), Err(Refused::ListedJustNow));
    assert_eq!(check_widening(true, &friend, listed, later), Ok(()));
    assert_eq!(Refused::NotListed.text(), "先用 /permission 看一下列表，3 秒后再选。");
    assert_eq!(Refused::ListedJustNow.text(), "列表发出后 3 秒内的选择不算，请再发一次。");
    approvals.forget("bot");
    assert_eq!(approvals.listed_at("bot", &friend), None, "nothing is kept for a plugin gone");
}
