use super::super::*;
use super::support::Fixture;
use aura_hooks::HookFailureKind;
use serde_json::json;

const BEFORE: &str = "tool.before_call";
const AFTER: &str = "tool.after_call";

#[test]
fn trusted_hooks_run_in_order_and_each_sees_the_previous_transform() {
    let f = Fixture::new();
    let first = f.package("first", &[BEFORE], &["set-argument", "path", "a.txt"], "");
    let second = f.package("second", &[BEFORE], &["set-argument", "mode", "append"], "");
    f.trust(&first);
    f.trust(&second);
    let outcome = f.run(f.call(
        HookEvent::ToolBeforeCall,
        Some("fs.write"),
        json!({"arguments": {"text": "x"}}),
    ));
    assert!(outcome.transformed && outcome.block.is_none());
    assert_eq!(outcome.payload["arguments"], json!({"text":"x","path":"a.txt","mode":"append"}));
    let records = f.records();
    assert_eq!(
        records.iter().map(|r| r.hook_name.as_str()).collect::<Vec<_>>(),
        ["second", "first"]
    );
    assert!(
        records
            .iter()
            .all(|r| r.outcome == HookOutcome::Transformed && r.effect == HookEffect::Applied)
    );
    let input = f.newest_input();
    assert_eq!(input["protocol"], "aura.hook.v1");
    assert_eq!(input["event"], BEFORE);
    assert_eq!(
        input["payload"]["arguments"]["path"], "a.txt",
        "the second Hook saw the first's change"
    );
}

#[test]
fn an_untrusted_or_disabled_hook_never_runs_and_a_tool_match_limits_tool_events() {
    let f = Fixture::new();
    let untrusted =
        f.package("untrusted", &[BEFORE], &["answer", r#"{"decision":"block","reason":"x"}"#], "");
    f.trust(&untrusted);
    f.dispatch(
        "agent.hooks.untrust",
        json!({"path": untrusted.display().to_string(), "expected_revision": f.revision()}),
    )
    .expect("untrusted");
    let disabled =
        f.package("disabled", &[BEFORE], &["answer", r#"{"decision":"block","reason":"x"}"#], "");
    f.trust(&disabled);
    f.dispatch(
        "agent.hooks.set_enabled",
        json!({"path": disabled.display().to_string(), "enabled": false, "expected_revision": f.revision()}),
    )
    .expect("disabled");
    let matched = f.package(
        "writes-only",
        &[BEFORE],
        &["answer", r#"{"decision":"block","reason":"只拦写入"}"#],
        "[match]\ntools = [\"fs.write\"]",
    );
    f.trust(&matched);
    let read = f.run(f.call(HookEvent::ToolBeforeCall, Some("fs.read"), json!({"arguments": {}})));
    assert!(read.block.is_none() && read.invocations.is_empty(), "nothing hears fs.read");
    let write =
        f.run(f.call(HookEvent::ToolBeforeCall, Some("fs.write"), json!({"arguments": {}})));
    assert_eq!(
        write.block,
        Some(HookBlock {
            hook_name: "writes-only".into(),
            reason: "只拦写入".into(),
            failed: false
        })
    );
    assert_eq!(f.records().len(), 1, "only the matching, trusted, enabled Hook ran");
}

#[test]
fn a_block_stops_later_hooks_and_only_a_pre_event_block_stops_the_action() {
    let f = Fixture::new();
    let blocker = f.package(
        "blocker",
        &[BEFORE, AFTER],
        &["answer", r#"{"decision":"block","reason":"不允许"}"#],
        "",
    );
    let later = f.package("later", &[BEFORE, AFTER], &["continue"], "");
    f.trust(&blocker);
    f.trust(&later);
    let before =
        f.run(f.call(HookEvent::ToolBeforeCall, Some("fs.write"), json!({"arguments": {}})));
    assert_eq!(before.invocations.len(), 1, "the later Hook did not run");
    assert_eq!(before.invocations[0].effect, HookEffect::ActionBlocked);
    let after = f.run(f.call(HookEvent::ToolAfterCall, Some("fs.write"), json!({"result": "ok"})));
    assert_eq!(after.invocations.len(), 1);
    assert_eq!(after.invocations[0].effect, HookEffect::LaterHooksStopped);
    assert_eq!(after.payload["result"], "ok", "a post-event block changes nothing that happened");
}

#[test]
fn failures_follow_the_policy_and_are_recorded_with_their_kind() {
    let f = Fixture::new();
    for (name, arguments, extra) in [
        ("slow", vec!["sleep", "5000"], "timeout_ms = 300"),
        ("crash", vec!["exit", "3"], ""),
        ("garbage", vec!["answer", "not json"], ""),
        ("out-of-bounds", vec!["answer", r#"{"decision":"transform","text":"x"}"#], ""),
        ("approval", vec!["answer", r#"{"decision":"continue","allow":true}"#], ""),
    ] {
        f.trust(&f.package(name, &[BEFORE], &arguments, extra));
    }
    let open = f.run(f.call(HookEvent::ToolBeforeCall, Some("fs.write"), json!({"arguments": {}})));
    assert!(open.block.is_none(), "user Hooks fail open");
    let mut kinds =
        f.records().iter().map(|r| (r.hook_name.clone(), r.failure)).collect::<Vec<_>>();
    kinds.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        kinds,
        [
            ("approval".to_owned(), Some(HookFailureKind::InvalidResult)),
            ("crash".to_owned(), Some(HookFailureKind::ExitCode)),
            ("garbage".to_owned(), Some(HookFailureKind::InvalidResult)),
            ("out-of-bounds".to_owned(), Some(HookFailureKind::InvalidResult)),
            ("slow".to_owned(), Some(HookFailureKind::TimedOut)),
        ]
    );
    let slow = f.records().into_iter().find(|r| r.hook_name == "slow").expect("slow");
    assert!(slow.duration_ms < 4000, "the timeout stopped the process: {}ms", slow.duration_ms);
}

#[test]
fn a_security_hook_that_fails_or_changed_since_trust_blocks_the_action() {
    let f = Fixture::new();
    let guard = f.package("guard", &[BEFORE], &["continue"], "kind = \"security\"");
    f.trust(&guard);
    assert!(
        f.run(f.call(HookEvent::ToolBeforeCall, Some("fs.write"), json!({"arguments": {}})))
            .block
            .is_none()
    );
    std::fs::write(guard.join("notes.txt"), "a new file is new content").expect("edit package");
    let changed =
        f.run(f.call(HookEvent::ToolBeforeCall, Some("fs.write"), json!({"arguments": {}})));
    let block = changed.block.expect("fail closed");
    assert!(
        block.failed && block.reason.contains("guard") && block.reason.contains("重新信任"),
        "{block:?}"
    );
    assert_eq!(changed.invocations[0].failure, Some(HookFailureKind::ContentChanged));
    let user = f.package("user-hook", &[AFTER], &["continue"], "");
    f.trust(&user);
    std::fs::write(user.join("notes.txt"), "changed").expect("edit package");
    let skipped =
        f.run(f.call(HookEvent::ToolAfterCall, Some("fs.write"), json!({"result": "ok"})));
    assert!(skipped.block.is_none(), "a changed user Hook is skipped, not obeyed");
    assert_eq!(skipped.invocations[0].failure, Some(HookFailureKind::ContentChanged));
}

#[test]
fn stderr_is_kept_for_the_user_and_a_hook_may_ignore_its_input() {
    let f = Fixture::new();
    f.trust(&f.package("talks", &["turn.completed"], &["stderr", "debug line"], ""));
    f.trust(&f.package("deaf", &["turn.completed"], &["ignore-input"], ""));
    let outcome =
        f.run(f.call(HookEvent::TurnCompleted, None, json!({"text": "x".repeat(300_000)})));
    assert!(outcome.block.is_none());
    let records = f.records();
    let talks = records.iter().find(|r| r.hook_name == "talks").expect("talks");
    assert_eq!(talks.stderr.as_deref(), Some("debug line"));
    let deaf = records.iter().find(|r| r.hook_name == "deaf").expect("deaf");
    assert_eq!(deaf.outcome, HookOutcome::Continued, "not reading stdin is not a failure");
}
