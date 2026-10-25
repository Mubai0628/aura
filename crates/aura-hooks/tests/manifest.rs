use aura_hooks::{HookCommand, HookEvent, HookFailurePolicy, HookKind, parse_manifest};

const VALID: &str = r#"
schema_version = 1
name = "format-check"
version = "1.0.0"
description = "写文件前检查格式"
events = ["tool.before_call", "tool.after_call"]
[command]
powershell_script = "check.ps1"
arguments = ["--strict"]
[match]
tools = ["fs.write", "shell.*"]
"#;

#[test]
fn a_user_hook_fails_open_and_a_security_hook_fails_closed_by_default() {
    let user = parse_manifest(VALID).expect("valid manifest");
    assert_eq!(user.kind, HookKind::User);
    assert_eq!(user.failure_policy, HookFailurePolicy::FailOpen);
    assert_eq!(user.timeout_ms, 10_000);
    assert_eq!(user.events, [HookEvent::ToolBeforeCall, HookEvent::ToolAfterCall]);
    assert_eq!(
        user.command,
        HookCommand::PowershellScript {
            path: "check.ps1".into(),
            arguments: vec!["--strict".into()]
        }
    );
    let security = parse_manifest(
        &VALID.replace("version = \"1.0.0\"", "version = \"1.0.0\"\nkind = \"security\""),
    )
    .expect("security hook");
    assert_eq!(security.failure_policy, HookFailurePolicy::FailClosed);
    let explicit = parse_manifest(&VALID.replace(
        "version = \"1.0.0\"",
        "version = \"1.0.0\"\nkind = \"security\"\nfailure_policy = \"fail_open\"",
    ))
    .expect("explicit policy");
    assert_eq!(explicit.failure_policy, HookFailurePolicy::FailOpen);
}

#[test]
fn tool_patterns_match_exact_names_and_whole_families_only() {
    let hook = parse_manifest(VALID).expect("valid manifest");
    assert!(hook.matches_tool("fs.write"));
    assert!(hook.matches_tool("shell.powershell"));
    assert!(!hook.matches_tool("fs.write_all"));
    assert!(!hook.matches_tool("shellx.run"));
    assert!(!hook.matches_tool("fs.read"));
}

#[test]
fn unsupported_isolation_unavailable_events_and_bad_fields_are_refused_together() {
    let text = VALID
        .replace("version = \"1.0.0\"", "version = \"1.0.0\"\nisolation = \"strict\"\ntimeout_ms = 99")
        .replace(
            "events = [\"tool.before_call\", \"tool.after_call\"]",
            "events = [\"session.ended\", \"model.stream_batch\", \"tool.nope\", \"tool.after_call\", \"tool.after_call\"]",
        )
        .replace("powershell_script = \"check.ps1\"", "powershell_script = \"../outside.ps1\"");
    let problems = parse_manifest(&text).expect_err("refused");
    let all = problems.join("\n");
    for expected in [
        "strict",
        "timeout_ms",
        "永久删除会话",
        "声明式 Hook",
        "tool.nope 不存在",
        "重复",
        "包内的相对路径",
    ] {
        assert!(all.contains(expected), "missing {expected} in {all}");
    }
}

#[test]
fn a_command_is_exactly_one_package_program_and_unknown_fields_are_refused() {
    let both = VALID.replace(
        "powershell_script = \"check.ps1\"",
        "powershell_script = \"check.ps1\"\nexecutable = \"bin/check.exe\"",
    );
    assert!(parse_manifest(&both).expect_err("both").join("").contains("只能有一个"));
    let other = VALID.replace("powershell_script = \"check.ps1\"", "executable = \"check.cmd\"");
    assert!(parse_manifest(&other).expect_err("not exe").join("").contains(".exe"));
    let unknown = VALID.replace("[command]", "allow = true\n[command]");
    assert!(parse_manifest(&unknown).expect_err("unknown key").join("").contains("格式不对"));
    let absolute = VALID.replace("check.ps1", "C:\\\\tools\\\\check.ps1");
    assert!(parse_manifest(&absolute).expect_err("absolute").join("").contains("相对路径"));
}

#[test]
fn a_tool_match_without_tool_events_is_refused() {
    let text = VALID.replace(
        "events = [\"tool.before_call\", \"tool.after_call\"]",
        "events = [\"turn.completed\"]",
    );
    assert!(parse_manifest(&text).expect_err("pointless match").join("").contains("match.tools"));
}
