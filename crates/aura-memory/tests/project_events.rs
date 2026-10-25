mod support;
use aura_domain_common::{SessionId, UtcTimestamp, WorkspacePath};
use aura_memory::*;
use support::*;

fn event(id: &str, at: UtcTimestamp) -> ProjectEvent {
    ProjectEvent {
        id: blake3::hash(id.as_bytes()).to_hex().to_string(),
        at,
        title: format!("工作 {id}"),
        summary: format!("已验证 {id}"),
        session_id: Some(SessionId::new()),
    }
}
#[test]
fn two_sessions_merge_events_without_losing_content_and_archive_only_expired_blocks() {
    let f = Fixture::project();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let threads = (0..2)
        .map(|i| {
            let vault = f.vault.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                let event = event(&format!("session-{i}"), UtcTimestamp::now());
                let actor = WriteActor {
                    kind: "automatic_maintenance".into(),
                    session_id: event.session_id,
                    turn_id: None,
                };
                barrier.wait();
                vault.append_events(&[event], 7, &actor).expect("atomic merge");
            })
        })
        .collect::<Vec<_>>();
    for task in threads {
        task.join().expect("session");
    }
    let recent = WorkspacePath::parse("events/recent.md").expect("path");
    let read = f.vault.read_path(&recent).expect("recent");
    assert!(read.note.body.contains("session-0") && read.note.body.contains("session-1"));
    assert_eq!(read.note.body.matches("<!-- aura-event ").count(), 2);
    assert_eq!(read.note.metadata.source_sessions.len(), 2);
    let old = UtcTimestamp::from_value(UtcTimestamp::now().value() - time::Duration::days(40));
    f.vault.append_events(&[event("expired", old)], 7, &actor()).expect("archive old event");
    assert!(!f.vault.read_path(&recent).expect("recent").note.body.contains("expired"));
    let at = old.value();
    let path = WorkspacePath::parse(format!(
        "events/archive/{:04}-{:02}.md",
        at.year(),
        u8::from(at.month())
    ))
    .expect("archive");
    let archived = f.vault.read_path(&path).expect("archive exists");
    assert!(archived.note.body.contains("expired"));
    assert_eq!(archived.note.metadata.status, MemoryStatus::Archived);
}

#[test]
fn malformed_external_event_markers_are_preserved_without_overwrite() {
    let f = Fixture::project();
    let original = f
        .write("events/recent.md", "# 手工事件\n<!-- aura-event damaged -->\n保留这段用户正文。\n");
    f.vault.write(&[original], &actor()).expect("manual file");
    let before = std::fs::read(f.root.join("events/recent.md")).expect("before");
    assert!(f.vault.append_events(&[event("new", UtcTimestamp::now())], 30, &actor()).is_err());
    assert_eq!(before, std::fs::read(f.root.join("events/recent.md")).expect("not overwritten"));
}
