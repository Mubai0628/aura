//! A version folder that a program still works in cannot be moved aside. Chrome starts Aura's
//! relay that way, and a relay of an earlier build stayed running after its Core was gone: going
//! back to an earlier version and updating again then failed for good.
use super::*;
use aura_chrome_domain::protocol::RELAY_PROGRAM;

impl Running {
    /// A program named as Chrome's relay, working in `folder` as Chrome starts the relay.
    fn relay_in(folder: &Path) -> Self {
        fs::create_dir_all(folder).unwrap();
        let program = folder.join(RELAY_PROGRAM);
        fs::copy(r"C:\Windows\System32\PING.EXE", &program).unwrap();
        Self(
            std::process::Command::new(program)
                .args(["-n", "60", "127.0.0.1"])
                .current_dir(folder)
                .spawn()
                .unwrap(),
        )
    }

    /// Some other program working in `folder`, as a terminal opened there does.
    fn working_in(folder: &Path) -> Self {
        Self(
            std::process::Command::new(r"C:\Windows\System32\PING.EXE")
                .args(["-n", "60", "127.0.0.1"])
                .current_dir(folder)
                .spawn()
                .unwrap(),
        )
    }

    fn ended(&mut self) -> bool {
        self.0.try_wait().unwrap().is_some()
    }
}

/// 0.1.0 installed and 0.1.1 applied once, so that 0.1.1's folder is there to be replaced.
fn with_version_folder() -> (Work, PathBuf) {
    let work = Work::new();
    work.install(&work.release("0.1.0"), None, false).unwrap();
    work.release("0.1.1");
    let package = work.folder.path().join("Aura-Update-0.1.1.aura-update");
    install::apply_update(&work.host(), &work.stub(), &work.root(), &package).unwrap();
    (work, package)
}

#[test]
fn an_update_replaces_a_version_folder_a_relay_was_left_running_in() {
    let (work, package) = with_version_folder();
    let mut relay = Running::relay_in(&work.root().join("versions").join("0.1.1"));
    install::apply_update(&work.host(), &work.stub(), &work.root(), &package).unwrap();
    assert!(relay.ended(), "the relay left running was stopped");
    aura_launcher::resolve_version(&work.root(), "0.1.1", None).unwrap();
    assert_eq!(work.state().pending.as_deref(), Some("0.1.1"));
}

#[test]
fn a_folder_another_program_works_in_is_named_and_the_package_is_kept_usable() {
    let (work, package) = with_version_folder();
    let folder = work.root().join("versions").join("0.1.1");
    let mut holder = Running::working_in(&folder);
    let Err(error) = install::apply_update(&work.host(), &work.stub(), &work.root(), &package)
    else {
        panic!("installed although the folder is held");
    };
    assert!(error.is::<install::FolderInUse>(), "{error:#}");
    assert!(format!("{error:#}").contains(&folder.display().to_string()), "{error:#}");
    assert!(!holder.ended(), "a program that is not Aura's is never stopped");
    drop(holder);
    install::apply_update(&work.host(), &work.stub(), &work.root(), &package)
        .expect("the same package installs once the folder is free");
}

#[test]
fn a_relay_alone_is_not_aura_running_but_does_not_hide_aura_running() {
    let work = Work::new();
    let setup = work.release("0.1.0");
    work.install(&setup, None, false).unwrap();
    let version = work.root().join("versions").join("0.1.0");
    let mut relay = Running::relay_in(&version);
    work.install(&setup, None, false).expect("a repair with only a relay left running");
    assert!(relay.ended(), "the relay was stopped");
    let mut relay = Running::relay_in(&version);
    let aura = Running::start(&work.root());
    assert!(work.install(&setup, None, false).is_err(), "refused while Aura runs");
    assert!(!relay.ended(), "nothing is stopped when Aura itself runs");
    drop(aura);
}
