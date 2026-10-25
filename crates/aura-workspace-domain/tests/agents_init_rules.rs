//! What `/init` refuses before Core looks at the workspace, all reported at once.
mod agents_init_support;
use agents_init_support::{CALL, has, input, refused, valid, valid_json};
use aura_workspace_domain::{InitLink, check};

#[test]
fn a_valid_submission_lists_everything_core_still_has_to_verify() {
    let plan = check(&valid(), &[]).expect("valid");
    assert_eq!(plan.files.len(), 2);
    assert_eq!(
        plan.links,
        [InitLink { source: "AGENTS.md".into(), target: "docs/agent-guides/style.md".into() }]
    );
    assert_eq!(plan.repository_evidence.len(), 1);
    assert_eq!(plan.repository_evidence[0].path, "docs/dev/testing.md");
    assert_eq!(plan.repository_evidence[0].command, "cargo test -p aura-core");
    assert_eq!(plan.executed_evidence.len(), 1);
    assert_eq!(plan.executed_evidence[0].tool_call_id.to_string(), CALL);
    assert_eq!(plan.executed_evidence[0].command, "corepack pnpm dev");
    assert_eq!(plan.directories.iter().cloned().collect::<Vec<_>>(), [".", "apps/desktop"]);
}

#[test]
fn chapters_cannot_add_level_one_or_two_headings_but_fenced_code_is_text() {
    for heading in ["# Extra", "## Extra", "Extra\n===", "Extra\n---"] {
        let mut value = valid_json();
        value["project_structure"] = format!("说明。\n\n{heading}\n\n更多。").into();
        assert!(has(&refused(value), "heading_level", "project_structure"), "{heading}");
    }
    let mut value = valid_json();
    value["project_structure"] =
        "说明。\n\n```text\n## 这是代码\n```\n\n---\n\n分隔线之后。".into();
    check(&input(value), &[]).expect("fenced headings and a thematic break are fine");
    let mut value = valid_json();
    value["commands"]["notes"] = "## Extra".into();
    assert!(has(&refused(value), "heading_level", "commands.notes"));
}

#[test]
fn guides_may_use_level_two_headings_but_not_a_second_title() {
    let mut value = valid_json();
    value["guides"][0]["body"] = "# Another title\n\nText.".into();
    assert!(has(&refused(value), "heading_level", "guides[0].body"));
}

#[test]
fn links_stay_inside_the_workspace_and_resolve_from_the_linking_file() {
    for link in [
        "[x](https://example.com)",
        "[x](C:/Users/me/x.md)",
        "[x](/etc/passwd)",
        "[x](../outside.md)",
        "<https://example.com>",
        "[x]()",
        "[ref]: https://example.com",
    ] {
        let mut value = valid_json();
        value["testing"] = format!("见下文。\n\n{link}").into();
        assert!(has(&refused(value), "link", "AGENTS.md:"), "{link}");
    }
    let mut value = valid_json();
    value["testing"] =
        "[a](#testing-guidelines) [b](<docs/a b.md>) [c](docs/c%20d.md#x) `[e](nowhere)`".into();
    value["guides"][0]["body"] = "Back to [root](../../AGENTS.md).".into();
    let links = check(&input(value), &[]).expect("relative links").links;
    let targets = links.iter().map(|l| (l.source.as_str(), l.target.as_str())).collect::<Vec<_>>();
    assert!(targets.contains(&("AGENTS.md", "docs/a b.md")));
    assert!(targets.contains(&("AGENTS.md", "docs/c d.md")));
    assert!(targets.contains(&("docs/agent-guides/style.md", "AGENTS.md")));
    assert!(!targets.iter().any(|(_, t)| *t == "nowhere"), "code spans are not links");
}

#[test]
fn local_machine_state_never_reaches_the_files() {
    for text in [
        "生成于 2026-10-16T08:00:00Z",
        "缓存在 C:\\Users\\someone\\cache",
        "共享目录 \\\\build-server\\drops",
        "脚本在 /home/someone/bin",
        "账号 owner35141 的设置",
    ] {
        let mut value = valid_json();
        value["coding_style"] = text.into();
        let violations = aura_workspace_domain::check(&input(value), &["owner35141".into()])
            .expect_err("local state must be refused");
        assert!(
            violations.iter().any(|v| matches!(v.rule, "volatile" | "local_path" | "local_state")),
            "{text}: {violations:?}"
        );
    }
    let mut value = valid_json();
    value["coding_style"] =
        "配置在 %USERPROFILE%\\.aura；发布日期写在 CHANGELOG（例如 2026-10-16）。".into();
    check(&input(value), &["owner35141".into()])
        .expect("variables and plain dates are not local state");
}

#[test]
fn a_command_states_facts_not_guesses() {
    for (field, text) in [
        ("purpose", "可能需要先构建"),
        ("prerequisites", "usually run after install"),
        ("purpose", "It might fail"),
    ] {
        let mut value = valid_json();
        value["commands"]["entries"][0][field] = text.into();
        assert!(has(&refused(value), "hedged", "commands.entries[0]"), "{text}");
    }
    let mut value = valid_json();
    value["commands"]["entries"][0]["purpose"] = "Run the nightly suite".into();
    check(&input(value), &[]).expect("a word that only contains a hedge is fine");
}

#[test]
fn commands_are_one_line_with_evidence_and_directories_inside_the_workspace() {
    let mut value = valid_json();
    value["commands"]["entries"][0]["command"] = "cargo test\ncargo clippy".into();
    value["commands"]["entries"][0]["evidence"]["path"] = ".".into();
    value["commands"]["entries"][1]["working_directory"] = "../elsewhere".into();
    let violations = refused(value);
    assert!(has(&violations, "single_line", "commands.entries[0].command"));
    assert!(has(&violations, "workspace_file", "commands.entries[0].evidence"));
    assert!(has(&violations, "workspace_relative", "commands.entries[1].working_directory"));
    // No command, and nothing in the chapter says why.
    let mut value = valid_json();
    value["commands"]["entries"] = serde_json::json!([]);
    value["commands"]["introduction"] = "".into();
    value["commands"]["notes"] = "".into();
    assert!(has(&refused(value), "required", "commands.entries"));
}

#[test]
fn guides_have_plain_unique_names_and_never_become_a_nested_agents_file() {
    for (name, rule) in [
        ("Agents", "guide_name"),
        ("agents", "guide_name"),
        ("a/b", "guide_name"),
        ("", "guide_name"),
    ] {
        let mut value = valid_json();
        value["guides"][0]["name"] = name.into();
        assert!(has(&refused(value), rule, "guides[0].name"), "{name}");
    }
    let mut value = valid_json();
    let guide = value["guides"][0].clone();
    value["guides"] = serde_json::json!([guide.clone(), guide]);
    assert!(has(&refused(value), "duplicate", "guides[1].name"));
}

#[test]
fn every_problem_is_reported_together_and_sizes_are_bounded() {
    let mut value = valid_json();
    value["testing"] = "".into();
    value["coding_style"] = "x".repeat(16 * 1024 + 1).into();
    value["commands"]["entries"][0]["purpose"] = "可能".into();
    let violations = refused(value);
    assert!(has(&violations, "required", "testing"));
    assert!(has(&violations, "too_large", "coding_style"));
    assert!(has(&violations, "hedged", "commands.entries[0].purpose"));
    assert!(violations.iter().all(|v| !v.to_string().is_empty()));
}

#[test]
fn inspection_still_renders_a_broken_submission_so_workspace_checks_can_run_too() {
    let mut value = valid_json();
    value["testing"] = "".into();
    let checked = aura_workspace_domain::inspect(&input(value), &[]);
    assert!(has(&checked.violations, "required", "testing"));
    assert_eq!(checked.plan.files.len(), 2, "the files are rendered anyway");
    assert_eq!(checked.plan.repository_evidence.len(), 1);
}

/// A workspace with no
/// command yet says so instead of listing one.
#[test]
fn a_workspace_without_commands_says_so_instead_of_listing_one() {
    let mut value = valid_json();
    value["commands"]["entries"] = serde_json::json!([]);
    value["commands"]["introduction"] = "这个工作区还没有构建、测试或开发命令。".into();
    check(&input(value.clone()), &[]).expect("an explained empty list is accepted");
    value["commands"]["introduction"] = "".into();
    value["commands"]["notes"] = "".into();
    let violations = refused(value);
    assert!(has(&violations, "required", "commands.entries"), "{violations:?}");
    assert!(
        violations.iter().any(|v| v.detail.contains("commands.introduction")),
        "the refusal says where to explain it: {violations:?}"
    );
}
