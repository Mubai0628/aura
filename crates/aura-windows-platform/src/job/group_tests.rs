use super::*;
use std::time::Duration;

const GIB: u64 = 1024 * 1024 * 1024;

fn waiting_cmd(
    groups: &[&GroupJob],
    job: &RootJob,
    directory: &Path,
) -> (crate::NativeSandboxProcess, ProcessIdentity) {
    // A native CMD that waits on its own standard input; no file or network action.
    let arguments = ["/d", "/c", "set", "/p", "AURA_GROUP_FIXTURE="]
        .map(|argument| argument.encode_utf16().collect::<Vec<_>>());
    let executable = crate::windows_directory().expect("Windows").join("System32").join("cmd.exe");
    let (process, timing) =
        crate::spawn_compat_process_in(groups, job, &executable, &arguments, directory, &[], None)
            .expect("CMD nested in its groups");
    let identity = timing.root_identity.expect("created root identity");
    (process, identity)
}

#[test]
fn a_group_holds_the_cpu_and_memory_caps_it_was_given() {
    let capped = GroupJob::create(GroupLimits { cpu_rate: 5_000, memory_bytes: Some(2 * GIB) })
        .expect("group with both caps");
    assert_eq!(
        capped.limits().expect("read back"),
        GroupLimits { cpu_rate: 5_000, memory_bytes: Some(2 * GIB) }
    );
    let cpu_only =
        GroupJob::create(GroupLimits { cpu_rate: 7_500, memory_bytes: None }).expect("CPU only");
    assert_eq!(
        cpu_only.limits().expect("read back"),
        GroupLimits { cpu_rate: 7_500, memory_bytes: None }
    );
    for rate in [0, 10_001] {
        assert!(GroupJob::create(GroupLimits { cpu_rate: rate, memory_bytes: None }).is_err());
    }
}

#[test]
fn a_groups_cpu_cap_can_be_changed_within_its_range() {
    let group = GroupJob::create(GroupLimits { cpu_rate: 7_500, memory_bytes: Some(2 * GIB) })
        .expect("group");
    group.set_cpu_rate(2_500).expect("a lower cap");
    assert_eq!(
        group.limits().expect("read back"),
        GroupLimits { cpu_rate: 2_500, memory_bytes: Some(2 * GIB) },
        "the memory cap stays"
    );
    group.set_cpu_rate(10_000).expect("all processors");
    assert_eq!(group.limits().expect("read back").cpu_rate, 10_000);
    for rate in [0, 10_001] {
        assert!(group.set_cpu_rate(rate).is_err());
    }
    assert_eq!(group.limits().expect("read back").cpu_rate, 10_000);
}

#[test]
fn two_groups_keep_their_processes_apart_and_stopping_one_job_leaves_the_other() {
    let directory = tempfile::tempdir().expect("owned working directory");
    let all = GroupJob::create(GroupLimits { cpu_rate: 7_500, memory_bytes: None }).expect("all");
    let limits = GroupLimits { cpu_rate: 5_000, memory_bytes: Some(4 * GIB) };
    let first = GroupJob::create(limits).expect("first group");
    let second = GroupJob::create(limits).expect("second group");
    let first_job = RootJob::create().expect("first execution Job");
    let second_job = RootJob::create().expect("second execution Job");

    let (first_process, first_identity) =
        waiting_cmd(&[&all, &first], &first_job, directory.path());
    let (mut second_process, second_identity) =
        waiting_cmd(&[&all, &second], &second_job, directory.path());

    assert!(first_job.contains_identity(&first_identity).expect("own Job"));
    assert!(first.contains_identity(&first_identity).expect("own group"));
    assert!(!second.contains_identity(&first_identity).expect("other group"));
    assert!(second.contains_identity(&second_identity).expect("own group"));
    assert!(!first.contains_identity(&second_identity).expect("other group"));
    assert!(all.contains_identity(&first_identity).expect("all groups"));
    assert!(all.contains_identity(&second_identity).expect("all groups"));
    // A console program's own host process joins its Job too.
    assert!(first.active_processes().expect("first") >= 1);
    assert!(second.active_processes().expect("second") >= 1);
    assert!(all.active_processes().expect("both") >= 2);

    first_process.terminate(1).expect("stop the first Job's tree");
    assert!(
        first_process.wait_for_process_tree_exit(Duration::from_secs(10)).expect("observe"),
        "the first Job's tree did not end"
    );
    assert_eq!(first.active_processes().expect("first emptied"), 0);
    assert!(second.active_processes().expect("second untouched") >= 1);
    assert!(all.active_processes().expect("the second's left") >= 1);
    assert_eq!(second_process.wait_timeout(Duration::ZERO).expect("still waiting"), None);

    second_process.close_stdin();
    assert!(
        second_process.wait_for_process_tree_exit(Duration::from_secs(10)).expect("observe"),
        "the second CMD did not end at the end of its input"
    );
    assert_eq!(all.active_processes().expect("all emptied"), 0);
}
