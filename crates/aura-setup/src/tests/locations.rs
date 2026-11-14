//! Where a new installation and its data go: a folder chosen for the program is private and
//! marked; one that cannot take it says why; a data folder holding this installation's data is
//! taken as it is; before there is any data another folder is where it starts, and data there is
//! moved by the launcher; none moves while an update waits.
use std::fs;

use aura_data_location::{DataMarker, InstallCopy, Locations, write_marker};

use super::{Work, default_locations, namespace};
use crate::{
    data_choice::{self, DataChoice},
    host::Host,
    marker,
};

#[test]
fn a_chosen_install_folder_is_private_and_marked() {
    let work = Work::new();
    let root = work.folder.path().join("chosen").join("Aura");
    let plan = work.install(&work.release("0.1.0"), Some(root.clone()), false).unwrap();
    assert_eq!(plan.root, root);
    assert_eq!(aura_windows_platform::private_folder_problem(&root).unwrap(), None);
    assert!(root.join(marker::INSTALL_MARKER).is_file());
    assert!(marker::is_installation_of(&root, &namespace()));
}

#[test]
fn an_install_folder_inside_the_data_is_refused_and_nothing_is_made() {
    let work = Work::new();
    let host = work.host();
    let inside = default_locations(&host).data_root.join("Aura");
    let error = work.install(&work.release("0.1.0"), Some(inside.clone()), false).unwrap_err();
    assert!(error.to_string().contains("在 Aura 的数据文件夹里"), "{error:#}");
    assert!(!inside.exists());
}

#[test]
fn a_new_installation_takes_a_folder_holding_its_data_as_it_is() {
    let work = Work::new();
    let host = work.host();
    let kept = work.folder.path().join("kept-data");
    fs::create_dir_all(kept.join("data")).unwrap();
    write_marker(&kept, &DataMarker::new(namespace())).unwrap();
    let locations = data_choice::locations(&host, &namespace(), None).unwrap();
    let choice =
        data_choice::choose(&host, &namespace(), &locations, &kept, &work.root(), true, false)
            .unwrap();
    assert_eq!(choice, DataChoice::Reuse(kept.clone()));
    work.install(&work.release("0.1.0"), None, false).unwrap();
    data_choice::apply(&host, &work.root(), &namespace(), &choice).unwrap();
    let state = work.state();
    assert_eq!(state.data_location.as_deref(), kept.to_str());
    let resolved =
        Locations::resolve(&host.roots().unwrap(), &namespace(), &InstallCopy::of(&state)).unwrap();
    assert_eq!(resolved.location.as_deref(), Some(kept.as_path()));
}

#[test]
fn before_there_is_any_data_the_chosen_data_folder_is_where_it_starts() {
    let work = Work::new();
    let host = work.host();
    let picked = work.folder.path().join("first-data");
    let locations = data_choice::locations(&host, &namespace(), None).unwrap();
    let choice =
        data_choice::choose(&host, &namespace(), &locations, &picked, &work.root(), true, false)
            .unwrap();
    assert_eq!(choice, DataChoice::Start(picked.clone()));
    work.install(&work.release("0.1.0"), None, false).unwrap();
    data_choice::apply(&host, &work.root(), &namespace(), &choice).unwrap();
    assert_eq!(aura_windows_platform::private_folder_problem(&picked).unwrap(), None);
    let state = work.state();
    let resolved =
        Locations::resolve(&host.roots().unwrap(), &namespace(), &InstallCopy::of(&state)).unwrap();
    assert_eq!(resolved.location.as_deref(), Some(picked.as_path()));
    let now = data_choice::locations(&host, &namespace(), Some(&state)).unwrap();
    assert_eq!(aura_data_move::requested(&namespace(), &now).unwrap(), None);
}

#[test]
fn data_kept_from_before_is_moved_by_the_launcher_at_the_first_start() {
    let work = Work::new();
    let host = work.host();
    let kept = default_locations(&host).data_root;
    fs::create_dir_all(&kept).unwrap();
    fs::write(kept.join("aura-v2.db"), b"conversations").unwrap();
    let picked = work.folder.path().join("new-data");
    let locations = data_choice::locations(&host, &namespace(), None).unwrap();
    let choice =
        data_choice::choose(&host, &namespace(), &locations, &picked, &work.root(), true, false)
            .unwrap();
    assert_eq!(choice, DataChoice::Move(picked.clone()));
    work.install(&work.release("0.1.0"), None, false).unwrap();
    data_choice::apply(&host, &work.root(), &namespace(), &choice).unwrap();
    let now = data_choice::locations(&host, &namespace(), Some(&work.state())).unwrap();
    assert_eq!(aura_data_move::requested(&namespace(), &now).unwrap(), Some(Some(picked)));
}

#[test]
fn the_data_does_not_move_while_an_update_waits_as_a_trial() {
    let work = Work::new();
    let host = work.host();
    work.install(&work.release("0.1.0"), None, false).unwrap();
    let locations = data_choice::locations(&host, &namespace(), Some(&work.state())).unwrap();
    let picked = work.folder.path().join("later");
    let reasons =
        data_choice::choose(&host, &namespace(), &locations, &picked, &work.root(), false, true)
            .unwrap_err();
    assert!(reasons.iter().any(|reason| reason.contains("更新还在试用")), "{reasons:?}");
}
