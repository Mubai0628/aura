use super::*;
use crate::compat::CompatBackend;
use aura_windows_user_contract::{ManagedShell, ManagedShellCommand};
use std::{sync::atomic::AtomicBool, time::Duration};

const GIB: u64 = 1024 * 1024 * 1024;

fn policy(group: &str, temp: &Path) -> ProcessPolicy {
    ProcessPolicy {
        group: group.into(),
        limits: GroupLimits { cpu_rate: 5_000, memory_bytes: Some(2 * GIB) },
        all_cpu_rate: 7_500,
        temp_directory: temp.join(group),
    }
}

fn backend() -> CompatBackend {
    // A worktree has no managed runtime of its own; the variable names the checkout's.
    let root = std::env::var_os("AURA_MANAGED_SHELL_ROOT").map_or_else(
        || {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/managed-runtimes/powershell-7.6.5-win-x64")
        },
        PathBuf::from,
    );
    CompatBackend::new(&root.join("payload"), &root.join("manifest.json"))
        .expect("verified local managed PS7 and system CMD candidate")
}

fn powershell(directory: &Path, script: &str) -> ManagedShellCommand {
    ManagedShellCommand {
        shell: ManagedShell::PowerShell,
        script: script.into(),
        working_directory: directory.display().to_string(),
        stdin: String::new(),
        timeout_ms: 60_000,
        max_output_bytes: 4096,
        secret_environment: Vec::new(),
    }
}

#[test]
fn a_group_is_kept_per_key_and_each_group_has_its_own_ports() {
    let temp = tempfile::tempdir().expect("owned temp root");
    let groups = ProcessGroups::default();
    let (all, first) = groups.enter(&policy("first", temp.path())).expect("first group");
    let (again_all, again) = groups.enter(&policy("first", temp.path())).expect("same group");
    assert!(Arc::ptr_eq(&first, &again) && Arc::ptr_eq(&all, &again_all));
    let (_, second) = groups.enter(&policy("second", temp.path())).expect("second group");
    assert!(!Arc::ptr_eq(&first, &second));
    let (one, two) = (first.ports(), second.ports());
    assert!(one.end() < two.start() || two.end() < one.start(), "{one:?} and {two:?} overlap");
    assert!(*one.end() < 49_152 && *two.end() < 49_152, "below the dynamic port range");
    assert_eq!(first.job().limits().expect("limits"), policy("first", temp.path()).limits);
    assert_eq!(all.limits().expect("all").cpu_rate, 7_500);
}

#[test]
fn the_cap_on_every_group_together_follows_the_newest_policy() {
    let temp = tempfile::tempdir().expect("owned temp root");
    let groups = ProcessGroups::default();
    let (all, first) = groups.enter(&policy("first", temp.path())).expect("first group");
    assert_eq!(all.limits().expect("all").cpu_rate, 7_500);
    // The user lowered the share: the next command's policy carries it.
    let lower = ProcessPolicy { all_cpu_rate: 2_500, ..policy("first", temp.path()) };
    let (again, same) = groups.enter(&lower).expect("same group");
    assert!(Arc::ptr_eq(&all, &again) && Arc::ptr_eq(&first, &same), "the same Jobs");
    assert_eq!(all.limits().expect("all").cpu_rate, 2_500);
    assert_eq!(first.job().limits().expect("own").cpu_rate, 5_000, "its own cap stays");
}

#[test]
fn a_group_command_gets_its_own_temp_and_ports_and_keeps_the_users_port() {
    let entry = |name: &str, value: &str| NativeEnvironmentEntry {
        name: name.into(),
        value_utf16: value.encode_utf16().collect(),
    };
    let value = |entries: &[NativeEnvironmentEntry], name: &str| {
        entries
            .iter()
            .find(|entry| entry.name.eq_ignore_ascii_case(name))
            .map(|entry| String::from_utf16_lossy(&entry.value_utf16))
    };
    let temp = Path::new(r"C:\Users\someone\AppData\Local\Temp\aura-agents\a");
    let mut users = vec![entry("Temp", r"C:\old"), entry("Port", "8080")];
    surround(&mut users, temp, &(30_100..=30_199)).expect("surrounded");
    assert_eq!(value(&users, "TEMP").as_deref(), Some(temp.to_str().expect("text")));
    assert_eq!(value(&users, "TMP").as_deref(), Some(temp.to_str().expect("text")));
    assert_eq!(value(&users, "AURA_PORT_RANGE").as_deref(), Some("30100-30199"));
    assert_eq!(value(&users, "PORT").as_deref(), Some("8080"), "the user's own PORT stays");
    let mut plain = vec![entry("PATH", r"C:\Windows")];
    surround(&mut plain, temp, &(30_100..=30_199)).expect("surrounded");
    assert_eq!(value(&plain, "PORT").as_deref(), Some("30100"));
}

#[test]
fn two_groups_build_the_same_file_in_their_own_copies_at_once() {
    let _serial = crate::checked_program::TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let backend = backend();
    let temp = tempfile::tempdir().expect("owned temp root");
    let copies = [("first", "alpha"), ("second", "beta")].map(|(group, text)| {
        let copy = tempfile::tempdir().expect("owned copy");
        std::fs::write(copy.path().join("lib.txt"), text).expect("the same file, changed");
        (group, copy)
    });
    // Both "builds" run at once: each waits, then turns its copy's file into its output and
    // writes the TEMP and port range it saw.
    let script = "Start-Sleep -Milliseconds 1500; \
        Get-Content -Path lib.txt | Set-Content -Path build.out -NoNewline; \
        Set-Content -Path temp.out -Value $env:TEMP -NoNewline; \
        Set-Content -Path ports.out -Value $env:AURA_PORT_RANGE -NoNewline";
    let mut running = Vec::new();
    for (group, copy) in &copies {
        let policy = policy(group, temp.path());
        let execution = backend
            .launch_in(&powershell(copy.path(), script), None, Some(&policy))
            .expect("launch in the group");
        running.push((execution, policy));
    }
    for (group, _) in &copies {
        let group = backend.groups.group(group).expect("group entered");
        // A console program's own host process joins its Job too.
        assert!(group.job().active_processes().expect("live") >= 1, "both build at once");
    }
    let threads: Vec<_> = running
        .into_iter()
        .map(|(mut execution, policy)| {
            std::thread::spawn(move || {
                let result = execution.run_direct(&AtomicBool::new(false)).expect("ran");
                let output =
                    String::from_utf8_lossy(&[result.stdout, result.stderr].concat()).into_owned();
                (result.exit_code, output, policy)
            })
        })
        .collect();
    let mut ports = Vec::new();
    for ((_, copy), thread) in copies.iter().zip(threads) {
        let (exit, output, policy) = thread.join().expect("build thread");
        assert_eq!(exit, Some(0), "{output}");
        let read = |name: &str| std::fs::read_to_string(copy.path().join(name)).expect(name);
        let own = std::fs::read_to_string(copy.path().join("lib.txt")).expect("source");
        assert_eq!(read("build.out"), own, "each build read its own copy");
        assert_eq!(Path::new(&read("temp.out")), policy.temp_directory);
        assert!(policy.temp_directory.is_dir(), "the group's TEMP exists");
        ports.push(read("ports.out"));
    }
    assert_ne!(ports[0], ports[1], "each group has its own port range");
}

#[test]
fn stopping_one_groups_command_leaves_the_other_groups_running() {
    let _serial = crate::checked_program::TEST_SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let backend = backend();
    let temp = tempfile::tempdir().expect("owned temp root");
    let work = tempfile::tempdir().expect("owned working directory");
    let wait = powershell(work.path(), "Start-Sleep -Seconds 30");
    let first_policy = policy("first", temp.path());
    let second_policy = policy("second", temp.path());
    let mut first = backend.launch_in(&wait, None, Some(&first_policy)).expect("first");
    let mut second = backend.launch_in(&wait, None, Some(&second_policy)).expect("second");
    let first_group = backend.groups.group("first").expect("first group");
    let second_group = backend.groups.group("second").expect("second group");
    assert!(first_group.job().active_processes().expect("live") >= 1);
    assert!(second_group.job().active_processes().expect("live") >= 1);

    assert!(first.stop(Duration::from_secs(10)).confirmed, "the first command's tree ended");
    assert_eq!(first_group.job().active_processes().expect("emptied"), 0);
    assert!(second_group.job().active_processes().expect("untouched") >= 1);
    assert!(second.active_processes().expect("still running") > 0);

    assert!(second.stop(Duration::from_secs(10)).confirmed);
    assert_eq!(second_group.job().active_processes().expect("emptied"), 0);
}
