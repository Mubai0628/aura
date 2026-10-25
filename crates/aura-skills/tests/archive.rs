//! Skill archives: the two layouts unpack into a valid Skill, and every unsafe entry, duplicate,
//! limit or layout problem is refused before anything is written.
use aura_skills::{
    FileHashCache, MAX_ARCHIVE_BYTES, MAX_SKILL_FILES, SkillArchiveError, scan_skill,
    unpack_skill_archive,
};
use std::io::Write;
use std::path::{Path, PathBuf};
use zip::{CompressionMethod, write::SimpleFileOptions};

const SKILL: &[u8] = b"---\nname: notes\ndescription: Notes in bullets\n---\n\nRead [the guide](references/guide.md).\n";

enum Item<'a> {
    File(&'a str, &'a [u8]),
    /// Always stored uncompressed, whatever the archive's method.
    Plain(&'a str, &'a [u8]),
    Folder(&'a str),
    Link(&'a str, &'a str),
}

fn archive(dir: &Path, items: &[Item<'_>], method: CompressionMethod) -> PathBuf {
    let path = dir.join(format!("skill-{}.zip", items.len()));
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).expect("zip"));
    let options = SimpleFileOptions::default().compression_method(method);
    for item in items {
        match item {
            Item::File(name, bytes) => {
                zip.start_file(*name, options).expect("entry");
                zip.write_all(bytes).expect("bytes");
            }
            Item::Plain(name, bytes) => {
                let stored = options.compression_method(CompressionMethod::Stored);
                zip.start_file(*name, stored).expect("entry");
                zip.write_all(bytes).expect("bytes");
            }
            Item::Folder(name) => zip.add_directory(*name, options).expect("folder"),
            Item::Link(name, target) => zip.add_symlink(*name, *target, options).expect("link"),
        }
    }
    zip.finish().expect("finish");
    path
}

fn unpack(path: &Path) -> (tempfile::TempDir, Result<PathBuf, SkillArchiveError>) {
    let out = tempfile::tempdir().expect("out");
    let result = unpack_skill_archive(std::fs::File::open(path).expect("open"), out.path());
    (out, result)
}

fn written(dir: &Path) -> usize {
    std::fs::read_dir(dir).expect("dir").count()
}

#[test]
fn skill_md_at_the_top_or_in_the_only_folder_unpacks_into_a_valid_skill() {
    let temp = tempfile::tempdir().expect("temp");
    let flat = archive(
        temp.path(),
        &[Item::File("SKILL.md", SKILL), Item::File("references/guide.md", b"# Guide\n")],
        CompressionMethod::Deflated,
    );
    let (out, root) = unpack(&flat);
    let root = root.expect("flat layout");
    assert_eq!(root, out.path());
    let package = scan_skill(&root, &FileHashCache::default()).expect("a valid Skill");
    assert_eq!(package.files.len(), 2);

    // One top-level folder, listed folders, a Mac's extras and Windows separators.
    let nested = archive(
        temp.path(),
        &[
            Item::Folder("notes/"),
            Item::File("notes/SKILL.md", SKILL),
            Item::Folder("notes/references/"),
            Item::File("notes\\references\\guide.md", b"# Guide\n"),
            Item::File("notes/.DS_Store", b"mac"),
            Item::File("__MACOSX/notes/._SKILL.md", b"mac"),
        ],
        CompressionMethod::Stored,
    );
    let (out, root) = unpack(&nested);
    let root = root.expect("nested layout");
    assert_eq!(root, out.path().join("notes"));
    assert!(!out.path().join("__MACOSX").exists(), "a Mac's folder is left out");
    assert!(!root.join(".DS_Store").exists());
    assert!(root.join("references").join("guide.md").is_file());
    let package = scan_skill(&root, &FileHashCache::default()).expect("a valid Skill");
    assert_eq!(
        package.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(),
        ["SKILL.md", "references/guide.md"]
    );
}

#[test]
fn unsafe_paths_and_links_are_refused_before_anything_is_written() {
    let temp = tempfile::tempdir().expect("temp");
    for name in [
        "../evil.md",
        "references/../../evil.md",
        "/absolute.md",
        "C:/drive.md",
        "notes:stream.md",
        "con.txt",
        "trailing.",
        "bad\u{1}name.md",
    ] {
        let path = archive(
            temp.path(),
            &[Item::File("SKILL.md", SKILL), Item::File(name, b"x")],
            CompressionMethod::Stored,
        );
        let (out, result) = unpack(&path);
        assert!(matches!(result, Err(SkillArchiveError::Path(_))), "{name}: {result:?}");
        assert_eq!(written(out.path()), 0, "{name}: nothing is written");
    }
    let linked = archive(
        temp.path(),
        &[Item::File("SKILL.md", SKILL), Item::Link("references", "C:/Windows")],
        CompressionMethod::Stored,
    );
    let (out, result) = unpack(&linked);
    assert!(matches!(result, Err(SkillArchiveError::SpecialFile(_))), "{result:?}");
    assert_eq!(written(out.path()), 0);
}

#[test]
fn duplicates_limits_bombs_and_layouts_are_refused() {
    let temp = tempfile::tempdir().expect("temp");
    let refused = |items: &[Item<'_>], method| unpack(&archive(temp.path(), items, method)).1;

    // Windows ignores case; a file and a folder cannot share a name.
    let case = refused(
        &[Item::File("SKILL.md", SKILL), Item::File("skill.md", b"x")],
        CompressionMethod::Stored,
    );
    assert!(matches!(case, Err(SkillArchiveError::Duplicate(_))), "{case:?}");
    let file_and_folder = refused(
        &[Item::File("SKILL.md", SKILL), Item::File("a", b"x"), Item::File("a/b.md", b"x")],
        CompressionMethod::Stored,
    );
    assert!(matches!(file_and_folder, Err(SkillArchiveError::Duplicate(_))), "{file_and_folder:?}");

    // The same limits as a folder.
    let names: Vec<String> = (0..MAX_SKILL_FILES).map(|i| format!("r/{i}.md")).collect();
    let mut many = vec![Item::File("SKILL.md", SKILL)];
    many.extend(names.iter().map(|name| Item::File(name, b"x")));
    let many = refused(&many, CompressionMethod::Stored);
    assert!(matches!(many, Err(SkillArchiveError::Limits { .. })), "{many:?}");
    let big = vec![b'x'; 8 * 1024 * 1024];
    let large = refused(
        &[Item::File("SKILL.md", SKILL), Item::File("big.md", &big)],
        CompressionMethod::Stored,
    );
    assert!(matches!(large, Err(SkillArchiveError::Limits { .. })), "{large:?}");

    // A small file that expands a thousandfold is refused as a bomb.
    let zeros = vec![0_u8; 1024 * 1024];
    let bomb = refused(
        &[Item::File("SKILL.md", SKILL), Item::File("zeros.bin", &zeros)],
        CompressionMethod::Deflated,
    );
    assert_eq!(bomb, Err(SkillArchiveError::Ratio));
    // The same bomb among plain bytes: the archive as a whole compresses normally, but the one
    // entry does not.
    let plain = vec![b'p'; 2 * 1024 * 1024];
    let hidden = refused(
        &[
            Item::File("SKILL.md", SKILL),
            Item::File("zeros.bin", &zeros),
            Item::Plain("plain.bin", &plain),
        ],
        CompressionMethod::Deflated,
    );
    assert_eq!(hidden, Err(SkillArchiveError::Ratio));

    // SKILL.md at the top or in the only top-level folder.
    for layout in [
        vec![Item::File("a/SKILL.md", SKILL), Item::File("b/SKILL.md", SKILL)],
        vec![Item::File("README.md", b"x")],
        vec![Item::File("a/SKILL.md", SKILL), Item::File("loose.md", b"x")],
    ] {
        assert!(matches!(
            refused(&layout, CompressionMethod::Stored),
            Err(SkillArchiveError::Layout(_))
        ));
    }

    // Not a ZIP at all, or a file too large to read.
    let text = temp.path().join("not.zip");
    std::fs::write(&text, b"plain text, not an archive").expect("text");
    assert_eq!(unpack(&text).1, Err(SkillArchiveError::NotZip));
    let huge = temp.path().join("huge.zip");
    std::fs::File::create(&huge).expect("huge").set_len(MAX_ARCHIVE_BYTES + 1).expect("length");
    assert_eq!(unpack(&huge).1, Err(SkillArchiveError::TooLarge(MAX_ARCHIVE_BYTES)));
}
