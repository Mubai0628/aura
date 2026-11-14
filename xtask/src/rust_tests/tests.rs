use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use super::{
    alone,
    artifacts::{self, Kind, Program},
    listeners,
    options::Options,
    threads, used,
};

fn program(package: &str, kind: Kind, name: &str) -> Program {
    Program {
        package: package.to_owned(),
        version: "0.3.0".to_owned(),
        manifest_dir: PathBuf::from(format!(r"D:\w\crates\{package}")),
        kind,
        name: name.to_owned(),
        source: PathBuf::from("src/lib.rs"),
        executable: PathBuf::from(format!(r"D:\w\target\debug\deps\{name}-0.exe")),
    }
}

fn programs() -> Vec<Program> {
    vec![
        program("aura-core", Kind::Lib, "aura_core"),
        program("aura-core", Kind::Bin, "aura-core"),
        program("aura-core", Kind::Test, "core"),
        program("aura-fs-tools", Kind::Lib, "aura_fs_tools"),
        program("aura-fs-tools", Kind::Test, "fs_tools"),
    ]
}

fn options(arguments: &[&str]) -> Options {
    Options::parse(arguments.iter().map(|&value| value.to_owned()).collect())
        .expect("arguments xtask takes")
}

fn names(selected: &[&Program]) -> Vec<String> {
    selected.iter().map(|program| format!("{} {}", program.package, program.label())).collect()
}

fn select<'a>(arguments: &[&str], all: &'a [Program]) -> Vec<&'a Program> {
    options(arguments).select(all).expect("a selection with programs")
}

#[test]
fn the_selection_takes_packages_kinds_and_named_programs_and_keeps_the_test_arguments() {
    let all = programs();
    assert_eq!(names(&select(&[], &all)).len(), 5);
    assert_eq!(
        names(&select(&["-p", "aura-fs-tools"], &all)),
        ["aura-fs-tools lib", "aura-fs-tools test fs_tools"]
    );
    assert_eq!(
        names(&select(&["--package", "aura-core", "--lib", "--bins"], &all)),
        ["aura-core lib", "aura-core bin aura-core"]
    );
    assert_eq!(names(&select(&["--test", "fs_tools"], &all)), ["aura-fs-tools test fs_tools"]);
    let parsed = options(&["-p", "aura-core", "--", "s08::", "--exact"]);
    assert_eq!(parsed.test_arguments, ["s08::", "--exact"]);
}

#[test]
fn what_every_run_is_anyway_is_accepted_and_anything_that_changes_the_build_is_refused() {
    assert_eq!(
        options(&["--workspace", "--all-features", "--no-fail-fast", "--locked"]),
        Options::default()
    );
    for refused in [&["--features", "x"][..], &["--release"], &["--doc"], &["--target-dir", "t"]] {
        let error = Options::parse(refused.iter().map(|&value| value.to_owned()).collect())
            .expect_err("an option that changes the build")
            .to_string();
        assert!(error.contains("is not supported"), "{refused:?}: {error}");
    }
}

#[test]
fn an_unknown_package_or_program_is_an_error_not_an_empty_run() {
    let all = programs();
    assert!(options(&["-p", "aura-nothing"]).select(&all).is_err());
    assert!(options(&["-p", "aura-core", "--test", "fs_tools"]).select(&all).is_err());
}

#[test]
fn a_budgeted_test_is_skipped_in_the_parallel_run_and_then_runs_alone_in_its_program() {
    let all = programs();
    let selected = select(&[], &all);
    let alone = alone::in_selection(&selected);
    assert_eq!(alone.len(), 1);
    assert_eq!(alone[0].0.label(), "lib");
    assert_eq!(alone[0].0.package, "aura-core");
    let test = alone[0].1;
    assert!(
        test.ends_with("s08_real_core_shell_return_times_include_authority_and_durable_results")
    );
    assert!(!test.contains(' '), "{test}");
    assert_eq!(alone::skip_arguments(&alone), ["--skip", test]);
    assert_eq!(alone::run_arguments(test), [test, "--exact", "--test-threads=1"]);
    assert!(alone::in_selection(&select(&["-p", "aura-fs-tools"], &all)).is_empty());
}

#[test]
fn the_desktop_tests_run_one_at_a_time_and_every_other_program_in_parallel() {
    assert_eq!(
        threads::for_program(&program("aura-desktop-control", Kind::Test, "desktop_control")),
        "1"
    );
    assert_eq!(
        threads::for_program(&program("aura-desktop-control", Kind::Lib, "aura_desktop_control")),
        crate::TEST_THREADS
    );
    assert_eq!(
        threads::for_program(&program("aura-fs-tools", Kind::Test, "fs_tools")),
        crate::TEST_THREADS
    );
}

#[test]
fn the_core_tests_that_listen_on_the_lan_run_from_a_fixed_name_beside_the_program() {
    assert!(listeners::is_listener("aura-core", Kind::Lib));
    assert!(!listeners::is_listener("aura-core", Kind::Test));
    assert!(!listeners::is_listener("aura-fs-tools", Kind::Lib));
    assert_eq!(
        listeners::fixed_name(Path::new(r"D:\w\target\debug\deps\aura_core-0123456789abcdef.exe")),
        PathBuf::from(r"D:\w\target\debug\deps\aura_core-tests.exe")
    );
}

#[test]
fn a_listeners_link_is_made_again_so_it_never_runs_an_old_build() {
    let folder = tempfile::tempdir().expect("a test folder");
    let built = folder.path().join("aura_core-0123456789abcdef.exe");
    let write = |text: &str| fs::write(&built, text).expect("the built program");
    let read = |path: &Path| fs::read_to_string(path).expect("the linked program");
    write("first build");
    let mut core = program("aura-core", Kind::Lib, "aura_core");
    core.executable = built.clone();
    let link = listeners::launch_path(&core);
    assert_eq!(link, folder.path().join("aura_core-tests.exe"));
    assert_eq!(read(&link), "first build");
    // The linker writes a rebuilt program as a new file; the old link still holds the old one.
    fs::remove_file(&built).expect("the old build removed");
    write("second build");
    assert_eq!(read(&link), "first build");
    assert_eq!(listeners::launch_path(&core), link);
    assert_eq!(read(&link), "second build");
    let other = program("aura-fs-tools", Kind::Lib, "aura_fs_tools");
    assert_eq!(listeners::launch_path(&other), other.executable);
}

fn artifact(
    kind: &str,
    name: &str,
    test: bool,
    files: &[&str],
    executable: Option<&str>,
) -> String {
    serde_json::json!({
        "reason": "compiler-artifact",
        "manifest_path": r"D:\w\crates\a\Cargo.toml",
        "target": {"kind": [kind], "name": name, "src_path": format!(r"D:\w\crates\a\{name}.rs")},
        "profile": {"test": test},
        "filenames": files,
        "executable": executable,
        "fresh": true,
    })
    .to_string()
}

#[test]
fn cargos_artifact_messages_give_the_test_programs_with_package_and_kind() {
    let packages = HashMap::from([(
        PathBuf::from(r"D:\w\crates\a\Cargo.toml"),
        ("aura-a".to_owned(), "0.3.0".to_owned()),
    )]);
    let stdout = [
        artifact("lib", "aura_a", false, &[], None),
        artifact("lib", "aura_a", true, &[], Some(r"D:\w\target\debug\deps\aura_a-1.exe")),
        artifact("bin", "aura-a", false, &[], Some(r"D:\w\target\debug\deps\aura_a-2.exe")),
        artifact("test", "a", true, &[], Some(r"D:\w\target\debug\deps\a-3.exe")),
        r#"{"reason":"build-finished","success":true}"#.to_owned(),
        "not json".to_owned(),
    ]
    .join("\n");
    let parsed = artifacts::parse(&stdout, &packages).expect("the test programs");
    assert_eq!(names(&parsed.iter().collect::<Vec<_>>()), ["aura-a lib", "aura-a test a"]);
    assert_eq!(parsed[0].manifest_dir, PathBuf::from(r"D:\w\crates\a"));
    assert_eq!(parsed[1].executable, PathBuf::from(r"D:\w\target\debug\deps\a-3.exe"));
}

#[test]
fn a_record_keeps_the_deps_files_of_every_unit_cargo_reported() {
    let stdout = [
        artifact(
            "lib",
            "aura_a",
            false,
            &[
                r"D:\w\target\debug\deps\libaura_a-01.rlib",
                r"D:\w\target\debug\deps\libaura_a-01.rmeta",
            ],
            None,
        ),
        artifact(
            "bin",
            "aura-a",
            false,
            &[r"D:\w\target\debug\aura-a.exe", r"D:\w\target\debug\aura_a.pdb"],
            Some(r"D:\w\target\debug\aura-a.exe"),
        ),
        artifact("test", "a", true, &[r"D:\w\target\debug\deps\a-02.exe"], None),
        r#"{"reason":"build-script-executed","package_id":"x"}"#.to_owned(),
    ]
    .join("\n");
    assert_eq!(
        used::deps_files(&stdout).into_iter().collect::<Vec<_>>(),
        ["a-02.exe", "libaura_a-01.rlib", "libaura_a-01.rmeta"]
    );
}

#[test]
fn the_units_in_use_come_from_both_records_and_date_from_the_older_one() {
    let folder = tempfile::tempdir().expect("a test folder");
    assert!(used::units_in(folder.path(), "").is_none(), "no record yet");
    let clippy_time = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1 << 30);
    for (record, files, written) in [
        (used::TEST_BUILD, "a-02.exe\nlibaura_a-01.rlib\n", None),
        (used::CLIPPY, "libaura_a-05.rmeta\n", Some(clippy_time)),
    ] {
        let path = folder.path().join(format!("{record}.txt"));
        fs::write(&path, files).expect("a record");
        if let Some(written) = written {
            fs::File::options()
                .write(true)
                .open(&path)
                .and_then(|file| file.set_modified(written))
                .expect("the record's time");
        }
    }
    let xtask = artifact("bin", "xtask", false, &[r"D:\w\target\debug\deps\xtask-06.exe"], None);
    let found = used::units_in(folder.path(), &xtask).expect("both records");
    let mut units = found.units.into_iter().collect::<Vec<_>>();
    units.sort();
    assert_eq!(
        units,
        ["a-02", "aura_a-01", "aura_a-05", "libaura_a-01", "libaura_a-05", "xtask-06"]
    );
    assert_eq!(found.recorded, clippy_time);
}

#[test]
fn a_deps_file_names_its_unit_with_and_without_the_library_prefix() {
    let units = |file: &'static str| used::unit_names(file).collect::<Vec<_>>();
    assert_eq!(units("libaura_a-01.rlib"), ["libaura_a-01", "aura_a-01"]);
    assert_eq!(units("aura_a-01.d"), ["aura_a-01"]);
    assert_eq!(units("serde_derive-03.dll.lib"), ["serde_derive-03"]);
    assert_eq!(units("libc-04.d"), ["libc-04", "c-04"]);
}
