//! Credential Manager calls made at the same time from several threads of one process and from
//! several processes, through `WindowsCredentialStore`.
//!
//! Every target is unique: it is written, read back, deleted and read again. Afterwards nothing
//! may be left under the case's namespace, and no read may disagree with the call before it.
//! Credential Manager loses updates when such calls overlap - a successful delete comes back
//! later, a successful write reads back as missing - so these cases hold only while every call
//! is ordered by the user's lock. The lock orders Aura's calls only, so a case that finds
//! anything runs once more under a fresh namespace (`at_most_twice`).
use std::{
    process::{Command, Stdio},
    sync::{Arc, Barrier, Mutex},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use aura_domain_common::CredentialRef;
use windows_sys::Win32::{
    Foundation::{ERROR_NOT_FOUND, GetLastError},
    Security::Credentials::{CREDENTIALW, CredEnumerateW, CredFree},
};

use crate::{CredentialStoreError, SecretBytes, WindowsCredentialStore};

const THREADS: usize = 6;
const THREAD_CYCLES: usize = 20;
const PROCESSES: usize = 4;
const PROCESS_CYCLES: usize = 30;
/// How long a lost delete may take to show again once the calls have ended.
const SETTLE: Duration = Duration::from_secs(3);
const WORKER: &str = "credential_lock_tests::credential_cycles_in_a_child_process";
const NAMESPACE_VARIABLE: &str = "AURA_CREDENTIAL_LOCK_TEST_NAMESPACE";
const PROCESS_VARIABLE: &str = "AURA_CREDENTIAL_LOCK_TEST_PROCESS";

#[test]
fn calls_from_several_threads_leave_nothing_behind_and_read_back_what_they_wrote() {
    at_most_twice("threads", |namespace| {
        let wrong = Arc::new(Mutex::new(Vec::new()));
        let start = Arc::new(Barrier::new(THREADS));
        let workers = (0..THREADS)
            .map(|thread| {
                let (namespace, wrong, start) =
                    (namespace.to_owned(), Arc::clone(&wrong), Arc::clone(&start));
                thread::spawn(move || {
                    start.wait();
                    for index in 0..THREAD_CYCLES {
                        let mut found = cycle(&namespace, &format!("t{thread}-{index}"));
                        wrong.lock().expect("findings").append(&mut found);
                    }
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().expect("worker thread");
        }
        let left = left_behind(namespace);
        let wrong = wrong.lock().expect("findings").clone();
        (!wrong.is_empty() || !left.is_empty()).then(|| {
            format!(
                "{} calls disagreed: {wrong:?}; {} of {} deleted credentials are back: {left:?}",
                wrong.len(),
                left.len(),
                THREADS * THREAD_CYCLES
            )
        })
    });
}

#[test]
fn calls_from_several_processes_leave_nothing_behind_and_read_back_what_they_wrote() {
    let program = std::env::current_exe().expect("this test program");
    at_most_twice("processes", |namespace| {
        let children = (0..PROCESSES)
            .map(|process| {
                crate::test_spawn::spawn(
                    Command::new(&program)
                        .args([
                            "--ignored",
                            "--exact",
                            WORKER,
                            "--test-threads",
                            "1",
                            "--nocapture",
                        ])
                        .env(NAMESPACE_VARIABLE, namespace)
                        .env(PROCESS_VARIABLE, process.to_string())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::piped()),
                )
                .expect("a child copy of this test program")
            })
            .collect::<Vec<_>>();
        let mut failed = Vec::new();
        for child in children {
            let output = child.wait_with_output().expect("the child's output");
            let stdout = String::from_utf8_lossy(&output.stdout);
            // A filter that matched nothing would also exit cleanly, having written nothing.
            if !output.status.success() || !stdout.contains("1 passed") {
                let stderr = String::from_utf8_lossy(&output.stderr);
                failed.push(format!("{}\n{stdout}\n{stderr}", output.status));
            }
        }
        let left = left_behind(namespace);
        (!failed.is_empty() || !left.is_empty()).then(|| {
            format!(
                "{} child processes failed: {failed:?}; {} of {} deleted credentials are back: \
                 {left:?}",
                failed.len(),
                left.len(),
                PROCESSES * PROCESS_CYCLES
            )
        })
    });
}

/// Runs a case under a fresh namespace, and once more under another when it found anything; the
/// case fails only when both runs did.
///
/// The lock orders Aura's own calls. Another program writing its credentials while a case runs
/// can still make one of the case's deletes come back: one of 120 thread cycles did, at the
/// moment a non-Aura credential was rewritten. Without the lock 11 and 24 of 120 deletes came
/// back in a single run, so a missing lock still fails both runs.
fn at_most_twice(kind: &str, case: impl Fn(&str) -> Option<String>) {
    let run = || {
        let namespace = namespace(kind);
        let _cleanup = RemoveNamespace(namespace.clone());
        case(&namespace)
    };
    let Some(first) = run() else {
        return;
    };
    println!("{kind}: {first}; the case runs once more under a fresh namespace");
    if let Some(second) = run() {
        panic!("{kind}, both runs found disagreements: {first}; then {second}");
    }
}

/// The child half of the case above: one process, one thread, its own targets.
#[test]
#[ignore = "run in child processes by calls_from_several_processes_leave_nothing_behind_and_read_back_what_they_wrote"]
fn credential_cycles_in_a_child_process() {
    let (Ok(namespace), Ok(process)) =
        (std::env::var(NAMESPACE_VARIABLE), std::env::var(PROCESS_VARIABLE))
    else {
        return;
    };
    let mut wrong = Vec::new();
    for index in 0..PROCESS_CYCLES {
        wrong.append(&mut cycle(&namespace, &format!("p{process}-{index}")));
    }
    assert!(wrong.is_empty(), "{} calls in this process disagreed: {wrong:?}", wrong.len());
}

fn namespace(kind: &str) -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos();
    format!("lock-{kind}-{nanos:x}")
}

/// One write, read, delete and read on its own target, and every disagreement on the way.
fn cycle(namespace: &str, name: &str) -> Vec<String> {
    let reference = CredentialRef::for_test(namespace, name).expect("test reference");
    let value = format!("{namespace}/{name}");
    let store = WindowsCredentialStore;
    let mut wrong = Vec::new();
    if let Err(error) = store.write(&reference, &SecretBytes::new(value.clone().into_bytes())) {
        wrong.push(format!("{name}: write failed: {error}"));
        return wrong;
    }
    match store.read(&reference) {
        Ok(secret) if secret.expose() == value.as_bytes() => {}
        Ok(_) => wrong.push(format!("{name}: read back another value")),
        Err(error) => wrong.push(format!("{name}: read right after the write: {error}")),
    }
    match store.delete(&reference) {
        Ok(true) => {}
        Ok(false) => wrong.push(format!("{name}: the delete found nothing")),
        Err(error) => wrong.push(format!("{name}: delete failed: {error}")),
    }
    match store.read(&reference) {
        Err(CredentialStoreError::NotFound { .. }) => {}
        Ok(_) => wrong.push(format!("{name}: still readable right after the delete")),
        Err(error) => wrong.push(format!("{name}: read after the delete: {error}")),
    }
    wrong
}

/// What is still present under the namespace: right away, and once lost deletes had time to
/// show again.
fn left_behind(namespace: &str) -> Vec<String> {
    let mut left = listed(namespace).expect("list the case's namespace");
    thread::sleep(SETTLE);
    for name in listed(namespace).expect("list the case's namespace again") {
        if !left.contains(&name) {
            left.push(name);
        }
    }
    left.sort();
    left
}

/// The target names under `Aura/test/<namespace>/`, read under the user's lock like every other
/// call; no credential value is read.
fn listed(namespace: &str) -> Result<Vec<String>, u32> {
    let _lock = crate::credential_lock::hold().map_err(|_| u32::MAX)?;
    let filter =
        format!("Aura/test/{namespace}/*").encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut count = 0_u32;
    let mut list: *mut *mut CREDENTIALW = std::ptr::null_mut();
    // SAFETY: the filter is NUL-terminated and both outputs are writable.
    if unsafe { CredEnumerateW(filter.as_ptr(), 0, &mut count, &mut list) } == 0 {
        // SAFETY: read immediately after the failed call.
        let error = unsafe { GetLastError() };
        return if error == ERROR_NOT_FOUND { Ok(Vec::new()) } else { Err(error) };
    }
    let mut names = Vec::new();
    // SAFETY: CredEnumerateW returned `count` credential pointers that live until CredFree.
    for entry in unsafe { std::slice::from_raw_parts(list, count as usize) } {
        // SAFETY: each pointer refers to one returned credential.
        let target = unsafe { (**entry).TargetName };
        if target.is_null() {
            continue;
        }
        // SAFETY: TargetName is NUL-terminated inside the returned allocation.
        let length = (0..).take_while(|&index| unsafe { *target.add(index) } != 0).count();
        // SAFETY: `length` units precede the terminator.
        names.push(String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(target, length) }));
    }
    // SAFETY: the list was allocated by CredEnumerateW and is freed once.
    unsafe { CredFree(list.cast()) };
    Ok(names)
}

/// Deletes whatever the case left under its namespace, on success and on a failed assertion.
struct RemoveNamespace(String);

impl Drop for RemoveNamespace {
    fn drop(&mut self) {
        let Ok(names) = listed(&self.0) else {
            return;
        };
        for name in names {
            let Some(label) = name.rsplit('/').next() else {
                continue;
            };
            if let Ok(reference) = CredentialRef::for_test(&self.0, label) {
                let _ = WindowsCredentialStore.delete(&reference);
            }
        }
    }
}
