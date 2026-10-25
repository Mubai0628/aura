//! What makes a command directory usable, and the reason shown when it is not.
mod support;
use aura_agent_domain::CommandSource;
use aura_commands::{check_template, load, parse_manifest};
use support::{MANIFEST, TEMPLATE, command, manifest_named, root};

#[test]
fn a_complete_directory_is_a_prompt_command_with_its_template_and_revision() {
    let root = root();
    let loaded =
        load(&command(root.path(), "review", Some(MANIFEST), Some(TEMPLATE)), CommandSource::User);
    assert_eq!(loaded.issue, None);
    assert_eq!(loaded.title.as_deref(), Some("审查改动"));
    assert_eq!(loaded.description.as_deref(), Some("列出本次改动的风险"));
    assert_eq!(loaded.argument_hint.as_deref(), Some("范围（可选）"));
    assert_eq!(loaded.template.as_deref(), Some(TEMPLATE));
    let entry = loaded.entry(true, None);
    assert!(entry.runnable());
    assert_eq!(entry.revision.as_ref().map(String::len), Some(64));
}

#[test]
fn the_manifest_names_its_directory_and_declares_nothing_executable() {
    for (manifest, reason) in [
        (MANIFEST.replace("\"review\"", "\"other\""), "name 必须与目录名 review 相同"),
        (format!("{MANIFEST}runtime = \"node\"\n"), "首版只支持提示命令"),
        (format!("{MANIFEST}shell = \"pwsh\"\n"), "command.toml 无法解析"),
        (MANIFEST.replace("version = 1", "version = 0"), "version 必须是正整数"),
        (MANIFEST.replace("列出本次改动的风险", ""), "description 不能为空"),
        ("not = [valid".to_owned(), "command.toml 无法解析"),
    ] {
        let issue = parse_manifest(&manifest, "review").expect_err(reason);
        assert!(issue.contains(reason), "{issue}");
    }
    let untitled = MANIFEST.replace("title = \"审查改动\"\n", "");
    assert_eq!(parse_manifest(&untitled, "review").expect("title is optional").title(), "review");
}

#[test]
fn a_template_is_bounded_plain_text_linking_only_inside_its_directory() {
    check_template("看 [示例](assets/example.md) 与 [本节](#用法)。")
        .expect("own files and anchors");
    for (template, reason) in [
        ("   \n", "template.md 是空的"),
        ("用 ghp_a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8 发布", "像是包含凭据"),
        ("见 [别处](../other/template.md)", "不在命令目录内"),
        ("见 [网页](https://example.com)", "不在命令目录内"),
        ("响铃\u{7}", "控制字符"),
    ] {
        let issue = check_template(template).expect_err(reason);
        assert!(issue.contains(reason), "{issue}");
    }
    assert!(check_template(&"x".repeat(32 * 1024 + 1)).expect_err("too large").contains("超过"));
}

#[test]
fn a_broken_directory_still_has_a_revision_and_names_its_problem() {
    let root = root();
    let named = manifest_named("missing-template");
    for (name, manifest, template, reason) in [
        ("missing-template", Some(named.as_str()), None, "缺少 template.md"),
        ("missing-manifest", None, Some(TEMPLATE), "缺少 command.toml"),
        ("Bad_Name", Some(MANIFEST), Some(TEMPLATE), "目录名只能用小写字母"),
    ] {
        let loaded =
            load(&command(root.path(), name, manifest, template), CommandSource::Workspace);
        assert!(
            loaded.issue.as_deref().is_some_and(|issue| issue.contains(reason)),
            "{name}: {:?}",
            loaded.issue
        );
        assert!(
            loaded.revision.is_some(),
            "{name}: a broken command can still be removed by revision"
        );
        assert!(!loaded.entry(true, None).runnable());
    }
}

#[test]
fn a_rendered_manifest_parses_back_to_the_same_fields() {
    let text = aura_commands::render_manifest("review", Some(" 审查 "), "列出风险", Some(""), 2)
        .expect("a valid manifest");
    let manifest = parse_manifest(&text, "review").expect("parses back");
    assert_eq!(manifest.title(), "审查");
    assert_eq!(manifest.argument_hint, None, "a blank hint is left out");
    assert_eq!(manifest.version, 2);
    assert!(aura_commands::render_manifest("Bad", None, "x", None, 1).is_err());
}
