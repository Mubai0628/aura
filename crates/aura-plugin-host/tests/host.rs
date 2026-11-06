//! The host keeps a service running only while it is wanted, starts it again after it ended with
//! a growing wait, and answers every line the service writes without letting it hold Aura up.
mod support;
use aura_plugins::PluginServiceStatus;
use serde_json::json;
use std::sync::atomic::Ordering;
use std::time::Duration;
use support::*;

fn restarting(rig: &Rig) -> Option<(u32, String)> {
    match rig.calls.status() {
        Some(PluginServiceStatus::Restarting { seconds, reason }) => Some((seconds, reason)),
        _ => None,
    }
}

#[test]
fn a_service_starts_with_hello_and_runs_once_it_is_ready() {
    let rig = rig();
    rig.host.reconcile(Vec::new());
    assert_eq!(rig.launcher.count(), 0, "nothing starts until it is wanted");
    rig.host.reconcile(vec![launch("a")]);
    let service = rig.launcher.last();
    let hello = &service.lines()[0];
    assert_eq!(hello["type"], "hello");
    assert_eq!(hello["protocol"], 1);
    assert_eq!(hello["plugin"], "chat-bot");
    assert_eq!(
        hello["channels"],
        json!([{"id": "main", "platform": "other", "title": "测试渠道"}])
    );
    assert_eq!(hello["settings"], json!({"app_id": "1024"}));
    assert_eq!(rig.calls.status(), Some(PluginServiceStatus::Starting));
    service.ready();
    eventually("running", || {
        matches!(rig.calls.status(), Some(PluginServiceStatus::Running { .. }))
    });
    let ready = rig.calls.ready.lock().expect("ready").clone();
    assert_eq!(ready.len(), 1);
    assert_eq!(ready[0].1[0].id, "main");
    assert_eq!(ready[0].1[0].max_text_chars, 2000);
    rig.host.reconcile(vec![launch("a")]);
    assert_eq!(rig.launcher.count(), 1, "the same service wanted again keeps running");
}

#[test]
fn a_service_no_longer_wanted_is_asked_to_end_then_ended() {
    let rig = rig();
    let service = rig.running();
    rig.host.reconcile(Vec::new());
    assert!(service.lines().iter().any(|line| line["type"] == "shutdown"));
    assert!(!service.stopped.load(Ordering::Acquire), "it has a few seconds to end by itself");
    rig.clock.advance(Duration::from_secs(5));
    rig.host.tick();
    assert!(service.stopped.load(Ordering::Acquire));
    eventually("the end seen", || rig.calls.ended() == 1);
    eventually("nothing left", || rig.host.idle());
}

#[test]
fn a_service_that_ends_starts_again_after_one_two_and_four_seconds() {
    let rig = rig();
    rig.running().exit(1);
    eventually("waiting to start again", || restarting(&rig).is_some());
    let (seconds, reason) = restarting(&rig).expect("restarting");
    assert_eq!(seconds, 1);
    assert!(reason.contains("退出码 1"), "{reason}");
    rig.clock.advance(Duration::from_millis(500));
    rig.host.tick();
    assert_eq!(rig.launcher.count(), 1, "not before its wait is over");
    for (wait, count) in [(1, 2), (2, 3), (4, 4)] {
        rig.clock.advance(Duration::from_secs(wait));
        rig.host.tick();
        assert_eq!(rig.launcher.count(), count);
        rig.launcher.last().exit(3);
        let next = wait * 2;
        eventually("the next wait", || {
            restarting(&rig).is_some_and(|(seconds, _)| u64::from(seconds) == next)
        });
    }
    rig.clock.advance(Duration::from_secs(8));
    rig.host.tick();
    let service = rig.launcher.last();
    service.ready();
    eventually("running", || {
        matches!(rig.calls.status(), Some(PluginServiceStatus::Running { .. }))
    });
    rig.clock.advance(Duration::from_secs(5 * 60));
    service.exit(1);
    eventually("the count started over", || {
        restarting(&rig).is_some_and(|(seconds, _)| seconds == 1)
    });
}

#[test]
fn the_wait_never_grows_past_a_minute() {
    let rig = rig();
    rig.running().exit(1);
    let mut last = 0;
    for _ in 0..9 {
        eventually("waiting", || restarting(&rig).is_some());
        let (seconds, _) = restarting(&rig).expect("restarting");
        last = seconds;
        rig.clock.advance(Duration::from_secs(u64::from(seconds)));
        rig.host.tick();
        rig.calls.statuses.lock().expect("statuses").clear();
        rig.launcher.last().exit(1);
    }
    assert_eq!(last, 60);
}

#[test]
fn a_service_that_never_says_ready_is_ended_and_started_again() {
    let rig = rig();
    rig.host.reconcile(vec![launch("a")]);
    let service = rig.launcher.last();
    rig.clock.advance(Duration::from_secs(29));
    rig.host.tick();
    assert!(!service.stopped.load(Ordering::Acquire));
    rig.clock.advance(Duration::from_secs(1));
    rig.host.tick();
    assert!(service.stopped.load(Ordering::Acquire));
    eventually("waiting to start again", || restarting(&rig).is_some());
    assert!(restarting(&rig).expect("restarting").1.contains("30 秒内没有发 ready"));
}

#[test]
fn a_protocol_version_aura_does_not_know_stops_the_service_for_good() {
    let rig = rig();
    rig.host.reconcile(vec![launch("a")]);
    let service = rig.launcher.last();
    service.say_json(json!({"type": "ready", "protocol": 2}));
    let error = service.error("protocol.version");
    assert!(error["message"].as_str().is_some_and(|text| text.contains('2')), "{error}");
    eventually("stopped", || {
        matches!(rig.calls.status(), Some(PluginServiceStatus::Stopped { .. }))
    });
    assert!(service.stopped.load(Ordering::Acquire));
    rig.clock.advance(Duration::from_secs(120));
    rig.host.tick();
    assert_eq!(rig.launcher.count(), 1, "the same service is not started again");
    rig.host.reconcile(vec![launch("b")]);
    assert_eq!(rig.launcher.count(), 2, "a changed plugin starts again");
}

#[test]
fn bad_lines_are_answered_with_their_reason_and_the_next_line_is_read() {
    let rig = rig();
    let service = rig.running();
    service.say("this is not json");
    assert!(
        service.error("protocol.not_json")["message"]
            .as_str()
            .is_some_and(|m| m.contains("第 2 行"))
    );
    service.say_json(json!({"type": "bogus"}));
    service.error("protocol.unknown_type");
    service.say_json(
        json!({"type": "request", "id": 7, "method": "notify", "params": {}, "extra": 1}),
    );
    assert_eq!(service.answer(json!(7))["error"]["code"], "protocol.bad_message");
    service.say_json(json!({"type": "request", "method": "notify"}));
    service.error("protocol.bad_message");
    service.say(&"x".repeat(1024 * 1024 + 1));
    service.error("protocol.line_too_long");
    service.say_json(
        json!({"type": "request", "id": "after", "method": "notify", "params": {"text": "hi"}}),
    );
    let answer = service.answer(json!("after"));
    assert_eq!(answer["result"]["shown"], true, "{answer}");
    service.say_json(json!({"type": "request", "id": "x", "method": "remote.allow", "params": {}}));
    assert_eq!(service.answer(json!("x"))["error"]["code"], "protocol.unknown_method");
}

#[test]
fn requests_before_ready_too_many_at_once_and_repeated_ids_are_refused() {
    let rig = rig();
    rig.host.reconcile(vec![launch("a")]);
    let service = rig.launcher.last();
    service.say_json(json!({"type": "request", "id": 1, "method": "notify"}));
    assert_eq!(service.answer(json!(1))["error"]["code"], "protocol.not_ready");
    service.ready();
    eventually("running", || {
        matches!(rig.calls.status(), Some(PluginServiceStatus::Running { .. }))
    });
    for index in 0..16 {
        service.say_json(json!({"type": "request", "id": format!("h{index}"), "method": "held"}));
    }
    eventually("all held", || rig.calls.calls.lock().expect("calls").len() == 16);
    service.say_json(json!({"type": "request", "id": "h3", "method": "held"}));
    assert_eq!(service.answer(json!("h3"))["error"]["code"], "protocol.duplicate_id");
    service.say_json(json!({"type": "request", "id": "17", "method": "notify"}));
    assert_eq!(service.answer(json!("17"))["error"]["code"], "protocol.busy");
    rig.calls.held.add_permits(16);
    eventually("every held request answered", || {
        let released = service
            .lines()
            .into_iter()
            .filter(|line| line["type"] == "response" && line["result"]["released"] == true);
        released.count() == 16
    });
    service.say_json(json!({"type": "request", "id": "18", "method": "notify"}));
    assert_eq!(service.answer(json!("18"))["result"]["shown"], true);
}

#[test]
fn a_slow_request_does_not_hold_up_the_next() {
    let rig = rig();
    let service = rig.running();
    service.say_json(json!({"type": "request", "id": "slow", "method": "held"}));
    service.say_json(json!({"type": "request", "id": "quick", "method": "notify"}));
    assert_eq!(service.answer(json!("quick"))["result"]["shown"], true);
    assert!(!service.lines().iter().any(|line| line["id"] == "slow"));
    rig.calls.held.add_permits(1);
    service.answer(json!("slow"));
}

#[test]
fn a_service_that_stops_reading_is_ended_and_started_again() {
    let rig = rig();
    let service = rig.running();
    service.full.store(true, Ordering::Release);
    service.say_json(json!({"type": "request", "id": 1, "method": "notify"}));
    eventually("stopped", || service.stopped.load(Ordering::Acquire));
    eventually("waiting to start again", || restarting(&rig).is_some());
    assert!(restarting(&rig).expect("restarting").1.contains("不再读"));
}

#[test]
fn a_changed_service_is_asked_to_end_and_started_as_it_is_now() {
    let rig = rig();
    let first = rig.running();
    rig.host.reconcile(vec![launch("b")]);
    assert!(first.lines().iter().any(|line| line["type"] == "shutdown"));
    assert_eq!(rig.launcher.count(), 1, "never two at once");
    rig.clock.advance(Duration::from_secs(5));
    rig.host.tick();
    eventually("the first one gone", || rig.calls.ended() == 1);
    rig.host.tick();
    assert_eq!(rig.launcher.count(), 2);
    assert_eq!(rig.launcher.last().identity, "b");
}

#[test]
fn a_start_that_fails_waits_and_tries_again() {
    let rig = rig();
    *rig.launcher.fail.lock().expect("fail") = Some("在 PATH 里找不到 python".into());
    rig.host.reconcile(vec![launch("a")]);
    let (seconds, reason) = restarting(&rig).expect("waiting");
    assert_eq!(seconds, 1);
    assert!(reason.contains("没能启动：在 PATH 里找不到 python"), "{reason}");
    *rig.launcher.fail.lock().expect("fail") = None;
    rig.clock.advance(Duration::from_secs(1));
    rig.host.tick();
    assert_eq!(rig.launcher.count(), 1);
}

#[test]
fn undeclared_channels_are_refused_and_the_rest_kept() {
    let rig = rig();
    rig.host.reconcile(vec![launch("a")]);
    let service = rig.launcher.last();
    service.say_json(json!({"type": "ready", "protocol": 1, "channels": [
        {"id": "main", "max_text_chars": 500, "markdown": true},
        {"id": "elsewhere", "max_text_chars": 500},
        {"id": "main", "max_text_chars": 500}
    ]}));
    eventually("running", || {
        matches!(rig.calls.status(), Some(PluginServiceStatus::Running { .. }))
    });
    let messages: Vec<String> = service
        .lines()
        .iter()
        .filter(|line| line["type"] == "error")
        .map(|line| line["message"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert!(messages.iter().any(|m| m.contains("elsewhere")), "{messages:?}");
    assert!(messages.iter().any(|m| m.contains("两次")), "{messages:?}");
    let ready = rig.calls.ready.lock().expect("ready").clone();
    assert_eq!(ready[0].1.len(), 1);
    assert!(ready[0].1[0].markdown);
}

#[test]
fn what_the_service_logs_is_kept_without_its_secrets_and_within_the_limit() {
    let rig = rig();
    let service = rig.running();
    service.stderr(&format!("token {SECRET} leaked\npart"));
    service.stderr(" of a line\n");
    service.say_json(json!({"type": "log", "level": "warning", "text": format!("{SECRET} again")}));
    eventually("logged", || rig.calls.logs().len() >= 3);
    let logs = rig.calls.logs();
    assert!(logs.contains(&("stderr".into(), "token •••• leaked".into())), "{logs:?}");
    assert!(logs.contains(&("stderr".into(), "part of a line".into())), "{logs:?}");
    assert!(logs.contains(&("warning".into(), "•••• again".into())), "{logs:?}");
    assert!(!service.raw().contains(SECRET), "nothing Aura writes carries a secret value");
    for index in 0..150 {
        service.stderr(&format!("line {index}\n"));
    }
    eventually("the limit reached", || rig.calls.logs().iter().any(|(_, text)| text == "line 116"));
    std::thread::sleep(Duration::from_millis(100));
    assert!(!rig.calls.logs().iter().any(|(_, text)| text == "line 117"), "120 lines a minute");
    rig.clock.advance(Duration::from_secs(60));
    service.stderr("after a minute\n");
    eventually("the next minute", || {
        rig.calls.logs().iter().any(|(_, text)| text == "after a minute")
    });
    assert!(rig.calls.logs().iter().any(|(_, text)| text.contains("33 行日志超过上限")));
}
