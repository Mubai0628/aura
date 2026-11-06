//! The one desktop host: started on the first request and reused; one request at a time; started
//! again after it ended; ended when it does not answer in time; never used when it does not say it
//! is ready in this protocol. What was not sent says so; what may have reached the program says
//! that it is not known.
mod support;

use aura_desktop_domain::*;
use aura_desktop_service::{HostLink, LinkError};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use support::{Answer, Launcher, Stand};

const SHORT: Duration = Duration::from_millis(300);

fn link(stand: &Arc<Stand>) -> HostLink {
    HostLink::with_times(Arc::new(Launcher(Arc::clone(stand))), SHORT, Duration::from_millis(100))
}

fn displays() -> HostOutcome {
    HostOutcome::Done(HostResult::Displays { monitors: vec![] })
}

/// Waits up to a second for `done`.
async fn until(done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(1);
    while !done() {
        assert!(Instant::now() < deadline, "not in time");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// An action kept in hold `key`.
fn held(key: &str) -> HostCommand {
    HostCommand::Guarded {
        command: Box::new(HostCommand::Displays),
        hold: Some(key.into()),
        stop_keys: None,
    }
}

fn failure(code: HostErrorCode) -> HostOutcome {
    HostOutcome::Failed(HostError::new(code, "没有执行", Delivery::NotDelivered))
}

#[tokio::test]
async fn a_running_hosts_completed_steps_are_visible_before_its_reply() {
    let stand = Stand::new(|_| Answer::Progress {
        app_id: "fixture".into(),
        steps_done: 2,
        outcome: displays(),
    });
    let link = Arc::new(link(&stand));
    let call = aura_domain_common::ToolCallId::new();
    let caller = tokio::spawn({
        let link = Arc::clone(&link);
        async move { link.request_for_call(HostCommand::Displays, SHORT, Some(call)).await }
    });
    until(|| link.steps_done("fixture") == 2).await;
    assert_eq!(link.steps_done_for(call, "fixture"), 2);
    assert_eq!(link.steps_done_for(aura_domain_common::ToolCallId::new(), "fixture"), 0);
    assert_eq!(caller.await.expect("caller"), Ok(displays()));
}

#[tokio::test]
async fn a_hold_is_known_from_its_actions_reply_until_the_host_says_it_gave_it_back() {
    let stand = Stand::new(|_| Answer::Now(displays()));
    let link = link(&stand);
    assert_eq!(link.held(), None);
    link.request(HostCommand::Displays, SHORT).await.expect("a read");
    assert_eq!(link.held(), None, "a read keeps nothing");
    link.request(held("t1"), SHORT).await.expect("an action");
    assert_eq!(link.held().as_deref(), Some("t1"));
    stand.say(&HostMessage::HoldEnded {
        hold: "t1".into(),
        reason: HoldEnd::TakenOver,
        cause: Some("按了 Esc".into()),
    });
    until(|| link.held().is_none()).await;
    assert_eq!(link.hold_end("t1"), Some(HoldEnd::TakenOver));
    assert_eq!(link.takeover_cause("t1").as_deref(), Some("按了 Esc"));
    assert_eq!(link.hold_end("t2"), None);
    assert_eq!(link.request(HostCommand::Displays, SHORT).await, Ok(displays()), "no reply lost");
}

#[tokio::test]
async fn a_hold_the_host_gave_back_before_answering_is_known_with_the_answer() {
    let stand = Stand::new(|_| {
        Answer::Saying(
            vec![HostMessage::HoldEnded { hold: "t1".into(), reason: HoldEnd::Idle, cause: None }],
            displays(),
        )
    });
    let link = link(&stand);
    link.request(held("t2"), SHORT).await.expect("an action");
    assert_eq!(link.hold_end("t1"), Some(HoldEnd::Idle));
    assert_eq!(link.held().as_deref(), Some("t2"));
}

#[tokio::test]
async fn a_hold_taken_over_after_its_commands_last_step_stays_ended_with_the_answer() {
    let saying = |reason| {
        Stand::new(move |_| {
            let cause = (reason == HoldEnd::TakenOver).then(|| "滚了滚轮".to_owned());
            Answer::Saying(
                vec![HostMessage::HoldEnded { hold: "t1".into(), reason, cause }],
                displays(),
            )
        })
    };
    let taken = saying(HoldEnd::TakenOver);
    let after = link(&taken);
    after.request(held("t1"), SHORT).await.expect("an action");
    assert_eq!(after.held(), None, "the user took it over before the host answered");
    assert_eq!(after.hold_end("t1"), Some(HoldEnd::TakenOver));
    assert_eq!(after.takeover_cause("t1").as_deref(), Some("滚了滚轮"));
    let idle = saying(HoldEnd::Idle);
    let again = link(&idle);
    again.request(held("t1"), SHORT).await.expect("an action");
    assert_eq!(again.held().as_deref(), Some("t1"), "the action took the desktop again");
    assert_eq!(again.hold_end("t1"), None, "the end was of the hold before");
}

#[tokio::test]
async fn a_hold_ends_with_its_host() {
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let stand = Stand::new(move |_| {
        if calls.fetch_add(1, Ordering::SeqCst) == 1 {
            Answer::Crash
        } else {
            Answer::Now(displays())
        }
    });
    let link = link(&stand);
    link.request(held("t1"), SHORT).await.expect("an action");
    assert_eq!(link.request(held("t1"), SHORT).await, Err(LinkError::Ended));
    assert_eq!(link.held(), None, "the host that kept it ended");
    link.request(held("t1"), SHORT).await.expect("a new host");
    assert_eq!(link.held().as_deref(), Some("t1"));
    link.stop().await;
    assert_eq!(link.held(), None);
}

#[tokio::test]
async fn an_action_refused_as_busy_or_as_taken_over_changes_only_its_own_hold() {
    let t1 = std::sync::atomic::AtomicUsize::new(0);
    let stand = Stand::new(move |command| match command {
        HostCommand::Guarded { hold: Some(key), .. } if key == "t1" => {
            if t1.fetch_add(1, Ordering::SeqCst) == 0 {
                Answer::Now(displays())
            } else {
                Answer::Now(failure(HostErrorCode::TakenOver))
            }
        }
        HostCommand::Guarded { hold: Some(key), .. } if key == "t2" => {
            Answer::Now(failure(HostErrorCode::Busy))
        }
        _ => Answer::Now(failure(HostErrorCode::TakenOver)),
    });
    let link = link(&stand);
    link.request(held("t1"), SHORT).await.expect("an action");
    link.request(held("t2"), SHORT).await.expect("a reply");
    assert_eq!(link.held().as_deref(), Some("t1"), "the host still keeps t1");
    link.request(held("t3"), SHORT).await.expect("a reply");
    assert_eq!(
        (link.held().as_deref(), link.hold_end("t3")),
        (Some("t1"), Some(HoldEnd::TakenOver))
    );
    link.request(held("t1"), SHORT).await.expect("a reply");
    assert_eq!((link.held(), link.hold_end("t1")), (None, Some(HoldEnd::TakenOver)));
}

#[tokio::test]
async fn release_gives_the_hold_back() {
    let stand = Stand::new(|command| match command {
        HostCommand::Release { .. } => Answer::Now(HostOutcome::Done(HostResult::Released)),
        _ => Answer::Now(displays()),
    });
    let link = link(&stand);
    link.request(held("t1"), SHORT).await.expect("an action");
    link.release("t1").await;
    assert_eq!(stand.sent().last(), Some(&HostCommand::Release { hold: "t1".into() }));
    assert_eq!(link.held(), None);
    let sent = stand.sent().len();
    link.release("t1").await;
    assert_eq!(stand.sent().len(), sent, "nothing kept, nothing to give back");
}

#[tokio::test]
async fn canceling_a_pending_call_ends_its_host_before_reusing_the_link() {
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let stand = Stand::new(move |_| {
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Answer::Silent
        } else {
            Answer::Now(displays())
        }
    });
    let link = link(&stand);
    assert!(
        tokio::time::timeout(
            Duration::from_millis(50),
            link.request(HostCommand::Displays, Duration::from_secs(30))
        )
        .await
        .is_err()
    );
    assert_eq!(stand.stops(), 1, "the canceled command cannot carry on in the host");
    assert!(!link.running().await);
    assert_eq!(link.request(HostCommand::Displays, SHORT).await, Ok(displays()));
    assert_eq!(stand.starts(), 2);
}

#[tokio::test]
async fn the_host_starts_on_the_first_request_and_answers_each_in_turn() {
    let stand = Stand::new(|_| Answer::Now(displays()));
    let link = link(&stand);
    assert!(!link.running().await, "nothing runs before it is needed");
    for _ in 0..3 {
        let outcome = link.request(HostCommand::Displays, SHORT).await;
        assert_eq!(outcome, Ok(displays()));
    }
    assert_eq!((stand.starts(), stand.stops()), (1, 0));
    assert_eq!(stand.sent().len(), 3);
    link.stop().await;
    assert_eq!(stand.stops(), 1);
    assert!(!link.running().await);
}

#[tokio::test]
async fn a_host_that_ended_is_started_again_for_the_next_request() {
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let stand = Stand::new(move |_| {
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Answer::Crash
        } else {
            Answer::Now(displays())
        }
    });
    let link = link(&stand);
    let ended = link.request(HostCommand::Displays, SHORT).await;
    assert_eq!(ended, Err(LinkError::Ended));
    let error = LinkError::Ended.into_host_error();
    assert_eq!(error.delivery, Delivery::Uncertain, "it may have reached the program");
    assert_eq!(link.request(HostCommand::Displays, SHORT).await, Ok(displays()));
    assert_eq!(stand.starts(), 2);
}

#[tokio::test]
async fn a_host_that_does_not_answer_in_time_is_ended_and_the_next_request_starts_another() {
    let calls = std::sync::atomic::AtomicUsize::new(0);
    let stand = Stand::new(move |_| {
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Answer::Silent
        } else {
            Answer::Now(displays())
        }
    });
    let link = link(&stand);
    let started = Instant::now();
    let silent = link.request(HostCommand::Displays, SHORT).await;
    let waited = started.elapsed();
    assert_eq!(silent, Err(LinkError::TimedOut));
    assert!(waited >= SHORT && waited < SHORT * 3, "{waited:?}");
    assert_eq!(stand.stops(), 1, "the host that did not answer was ended");
    let error = LinkError::TimedOut.into_host_error();
    assert_eq!((error.code, error.delivery), (HostErrorCode::Timeout, Delivery::Uncertain));
    assert_eq!(link.request(HostCommand::Displays, SHORT).await, Ok(displays()));
    assert_eq!(stand.starts(), 2);
}

#[tokio::test]
async fn a_host_that_is_not_ready_in_this_protocol_is_never_sent_anything() {
    for stand in [
        Stand::never_ready(|_| Answer::Now(displays())),
        Stand::of_protocol(DESKTOP_PROTOCOL_VERSION + 1),
    ] {
        let link = link(&stand);
        let refused = link.request(HostCommand::Displays, SHORT).await;
        assert!(matches!(refused, Err(LinkError::NotStarted(_))), "{refused:?}");
        assert!(stand.sent().is_empty(), "nothing was sent");
        assert_eq!(stand.stops(), 1, "the host was ended");
        let error = refused.expect_err("refused").into_host_error();
        assert_eq!(error.delivery, Delivery::NotDelivered);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn requests_reach_the_host_one_at_a_time() {
    let stand = Stand::new(|_| Answer::After(Duration::from_millis(150), displays()));
    let link = Arc::new(link(&stand));
    let calls: Vec<_> = (0..3)
        .map(|_| {
            let link = Arc::clone(&link);
            tokio::spawn(async move { link.request(HostCommand::Displays, SHORT).await })
        })
        .collect();
    for call in calls {
        assert_eq!(call.await.expect("joined"), Ok(displays()));
    }
    assert_eq!(stand.most_in_flight.load(Ordering::SeqCst), 1);
    assert_eq!(stand.starts(), 1);
}
