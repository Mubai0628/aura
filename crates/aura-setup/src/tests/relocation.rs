//! Moving an installation: it arrives whole, private and marked, its registrations follow it and
//! the old folder goes; a move that fails part way leaves it where it was; a folder that cannot
//! take it says why.
use std::{
    fs,
    path::{Path, PathBuf},
};

use aura_chrome_domain::protocol::native_host_name;
use aura_launcher::{read_install_state, resolve_version};

use super::{NAMESPACE, Work, namespace};
use crate::{
    folder_check::Refusal,
    host::Host,
    marker,
    relocate::{self, Fault},
};

const SHORTCUT: &str = "Aura Test setup.lnk";

/// Whether a shortcut's bytes name `folder`, as its working directory does in UTF-16.
fn names(link: &[u8], folder: &Path) -> bool {
    let wide: Vec<u8> =
        folder.to_string_lossy().encode_utf16().flat_map(u16::to_le_bytes).collect();
    link.windows(wide.len()).any(|window| window == wide)
}

fn registered_at(work: &Work, folder: &Path) -> bool {
    let host = work.host();
    host.entry(&namespace()).unwrap()["InstallLocation"].as_str().map(PathBuf::from)
        == Some(folder.to_path_buf())
}

#[test]
fn the_installation_moves_whole_and_its_registrations_follow() {
    let work = Work::new();
    let host = work.host();
    work.install(&work.release("0.1.0"), None, true).unwrap();
    let from = work.root();
    // A taskbar pin, as the user makes one, and the relay's registration, as Core writes it.
    let pins = host.taskbar_pins().unwrap();
    fs::create_dir_all(&pins).unwrap();
    fs::copy(host.start_menu().unwrap().join(SHORTCUT), pins.join(SHORTCUT)).unwrap();
    let relay = from.join(r"versions\0.1.0\aura-chrome-host.exe");
    let manifest = work.folder.path().join("relay.json");
    fs::write(&manifest, serde_json::json!({ "name": "relay", "path": relay }).to_string())
        .unwrap();
    host.register_chrome_host(&native_host_name(NAMESPACE), &manifest);

    let to = work.folder.path().join("moved").join("Aura");
    let relocation = relocate::plan(&host, &from, &to).unwrap();
    let moved = relocate::relocate(&host, &relocation, &mut |_, _, _| {}, &|| false, None).unwrap();
    assert!(moved.left_behind.is_empty(), "{:?}", moved.left_behind);
    assert!(moved.notes.is_empty(), "{:?}", moved.notes);

    assert!(!from.exists(), "the old folder is still there");
    let state = read_install_state(&to).unwrap();
    resolve_version(&to, "0.1.0", Some(&state.release_descriptor)).unwrap();
    assert_eq!(fs::read(to.join("AuraLauncher.exe")).unwrap(), b"launcher 0.1.0");
    assert!(marker::is_installation_of(&to, &namespace()));
    assert_eq!(aura_windows_platform::private_folder_problem(&to).unwrap(), None);
    assert!(registered_at(&work, &to));
    let entry = host.entry(&namespace()).unwrap();
    assert!(entry["UninstallString"].as_str().unwrap().contains(&to.display().to_string()));
    for folder in [host.start_menu().unwrap(), host.desktop().unwrap(), pins] {
        let link = fs::read(folder.join(SHORTCUT)).unwrap();
        assert!(names(&link, &to), "{} names the old folder", folder.display());
    }
    let written: serde_json::Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(
        written["path"].as_str().map(PathBuf::from),
        Some(to.join(r"versions\0.1.0\aura-chrome-host.exe"))
    );
}

#[test]
fn a_move_chosen_with_an_upgrade_names_the_new_version_in_settings_apps() {
    let work = Work::new();
    let host = work.host();
    work.install(&work.release("0.1.0"), None, true).unwrap();
    let from = work.root();
    let to = work.folder.path().join("moved").join("Aura");
    // The wizard plans the move with the other choices, then installs, then moves.
    let relocation = relocate::plan(&host, &from, &to).unwrap();
    work.install(&work.release("0.1.1"), None, true).unwrap();
    assert_eq!(work.state().pending.as_deref(), Some("0.1.1"));
    relocate::relocate(&host, &relocation, &mut |_, _, _| {}, &|| false, None).unwrap();
    assert!(registered_at(&work, &to));
    assert_eq!(host.entry(&namespace()).unwrap()["DisplayVersion"], "0.1.1");
}

#[test]
fn a_move_that_fails_part_way_leaves_the_installation_where_it_was() {
    for fault in [Fault::CopyDiffers, Fault::RegistrationFails] {
        let work = Work::new();
        let host = work.host();
        work.install(&work.release("0.1.0"), None, false).unwrap();
        let from = work.root();
        let to = work.folder.path().join("moved").join("Aura");
        let relocation = relocate::plan(&host, &from, &to).unwrap();
        let error =
            relocate::relocate(&host, &relocation, &mut |_, _, _| {}, &|| false, Some(fault))
                .unwrap_err();
        assert!(format!("{error:#}").contains("Aura 留在"), "{fault:?}: {error:#}");
        assert!(!to.exists(), "{fault:?}: the copy was left");
        assert!(!to.parent().unwrap().exists(), "{fault:?}: the folder made above it was left");
        let state = read_install_state(&from).unwrap();
        resolve_version(&from, "0.1.0", Some(&state.release_descriptor)).unwrap();
        assert!(registered_at(&work, &from), "{fault:?}: the entry moved");
        let link = fs::read(host.start_menu().unwrap().join(SHORTCUT)).unwrap();
        assert!(names(&link, &from) && !names(&link, &to), "{fault:?}: the shortcut moved");
    }
}

#[test]
fn a_folder_that_cannot_take_the_installation_says_why() {
    let work = Work::new();
    let host = work.host();
    work.install(&work.release("0.1.0"), None, false).unwrap();
    let from = work.root();
    let busy = work.folder.path().join("busy");
    fs::create_dir_all(&busy).unwrap();
    fs::write(busy.join("notes.txt"), b"mine").unwrap();
    let refused = |to: &Path| relocate::plan(&host, &from, to).unwrap_err();
    assert!(refused(&busy).contains(&Refusal::NotEmpty(busy.clone())));
    assert_eq!(refused(&from), vec![Refusal::AlreadyThere]);
    assert!(refused(&from.join("inner")).contains(&Refusal::InInstallation));
    assert!(refused(Path::new(r"relative\Aura")).contains(&Refusal::NotAbsolute));
}
