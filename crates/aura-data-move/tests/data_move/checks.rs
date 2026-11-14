//! Checking a folder the user chose before anything is written: every reason it cannot take the
//! data is named, and an accepted folder comes with what the move will copy and the room it needs.
#![allow(clippy::unwrap_used)]
use crate::support;

use std::{
    fs,
    path::{Path, PathBuf},
};

use aura_data_move::{
    Accepted, Choice, Context, MAX_LOCATION_CHARS, Outcome, Part, Refusal, check, room_needed,
};
use support::{FILES, Profile, Run, ask, on_other_drive, production, write};

fn checked(profile: &Profile, choice: Choice) -> Result<Accepted, Vec<Refusal>> {
    let current = profile.current();
    check(
        &Context {
            roots: &profile.roots,
            namespace: &profile.namespace,
            current: &current,
            install_root: None,
            workspaces: &[],
            on_trial: false,
            production: profile.production(),
        },
        &choice,
    )
}

#[test]
fn a_new_folder_on_another_drive_is_accepted_with_what_moves_and_the_room_it_needs() {
    let profile = Profile::new(production());
    let drive = on_other_drive();
    let chosen = drive.path().join("AuraData");
    let accepted = checked(&profile, Choice::Folder(chosen.clone())).unwrap();
    assert_eq!(accepted.destination.as_deref(), Some(chosen.as_path()));
    let data: u64 = FILES.iter().map(|(_, bytes)| bytes.len() as u64).sum();
    let log = b"{\"event\":\"core_started\"}\n".len() as u64;
    let window = (b"{\"window\":1}".len() + b"drafts".len() + b"signed in".len()) as u64;
    let memory = b"- remembered".len() as u64;
    assert_eq!(accepted.plan.bytes, data + log + window + memory, "caches and Chrome stay");
    assert_eq!(accepted.needed, room_needed(accepted.plan.bytes));
    assert!(accepted.free >= accepted.needed);
    assert!(!chosen.exists(), "checking writes nothing");
}

#[test]
fn before_there_is_any_data_a_folder_is_checked_like_any_other_with_no_data_to_copy() {
    let profile = Profile::new(production());
    fs::remove_dir_all(&profile.current().data_root).unwrap();
    let drive = on_other_drive();
    let chosen = drive.path().join("AuraData");
    let accepted = checked(&profile, Choice::Folder(chosen.clone())).unwrap();
    assert_eq!(accepted.destination.as_deref(), Some(chosen.as_path()));
    let data = accepted.plan.items.iter().find(|item| item.part == Part::Data).unwrap();
    assert_eq!((data.bytes, data.files), (0, 0));
}

#[test]
fn room_is_the_data_a_tenth_more_and_a_gigabyte() {
    let gib = 1_u64 << 30;
    assert_eq!(room_needed(10 * gib), 12 * gib);
    assert_eq!(room_needed(0), gib);
}

#[test]
fn a_folder_with_files_in_it_takes_an_aura_folder_inside_which_must_be_empty() {
    let profile = Profile::new(production());
    let drive = on_other_drive();
    let chosen = drive.path().join("Stuff");
    write(&chosen.join("photo.jpg"), b"jpeg");
    let accepted = checked(&profile, Choice::Folder(chosen.clone())).unwrap();
    assert_eq!(accepted.destination, Some(chosen.join("Aura")));

    write(&chosen.join("Aura/old.txt"), b"old");
    assert_eq!(
        checked(&profile, Choice::Folder(chosen.clone())).unwrap_err(),
        [Refusal::NotEmpty(chosen.join("Aura"))]
    );
    fs::remove_file(chosen.join("Aura/old.txt")).unwrap();
    let accepted = checked(&profile, Choice::Folder(chosen.clone())).unwrap();
    assert_eq!(accepted.destination, Some(chosen.join("Aura")), "an empty Aura folder is used");

    let file = drive.path().join("file.txt");
    write(&file, b"text");
    assert_eq!(
        checked(&profile, Choice::Folder(file.clone())).unwrap_err(),
        [Refusal::IsAFile(file)]
    );
    assert_eq!(
        checked(&profile, Choice::Folder(PathBuf::from("relative\\folder"))).unwrap_err(),
        [Refusal::NotAbsolute]
    );
}

#[test]
fn every_reason_a_folder_cannot_take_the_data_is_named_at_once() {
    let profile = Profile::new(production());
    let install = profile.here("Programs/Aura");
    let workspace = profile.here("Projects/site");
    fs::create_dir_all(&install).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    let current = profile.current();
    let inside_install = install.join("data");
    let context = Context {
        roots: &profile.roots,
        namespace: &profile.namespace,
        current: &current,
        install_root: Some(&install),
        workspaces: std::slice::from_ref(&workspace),
        on_trial: true,
        production: true,
    };
    let refusals = check(&context, &Choice::Folder(inside_install.clone())).unwrap_err();
    assert_eq!(
        refusals,
        and_length(&inside_install, vec![Refusal::OnTrial, Refusal::InInstallation])
    );

    let holding = profile.here("Projects-new");
    let context = Context { workspaces: &[holding.join("app")], on_trial: false, ..context };
    assert_eq!(
        check(&context, &Choice::Folder(holding.clone())).unwrap_err(),
        and_length(&holding, vec![Refusal::HoldsWorkspace(holding.join("app"))])
    );
    let in_workspace = workspace.join("aura");
    assert_eq!(
        check(
            &Context { workspaces: std::slice::from_ref(&workspace), ..context },
            &Choice::Folder(in_workspace.clone())
        )
        .unwrap_err(),
        and_length(&in_workspace, vec![Refusal::InWorkspace(workspace.clone())])
    );
    assert_eq!(
        check(
            &Context { workspaces: &[], ..context },
            &Choice::Folder(install.parent().unwrap().to_path_buf())
        )
        .unwrap_err(),
        and_length(&install, vec![Refusal::InInstallation]),
        "a folder that holds files takes an Aura folder inside, here the installation's"
    );
    let gone = profile.here("Gone");
    let installed = gone.join("Aura");
    assert_eq!(
        check(
            &Context { workspaces: &[], install_root: Some(&installed), ..context },
            &Choice::Folder(gone.clone())
        )
        .unwrap_err(),
        and_length(&gone, vec![Refusal::HoldsInstallation])
    );
}

/// `reasons`, then the length of `destination` when it is over the limit: the profile lives in
/// TEMP, whose path is longer under a long user name or a test runner's own TEMP folder.
fn and_length(destination: &Path, mut reasons: Vec<Refusal>) -> Vec<Refusal> {
    let length = destination.as_os_str().len();
    if length > MAX_LOCATION_CHARS {
        reasons.push(Refusal::TooLong { length, most: MAX_LOCATION_CHARS });
    }
    reasons
}

#[test]
fn the_data_aura_s_own_folders_and_sync_folders_are_refused() {
    let profile = Profile::new(production());
    let current = profile.current();
    let refusals =
        checked(&profile, Choice::Folder(current.data_root.join("elsewhere"))).unwrap_err();
    assert!(refusals.contains(&Refusal::InData), "{refusals:?}");
    let refusals =
        checked(&profile, Choice::Folder(profile.roots.local.join("Aura").join("namespaces")))
            .unwrap_err();
    assert!(refusals.contains(&Refusal::InData), "inside %LOCALAPPDATA%\\Aura: {refusals:?}");
    let synced = profile.here("Dropbox");
    write(&synced.join(".dropbox"), b"{}");
    assert_eq!(
        checked(&profile, Choice::Folder(synced.join("Aura"))).unwrap_err(),
        [Refusal::SyncFolder("Dropbox".to_owned())]
    );
}

#[test]
fn a_location_longer_than_the_deepest_data_allows_is_refused_with_the_limit() {
    let profile = Profile::new(production());
    let drive = on_other_drive();
    let base = drive.path().as_os_str().len();
    let longest = drive.path().join("d".repeat(MAX_LOCATION_CHARS - base - 1));
    assert!(checked(&profile, Choice::Folder(longest)).is_ok());
    let too_long = drive.path().join("d".repeat(MAX_LOCATION_CHARS - base));
    assert_eq!(
        checked(&profile, Choice::Folder(too_long)).unwrap_err(),
        [Refusal::TooLong { length: MAX_LOCATION_CHARS + 1, most: MAX_LOCATION_CHARS }]
    );
    assert_eq!(
        Refusal::TooLong { length: 81, most: 79 }.to_string(),
        "路径有 81 个字符，最多 79 个"
    );

    // Data deeper than Aura writes lowers the limit for this move.
    write(&profile.default_place().data_root.join(format!("deeper/{}", "x".repeat(190))), b"x");
    let chosen = drive.path().join("d".repeat(60 - base - 1));
    let refusals = checked(&profile, Choice::Folder(chosen)).unwrap_err();
    assert_eq!(refusals, [Refusal::TooLong { length: 60, most: 259 - 1 - 5 - 197 }]);
}

#[test]
fn the_current_place_is_already_there_and_moving_back_waits_for_the_old_copy_to_go() {
    let profile = Profile::new(production());
    assert_eq!(checked(&profile, Choice::DefaultPlace).unwrap_err(), [Refusal::AlreadyThere]);
    let drive = on_other_drive();
    let destination = drive.path().join("AuraData");
    ask(&profile, Some(&destination));
    assert!(matches!(Run::default().perform(&profile, None), Outcome::Moved { .. }));
    assert_eq!(
        checked(&profile, Choice::Folder(destination.clone())).unwrap_err(),
        [Refusal::AlreadyThere]
    );
    let refusals = checked(&profile, Choice::DefaultPlace).unwrap_err();
    assert!(!refusals.is_empty());
    assert!(
        refusals.iter().all(|refusal| matches!(refusal, Refusal::DefaultTaken(_))),
        "{refusals:?}"
    );
    let fixed = profile.default_place().fixed_root;
    assert!(refusals.contains(&Refusal::DefaultTaken(fixed.join("data"))));
    assert!(!refusals.contains(&Refusal::DefaultTaken(fixed.join("chrome"))));

    aura_data_move::remove_old_copies(&profile.roots, &profile.namespace, &profile.current())
        .unwrap();
    let accepted = checked(&profile, Choice::DefaultPlace).unwrap();
    assert_eq!(accepted.destination, None);
}

#[test]
fn every_refusal_says_what_is_wrong_in_words() {
    let said = [
        (Refusal::OnTrial, "更新还在试用，确认能用后再搬"),
        (Refusal::AlreadyThere, "数据已经在这里"),
        (Refusal::SyncFolder("OneDrive".to_owned()), "在 OneDrive 的同步文件夹里"),
        // One the cloud files API knows, from a client without a name of its own here.
        (Refusal::SyncFolder(String::new()), "在同步文件夹里"),
        (
            Refusal::NoRoom { needed: 3 << 30, free: 1 << 30 },
            "空间不够：需要约 3072 MB，这个磁盘只剩 1024 MB",
        ),
        (
            Refusal::NotFixedDisk(aura_windows_platform::DriveKind::Removable),
            "这是U 盘或移动硬盘，只能选本机的固定磁盘",
        ),
    ];
    for (refusal, words) in said {
        assert_eq!(refusal.to_string(), words);
    }
}
