use super::*;
use aura_windows_platform::{open_held_for_move, open_verified_publish_parent};
use std::{fs::OpenOptions, os::windows::fs::OpenOptionsExt, path::Path};

/// Another program's handle on a file inside the folder, opened the way a scanner reads it: shared
/// for reading only, which is what makes Windows refuse to rename the folder around it.
fn hold_inside(folder: &Path) -> std::fs::File {
    const FILE_SHARE_READ: u32 = 1;
    OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(folder.join("inside.txt"))
        .expect("a reader's handle")
}

fn folder_with_a_file() -> (tempfile::TempDir, std::path::PathBuf) {
    let root = tempfile::tempdir().expect("root");
    let folder = root.path().join("tree");
    std::fs::create_dir(&folder).expect("folder");
    std::fs::write(folder.join("inside.txt"), b"inside").expect("file");
    (root, folder)
}

#[test]
fn a_folder_rename_another_program_briefly_blocks_goes_through_once_it_lets_go() {
    let (root, folder) = folder_with_a_file();
    let reader = hold_inside(&folder);
    let held = open_held_for_move(&folder).expect("held folder");
    let parent = open_verified_publish_parent(root.path()).expect("parent");
    // One try alone is refused while the reader has the file open.
    assert!(matches!(
        handle_bound_move_existing(&held, &parent, OsStr::new("moved")),
        Err(HandlePublishError::NativeRename { .. })
    ));
    let letting_go = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        drop(reader);
    });
    move_held(&held, &parent, OsStr::new("moved")).expect("moved once the reader let go");
    letting_go.join().expect("reader thread");
    drop(held);
    assert!(root.path().join("moved").join("inside.txt").exists());
    assert!(!folder.exists());
}

#[test]
fn a_rename_still_blocked_after_the_retries_is_refused_as_before() {
    let (root, folder) = folder_with_a_file();
    let _reader = hold_inside(&folder);
    let held = open_held_for_move(&folder).expect("held folder");
    let parent = open_verified_publish_parent(root.path()).expect("parent");
    let started = Instant::now();
    let error = move_held(&held, &parent, OsStr::new("moved")).expect_err("still blocked");
    assert!(matches!(error, HandlePublishError::NativeRename { .. }), "{error}");
    assert!(started.elapsed() >= RETRY_FOR, "tried for the whole window");
    assert!(folder.exists() && !root.path().join("moved").exists());
}
