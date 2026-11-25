use crate::support;
use support::{Fixture, actor};

#[test]
fn history_rebuild_preserves_aura_authorship_as_flat_markdown() {
    let fixture = Fixture::project();
    let write = fixture.write("rules/attributed.md", "# Authored note\n");
    let actor = aura_memory::WriteActor {
        session_id: Some(aura_domain_common::SessionId::new()),
        session_name: Some("Same name".into()),
        ..actor()
    };
    let hashes = fixture.vault.write(std::slice::from_ref(&write), &actor).expect("write");
    let text = std::fs::read_to_string(fixture.root.join(write.path.as_str())).expect("Markdown");
    assert!(text.contains("aura_creation_stamp:"), "the creator is missing from Markdown");
    let renamed =
        aura_memory::WriteActor { session_name: Some("After rename".into()), ..actor.clone() };
    let id = write.note.metadata.id;
    let edit = aura_memory::MemoryWrite { expected_hash: Some(hashes[0].clone()), ..write };
    fixture.vault.write(&[edit], &renamed).expect("update");
    let Fixture { temp, root, vault } = fixture;
    let scope = vault.scope();
    drop(vault);
    let cache = temp.path().join("cache/memory.db");
    std::fs::remove_file(&cache).expect("derived cache only");
    let reopened = aura_memory::Vault::open(root, scope, &cache).expect("rebuild");
    let page_query = aura_memory::MemoryTimelineQuery {
        author_session_id: actor.session_id,
        category: Some("conventions".into()),
        limit: Some(1),
        ..Default::default()
    };
    let page = reopened.timeline(&page_query).expect("isolated fixture state");
    assert_eq!(page.entries[0].session_name_at_write.as_deref(), Some("After rename"));
    let older = reopened
        .timeline(&aura_memory::MemoryTimelineQuery { before: page.before, ..page_query })
        .expect("isolated fixture state");
    assert_eq!(older.entries[0].session_name_at_write.as_deref(), Some("Same name"));
    let read = reopened
        .read_many(&[id, aura_domain_common::MemoryId::new()])
        .expect("isolated fixture state");
    assert_eq!(read.missing_ids.len(), 1);
    let provenance = read.notes[0].provenance.as_ref().expect("isolated fixture state");
    assert_eq!(
        provenance.creator.as_ref().expect("isolated fixture state").session_id,
        actor.session_id
    );
    assert_eq!(provenance.modifiers[0].session_id, actor.session_id);
}

#[test]
fn legacy_notes_keep_honest_authorship_and_bytes_after_index_rebuild() {
    let fixture = Fixture::project();
    let primary = aura_domain_common::SessionId::new();
    let contributor = aura_domain_common::SessionId::new();
    let mut unknown = fixture.write("rules/contributed.md", "# 不推测创建者\n");
    unknown.note.metadata.source_sessions = vec![primary, contributor];
    let custom = "reviewer_annotation: \"keep this extension\"";
    unknown.note.metadata.user_properties.push(custom.into());
    let unknown_path = fixture.root.join(unknown.path.as_str());
    let unknown_text = aura_memory::render(&unknown.note).expect("old-format note");
    std::fs::create_dir_all(unknown_path.parent().expect("fixture category")).expect("category");
    std::fs::write(&unknown_path, &unknown_text).expect("write external fixture");

    let known = fixture.write("rules/audited.md", "# 有原始创建记录\n");
    let author = aura_memory::WriteActor {
        session_id: Some(primary),
        session_name: Some("名字只有新字段记录".into()),
        ..actor()
    };
    fixture.vault.write(std::slice::from_ref(&known), &author).expect("create audit row");
    let known_path = fixture.root.join(known.path.as_str());
    let mut old_format =
        aura_memory::parse(&std::fs::read_to_string(&known_path).expect("note")).expect("Markdown");
    old_format.metadata.user_properties.retain(|p| !p.starts_with("aura_creation_stamp:"));
    let known_text = aura_memory::render(&old_format).expect("old-format note with create audit");
    std::fs::write(&known_path, &known_text).expect("write external fixture");
    fixture.vault.rebuild().expect("index seed");

    let ids = [unknown.note.metadata.id, known.note.metadata.id];
    let compare = |vault: &aura_memory::Vault| {
        let batch = vault.read_many(&ids).expect("legacy notes");
        assert!(batch.missing_ids.is_empty());
        let notes = &batch.notes;
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].note.metadata.source_sessions, vec![primary, contributor]);
        assert_eq!(notes[0].note.body, unknown.note.body);
        assert!(notes[0].note.metadata.user_properties.iter().any(|property| property == custom));
        assert!(notes[0].provenance.as_ref().and_then(|p| p.creator.as_ref()).is_none());
        let creator = notes[1]
            .provenance
            .as_ref()
            .and_then(|p| p.creator.as_ref())
            .expect("original create audit, not the contribution list");
        assert_eq!(creator.session_id, Some(primary));
        assert_eq!(creator.session_name_at_write, None);
        assert_eq!(notes[1].note.body, known.note.body);
        assert!(notes[1].history.iter().any(|row| row.actor == "external"));
    };
    compare(&fixture.vault);
    let Fixture { temp, root, vault } = fixture;
    let scope = vault.scope();
    drop(vault);
    let cache = temp.path().join("cache/memory.db");
    std::fs::rename(&cache, temp.path().join("cache/index-before-rebuild.db"))
        .expect("move only the derived index");
    let reopened = aura_memory::Vault::open(root, scope, &cache).expect("rebuild from files");
    compare(&reopened);
    assert_eq!(std::fs::read_to_string(unknown_path).expect("unchanged note"), unknown_text);
    assert_eq!(std::fs::read_to_string(known_path).expect("unchanged note"), known_text);
}

#[test]
fn deleting_only_derived_sqlite_rebuilds_notes_and_metadata_only_history_from_files() {
    let fixture = Fixture::project();
    let write =
        fixture.write("rules/a.md", "# Original title\nBody must not be in the history journal.\n");
    let Fixture { temp, root, vault } = fixture;
    let id = write.note.metadata.id;
    let scope = vault.scope();
    let hashes = vault.write(&[write], &actor()).expect("create");
    vault.delete(&[(id, hashes[0].clone())], &actor()).expect("delete");
    assert_eq!(vault.history(Some(id), 10).expect("history").len(), 2);
    drop(vault);
    let records = temp.path().join("cache/memory.records/history");
    for file in std::fs::read_dir(records).expect("metadata journal") {
        let text = std::fs::read_to_string(file.expect("entry").path()).expect("metadata only");
        assert!(!text.contains("Body must not be"));
        assert!(text.contains("Original title"));
    }
    std::fs::remove_file(temp.path().join("cache/memory.db")).expect("delete derived cache only");
    let reopened = aura_memory::Vault::open(root, scope, &temp.path().join("cache/memory.db"))
        .expect("rebuild");
    let history = reopened.history(Some(id), 10).expect("replayed metadata history");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].action, "delete");
    assert_eq!(reopened.status().expect("status").valid, 0);
}

#[test]
fn history_stays_in_monthly_segments_instead_of_a_file_per_write() {
    let fixture = Fixture::project();
    for i in 0..40 {
        let write = fixture.write(
            &format!("rules/n{i}.md"),
            &format!(
                "# 第 {i} 条
"
            ),
        );
        fixture.vault.write(&[write], &actor()).expect("write");
    }
    let Fixture { temp, root, vault } = fixture;
    let records = temp.path().join("cache/memory.records/history");
    assert_eq!(
        std::fs::read_dir(&records).expect("history").count(),
        1,
        "one segment, not a file per batch"
    );
    assert_eq!(vault.history(None, 100).expect("history").len(), 40);
    let scope = vault.scope();
    drop(vault);
    // Rebuilding from the segment yields the same records, each once.
    std::fs::remove_file(temp.path().join("cache/memory.db")).expect("derived cache only");
    let reopened = aura_memory::Vault::open(root, scope, &temp.path().join("cache/memory.db"))
        .expect("rebuild");
    assert_eq!(reopened.history(None, 100).expect("history").len(), 40);
}
