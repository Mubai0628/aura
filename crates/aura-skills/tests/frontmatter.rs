//! The YAML subset of `SKILL.md` front matter: what real Skills use is read; anything else is
//! refused with its line instead of being guessed.
use aura_skills::parse_skill_markdown;

fn skill(front: &str) -> String {
    format!("---\n{front}\n---\n\n# Instructions\n\nDo the thing.\n")
}

#[test]
fn the_fields_real_skills_use_are_read() {
    let parsed = parse_skill_markdown(&skill(
        "name: pdf-tools\n\
         description: \"Fill PDF forms: read fields, then write them.\"\n\
         license: Apache-2.0\n\
         allowed-tools: [Read, \"Bash(python:*)\"]\n\
         metadata:\n  author: someone\n  version: \"1.0\"",
    ))
    .expect("valid");
    let front = &parsed.frontmatter;
    assert_eq!(front.name, "pdf-tools");
    assert_eq!(front.description, "Fill PDF forms: read fields, then write them.");
    assert_eq!(front.license.as_deref(), Some("Apache-2.0"));
    assert_eq!(front.allowed_tools, ["Read", "Bash(python:*)"]);
    assert_eq!(front.other_keys, ["metadata"]);
    assert_eq!(parsed.body, "# Instructions\n\nDo the thing.\n");
    assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
}

#[test]
fn blocks_lists_quotes_and_comments_follow_yaml() {
    let parsed = parse_skill_markdown(
        "\u{feff}---\r\n\
         # a comment\r\n\
         name: 'it''s-fine'\r\n\
         description: >-\r\n  Folded lines\r\n  join with spaces.\r\n\r\n  New paragraph.\r\n\
         allowed-tools:\r\n- Read\r\n- Grep  # search\r\n\
         compatibility: |\r\n  line one\r\n  line two\r\n\
         ---\r\nBody\r\n",
    )
    .expect("valid");
    let front = &parsed.frontmatter;
    assert_eq!(front.name, "it's-fine");
    assert_eq!(front.description, "Folded lines join with spaces.\nNew paragraph.");
    assert_eq!(front.allowed_tools, ["Read", "Grep"]);
    assert_eq!(front.compatibility.as_deref(), Some("line one\nline two"));
    assert_eq!(parsed.warnings, ["name 建议只用小写字母、数字和单个连字符"]);
    let plain = parse_skill_markdown(&skill(
        "name: notes\ndescription: Keeps notes # not part of it\n  and continues here\nallowed-tools: Read, Write Edit",
    ))
    .expect("valid");
    assert_eq!(plain.frontmatter.description, "Keeps notes and continues here");
    assert_eq!(plain.frontmatter.allowed_tools, ["Read", "Write", "Edit"]);
}

#[test]
fn missing_or_malformed_front_matter_is_refused_with_its_reason() {
    let refused = |text: &str| parse_skill_markdown(text).expect_err("refused").join("；");
    assert!(refused("# no front matter\n").contains("必须以 ---"));
    assert!(refused("---\nname: a\n").contains("没有用 --- 结束"));
    assert!(refused(&skill("description: only")).contains("缺少 name"));
    assert!(refused(&skill("name: a")).contains("缺少 description"));
    assert!(refused(&skill("name: a\nname: b\ndescription: d")).contains("重复"));
    assert!(refused(&skill("name: a\ndescription: \"open")).contains("双引号没有结束"));
    assert!(refused(&skill("  stray: x\nname: a\ndescription: d")).contains("不在任何键下面"));
    assert!(refused(&skill("name: a\ndescription: d\n\tbad: tab")).contains("Tab"));
    assert!(refused(&skill("name: [a, b]\ndescription: d")).contains("name 必须是一段文字"));
    assert!(
        refused(&format!("---\nname: {}\ndescription: d\n---\nbody", "x".repeat(65)))
            .contains("最长")
    );
    assert!(
        refused(&format!("---\nname: a\ndescription: {}\n---\nbody", "d".repeat(1025)))
            .contains("description 最长")
    );
    assert!(refused("---\nname: a\ndescription: d\n---\n\n  \n").contains("正文是空的"));
    assert!(refused(&skill("name: a\ndescription: d\nnot a pair")).contains("key: value"));
}
