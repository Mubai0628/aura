//! A relay Chrome started from another version folder of this installation can only reach the
//! Core of that version, which does not run while this one does. A relay of an earlier build does
//! not end when its Core goes away: it keeps Chrome's connection, and keeps its folder from being
//! replaced or removed, until Chrome closes. Such relays are stopped when this Core starts, and the
//! extension reconnects to the relay registered now.
use aura_chrome_domain::protocol::RELAY_PROGRAM;
use aura_windows_platform::installer::{programs_running_in, stop_program};
use std::path::Path;

/// Stops the relays running from version folders other than the one `relay` is in, and says how
/// many. Nothing is looked at outside an installation's `versions` folder.
pub(super) fn stop_others(relay: &Path) -> usize {
    let Some(own) = relay.parent() else {
        return 0;
    };
    let Some(versions) = own.parent().filter(|folder| {
        folder.file_name().is_some_and(|name| name.eq_ignore_ascii_case("versions"))
    }) else {
        return 0;
    };
    let Ok(running) = programs_running_in(versions) else {
        return 0;
    };
    let own = lowered(own);
    running
        .iter()
        .filter(|program| {
            program.path.file_name().is_some_and(|name| name.eq_ignore_ascii_case(RELAY_PROGRAM))
                && program.path.parent().is_some_and(|folder| lowered(folder) != own)
        })
        .filter(|program| stop_program(program).is_ok())
        .count()
}

fn lowered(path: &Path) -> String {
    path.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Child, Command};

    /// A program named as the relay, working in its own folder as Chrome starts the relay.
    fn relay_in(folder: &Path) -> Child {
        std::fs::create_dir_all(folder).expect("version folder");
        let program = folder.join(RELAY_PROGRAM);
        std::fs::copy(r"C:\Windows\System32\PING.EXE", &program).expect("copy");
        Command::new(&program)
            .args(["-n", "60", "127.0.0.1"])
            .current_dir(folder)
            .spawn()
            .expect("start")
    }

    #[test]
    fn only_relays_of_other_version_folders_are_stopped() {
        let root = tempfile::tempdir().expect("installation");
        let versions = root.path().join("versions");
        let mut earlier = relay_in(&versions.join("0.1.6"));
        let mut current = relay_in(&versions.join("0.1.7"));
        assert_eq!(stop_others(&versions.join("0.1.7").join(RELAY_PROGRAM)), 1);
        assert!(earlier.try_wait().expect("status").is_some(), "the earlier relay has ended");
        assert!(current.try_wait().expect("status").is_none(), "this version's relay still runs");
        let _ = current.kill();
        let _ = current.wait();
        let _ = earlier.wait();
        std::fs::rename(versions.join("0.1.6"), root.path().join("aside")).expect("released");
    }

    #[test]
    fn a_relay_outside_an_installation_stops_nothing() {
        let root = tempfile::tempdir().expect("folder");
        let mut other = relay_in(&root.path().join("debug"));
        assert_eq!(stop_others(&root.path().join("target").join(RELAY_PROGRAM)), 0);
        assert!(other.try_wait().expect("status").is_none());
        let _ = other.kill();
        let _ = other.wait();
    }
}
