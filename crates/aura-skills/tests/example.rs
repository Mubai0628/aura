//! The documented example (`docs/examples/skills/answer-signature`) is a valid Skill exactly as
//! the owner imports it: no problem, no warning, and every file it refers to is there.
use aura_skills::{FileHashCache, read_resource, scan_skill};
use std::path::PathBuf;

#[test]
fn the_documented_example_is_a_valid_skill() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/examples/skills/answer-signature");
    let cache = FileHashCache::default();
    let package = scan_skill(&directory, &cache).expect("the example is a valid Skill");
    assert_eq!(package.frontmatter.name, "answer-signature");
    assert_eq!(package.frontmatter.allowed_tools, ["fs.write"]);
    assert!(package.warnings.is_empty(), "{:?}", package.warnings);
    assert!(package.missing_references.is_empty(), "{:?}", package.missing_references);
    assert_eq!(
        package.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        ["SKILL.md", "references/checklist.md"]
    );
    let checklist = read_resource(&directory, &cache, "references/checklist.md").expect("readable");
    assert!(checklist.resource.as_deref().is_some_and(|text| text.contains("署名前空一行")));
    assert_eq!(checklist.fingerprint, package.fingerprint);
}
