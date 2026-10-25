//! Skill folders: the fingerprint covers every file, links are never followed, limits hold,
//! resources stay inside the folder, and the root scan finds Skills one category deep.
use aura_skills::{
    FileHashCache, MAX_RESOURCE_BYTES, SkillPackageError, SkillResourceError, SkillSetting,
    SkillSource, SkillState, read_resource, scan_root, scan_skill, skill_id, view,
};
use std::path::Path;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("folder");
    std::fs::write(path, text).expect("file");
}

fn skill(root: &Path, name: &str) -> std::path::PathBuf {
    let directory = root.join(name);
    write(
        &directory.join("SKILL.md"),
        &format!(
            "---\nname: {name}\ndescription: Test skill {name}\n---\n\nRead [the guide](references/guide.md) first.\n"
        ),
    );
    write(&directory.join("references/guide.md"), "# Guide\n\nStep one.\n");
    directory
}

#[test]
fn the_fingerprint_covers_every_file_and_changes_with_any_of_them() {
    let temp = tempfile::tempdir().expect("temp");
    let directory = skill(temp.path(), "notes");
    let cache = FileHashCache::default();
    let first = scan_skill(&directory, &cache).expect("valid");
    assert_eq!(first.frontmatter.name, "notes");
    assert_eq!(
        first.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        ["SKILL.md", "references/guide.md"]
    );
    assert!(first.missing_references.is_empty());
    assert!(first.warnings.is_empty(), "{:?}", first.warnings);
    assert_eq!(scan_skill(&directory, &cache).expect("same").fingerprint, first.fingerprint);
    write(&directory.join("references/guide.md"), "# Guide\n\nStep two.\n");
    let changed = scan_skill(&directory, &FileHashCache::default()).expect("valid");
    assert_ne!(changed.fingerprint, first.fingerprint, "a resource change is a new content");
    write(&directory.join("scripts/run.py"), "print('x')\n");
    assert_ne!(
        scan_skill(&directory, &FileHashCache::default()).expect("valid").fingerprint,
        changed.fingerprint,
        "an added file is a new content"
    );
}

#[test]
fn missing_links_and_a_different_name_are_warnings_not_failures() {
    let temp = tempfile::tempdir().expect("temp");
    let directory = temp.path().join("folder-name");
    write(
        &directory.join("SKILL.md"),
        "---\nname: other-name\ndescription: d\n---\nSee [a](references/missing.md), [web](https://example.com) and [top](#top).\n",
    );
    let package = scan_skill(&directory, &FileHashCache::default()).expect("still usable");
    assert_eq!(package.missing_references, ["references/missing.md"]);
    assert!(package.warnings.iter().any(|w| w.contains("与文件夹名")), "{:?}", package.warnings);
}

#[test]
fn links_limits_and_bad_skill_files_are_refused() {
    let temp = tempfile::tempdir().expect("temp");
    let cache = FileHashCache::default();
    assert_eq!(
        scan_skill(&temp.path().join("absent"), &cache).expect_err("missing"),
        SkillPackageError::NotAFolder
    );
    let empty = temp.path().join("empty");
    std::fs::create_dir(&empty).expect("folder");
    assert_eq!(
        scan_skill(&empty, &cache).expect_err("no SKILL.md"),
        SkillPackageError::MissingSkillFile
    );
    let invalid = temp.path().join("invalid");
    write(&invalid.join("SKILL.md"), "no front matter");
    assert!(matches!(scan_skill(&invalid, &cache), Err(SkillPackageError::Invalid(_))));
    let outside = tempfile::tempdir().expect("outside");
    write(&outside.path().join("secret.txt"), "not part of the skill");
    let linked = skill(temp.path(), "linked");
    junction::create(outside.path(), linked.join("references/outside")).expect("junction");
    assert!(
        matches!(scan_skill(&linked, &cache), Err(SkillPackageError::Linked(path)) if path.contains("outside")),
        "a link inside the folder is never followed"
    );
    let large = skill(temp.path(), "large");
    std::fs::write(large.join("blob.bin"), vec![0_u8; 9 * 1024 * 1024]).expect("blob");
    assert_eq!(scan_skill(&large, &cache).expect_err("too large"), SkillPackageError::TooLarge);
}

#[test]
fn resources_are_read_inside_the_folder_only_and_as_fingerprinted() {
    let temp = tempfile::tempdir().expect("temp");
    let directory = skill(temp.path(), "notes");
    let cache = FileHashCache::default();
    let read = read_resource(&directory, &cache, "references\\guide.md").expect("inside");
    assert_eq!(read.resource.as_deref(), Some("# Guide\n\nStep one.\n"));
    assert_eq!(read.fingerprint, scan_skill(&directory, &cache).expect("valid").fingerprint);
    for outside in
        ["../notes/SKILL.md", "C:/Windows/win.ini", "/etc/passwd", "references/../../x", ""]
    {
        assert_eq!(
            read_resource(&directory, &cache, outside).expect_err("outside"),
            SkillResourceError::Path,
            "{outside}"
        );
    }
    assert!(matches!(
        read_resource(&directory, &cache, "references/absent.md"),
        Err(SkillResourceError::NotFound(_))
    ));
    std::fs::write(directory.join("image.png"), [0_u8, 159, 146, 150]).expect("binary");
    assert!(matches!(
        read_resource(&directory, &cache, "image.png"),
        Err(SkillResourceError::NotText(_))
    ));
    std::fs::write(directory.join("big.md"), "x".repeat(MAX_RESOURCE_BYTES as usize + 1))
        .expect("big");
    let refused = read_resource(&directory, &cache, "big.md").expect_err("too large to read");
    assert!(matches!(refused, SkillResourceError::TooLarge(_)));
    // A large file still counts in the fingerprint in full.
    let before = scan_skill(&directory, &FileHashCache::default()).expect("valid").fingerprint;
    std::fs::write(directory.join("big.md"), "y".repeat(MAX_RESOURCE_BYTES as usize + 1))
        .expect("big");
    assert_ne!(
        scan_skill(&directory, &FileHashCache::default()).expect("valid").fingerprint,
        before
    );
}

#[test]
fn roots_are_scanned_one_category_deep_and_states_follow_the_saved_choices() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path();
    skill(root, "alpha");
    skill(&root.join("writing"), "beta");
    skill(&root.join(".staging"), "hidden");
    std::fs::create_dir_all(root.join("empty-category/nothing")).expect("folder");
    let outside = tempfile::tempdir().expect("outside");
    skill(outside.path(), "gamma");
    junction::create(outside.path().join("gamma"), root.join("linked")).expect("junction");
    let found = scan_root(root, &FileHashCache::default());
    let relative: Vec<_> = found.iter().map(|f| f.relative.as_str()).collect();
    assert_eq!(relative, ["alpha", "linked", "writing/beta"]);
    assert!(matches!(found[1].scanned, Err(SkillPackageError::Linked(_))));

    let alpha = &found[0];
    let fingerprint = alpha.scanned.as_ref().expect("valid").fingerprint.clone();
    let id = skill_id(SkillSource::AgentGlobal, None, "alpha");
    let setting = |enabled, trusted: Option<&str>| SkillSetting {
        id,
        source: SkillSource::AgentGlobal,
        workspace_id: None,
        relative: "alpha".into(),
        enabled,
        trusted_fingerprint: trusted.map(str::to_owned),
        available_for_subagents: false,
    };
    let state =
        |setting: Option<&SkillSetting>| view(alpha, SkillSource::AgentGlobal, None, setting).state;
    assert_eq!(state(None), SkillState::Disabled, "found outside Aura: off until switched on");
    assert_eq!(view(alpha, SkillSource::AuraStore, None, None).state, SkillState::NotTrusted);
    assert_eq!(state(Some(&setting(true, None))), SkillState::NotTrusted);
    assert_eq!(state(Some(&setting(true, Some(&fingerprint)))), SkillState::Ready);
    assert_eq!(state(Some(&setting(true, Some("0123")))), SkillState::ContentChanged);
    assert_eq!(state(Some(&setting(false, Some(&fingerprint)))), SkillState::Disabled);
    assert_eq!(
        view(&found[1], SkillSource::AgentGlobal, None, None).state,
        SkillState::Unavailable
    );
    // The same folder name in another place is another Skill.
    assert_ne!(id, skill_id(SkillSource::AuraStore, None, "alpha"));
    let workspace = aura_domain_common::WorkspaceId::new();
    assert_ne!(
        skill_id(SkillSource::Workspace, Some(workspace), "alpha"),
        skill_id(SkillSource::Workspace, Some(aura_domain_common::WorkspaceId::new()), "alpha")
    );
    assert_eq!(id, skill_id(SkillSource::AgentGlobal, None, "ALPHA"), "Windows names ignore case");
}
