use aura_memory_domain::{MemoryDocument, MemoryMetadata, MemoryScope, parse, render};

#[test]
fn obsidian_flat_yaml_round_trip_preserves_stable_id_and_body() {
    let mut metadata = MemoryMetadata::new(MemoryScope::Project, "conventions".into());
    metadata.tags = vec!["中文".into(), "C++, Rust".into(), "a'b".into()];
    let note =
        MemoryDocument { metadata, body: "# 构建约定\n使用 [[commands/构建]]。\n".into() };
    let text = render(&note).expect("render");
    assert_eq!(parse(&text).expect("parse"), note);
    let external = text
        .replace("scope: \"project\"", "scope: project")
        .replace("category: \"conventions\"", "category: 'conventions' # comment")
        .replace(
            "tags: [\"中文\",\"C++, Rust\",\"a'b\"]",
            "tags:\n  - 中文\n  - 'C++, Rust'\n  - 'a''b'",
        );
    assert_eq!(parse(&external.replace('\n', "\r\n")).expect("Obsidian edit"), note);
}

#[test]
fn malformed_or_ambiguous_metadata_is_rejected_without_echoing_content() {
    let note = MemoryDocument {
        metadata: MemoryMetadata::new(MemoryScope::Global, "rules".into()),
        body: "# title\n".into(),
    };
    let text = render(&note).expect("render");
    for invalid in [
        text.replace("schema_version: 1", "schema_version: 2"),
        text.replace("importance: 3", "importance: 7"),
        text.replace("confidence: 1.0", "confidence: -0.5"),
        text.replace("scope: \"global\"", "scope: &alias global"),
        text.replace("---\n#", "id: duplicate\n---\n#"),
        text.replace("tags: []", "tags: [{key: secret}]"),
    ] {
        assert!(parse(&invalid).is_err(), "malformed metadata accepted");
    }
}

fn plain(scope: MemoryScope) -> MemoryDocument {
    MemoryDocument {
        metadata: MemoryMetadata::new(scope, "conventions".into()),
        body: "# 约定\n".into(),
    }
}

#[test]
fn user_properties_from_obsidian_keep_the_note_valid_and_survive_an_aura_rewrite() {
    let note = plain(MemoryScope::Project);
    let text = render(&note).expect("render");
    let user = "aliases:\n  - 构建约定\n  - build rules\ncssclasses: [wide]\n作者: 所有者 # 注释\npublish: true\n";
    let edited = text.replacen("---\n#", &format!("{user}---\n#"), 1);
    let mut parsed = parse(&edited).expect("a property outside the schema is not invalid");
    assert_eq!(parsed.metadata.id, note.metadata.id);
    parsed.body.push_str("补充一句。\n");
    let rewritten = render(&parsed).expect("render");
    assert!(rewritten.contains(user), "kept verbatim after Aura's fields:\n{rewritten}");
    assert_eq!(parse(&rewritten).expect("round trip"), parsed);
    // A note Aura writes itself carries none.
    assert!(!render(&note).expect("render").contains("aliases"));
}

#[test]
fn a_user_property_that_is_not_flat_still_makes_the_note_invalid() {
    let text = render(&plain(MemoryScope::Global)).expect("render");
    for invalid in [
        "meta:\n  nested: x\n",
        "object: {a: 1}\n",
        "block: |\n  text\n",
        "&anchor: x\n",
        "aliases:\n  - a\naliases: b\n",
        "list: [{a: 1}]\n",
    ] {
        let edited = text.replacen("---\n#", &format!("{invalid}---\n#"), 1);
        assert!(parse(&edited).is_err(), "{invalid}");
    }
}
