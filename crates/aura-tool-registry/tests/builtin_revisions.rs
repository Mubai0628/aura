use aura_tool_registry::agent_release::{
    release,
    revisions::{PUBLISHED, PUBLISHED_BROWSER, PUBLISHED_MEDIA, current},
};

#[test]
fn every_published_key_and_fingerprint_is_retained_in_the_revision_table() {
    let release = release().expect("verified current release");
    assert!(
        PUBLISHED.len() + PUBLISHED_BROWSER.len() + PUBLISHED_MEDIA.len()
            >= release.manifest().entries.len(),
        "historical rows are retained alongside current rows"
    );
    let mut instances = std::collections::BTreeSet::new();
    for entry in &release.manifest().entries {
        let row = current(entry.handler.name()).expect("published row");
        assert_eq!(row.tool, entry.definition.tool_id.to_string());
        assert_eq!(row.instance, entry.instance.tool_instance_id.to_string());
        assert_eq!(row.revision, entry.definition.tool_version.definition_revision.value());
        assert_eq!(row.fingerprint, entry.definition.definition_fingerprint.hex());
        assert!(
            instances.insert(row.instance),
            "different handlers never reuse an instance identity"
        );
    }
}

/// Every row ever published, not only the current ones: a later revision appended to the table
/// must take a fresh instance identity, keep its handler's ToolId, move forward and carry new
/// content. Configuration carry-over maps an old instance to a new one; a reused instance or a
/// repeated fingerprint would make that mapping ambiguous.
#[test]
fn appended_revisions_take_fresh_identities_and_move_forward() {
    let mut instances = std::collections::BTreeSet::new();
    let mut by_name: std::collections::BTreeMap<&str, Vec<_>> = Default::default();
    for row in PUBLISHED.iter().chain(PUBLISHED_BROWSER).chain(PUBLISHED_MEDIA) {
        assert!(instances.insert(row.instance), "instance {} is published twice", row.instance);
        by_name.entry(row.name).or_default().push(row);
    }
    for (name, rows) in by_name {
        let mut fingerprints = std::collections::BTreeSet::new();
        for pair in rows.windows(2) {
            assert!(pair[0].revision < pair[1].revision, "{name}: revisions move forward");
            assert_eq!(pair[0].tool, pair[1].tool, "{name}: a revision keeps its ToolId");
        }
        for row in rows {
            assert!(fingerprints.insert(row.fingerprint), "{name}: a revision changes content");
        }
    }
}
