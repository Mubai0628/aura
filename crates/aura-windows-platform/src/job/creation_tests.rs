//! Test-only observation before adoption/resume; never creates accounts or AppContainers.
use super::*;
use std::cell::RefCell;
use windows_sys::Win32::{Foundation::HANDLE, System::JobObjects::IsProcessInJob};

thread_local! {
    static OBSERVATIONS: RefCell<Option<Vec<(bool, bool)>>> = const { RefCell::new(None) };
}

pub(crate) fn inspect_created(job: HANDLE, process: HANDLE) -> io::Result<()> {
    let armed = OBSERVATIONS.with(|slot| slot.borrow().is_some());
    if !armed {
        return Ok(());
    }
    let mut contained = 0;
    // SAFETY: caller retains the exact created process and target Job, before resume.
    let queried = unsafe { IsProcessInJob(process, job, &mut contained) } != 0;
    // Fault injection owns cleanup even if the observation fails; never orphan a suspended root.
    let stopped = unsafe { TerminateProcess(process, 1) } != 0
        && unsafe { WaitForSingleObject(process, 5_000) } == WAIT_OBJECT_0;
    OBSERVATIONS.with(|slot| {
        if let Some(values) = slot.borrow_mut().as_mut() {
            values.push((queried && contained != 0, stopped));
        }
    });
    Err(io::Error::other("test-only crash immediately after process creation"))
}

pub(crate) fn observe_creation<T>(launch: impl FnOnce() -> T) -> (T, Vec<(bool, bool)>) {
    OBSERVATIONS.with(|slot| *slot.borrow_mut() = Some(Vec::new()));
    let result = launch();
    let values = OBSERVATIONS.with(|slot| slot.borrow_mut().take().expect("armed observer"));
    (result, values)
}

#[test]
fn desktop_root_is_in_job_even_if_parent_fails_before_adoption() {
    let root = tempfile::tempdir().expect("temporary desktop launch");
    let job = RootJob::create().expect("owned root Job");
    let args = ["/d".to_owned(), "/c".to_owned(), "echo forbidden > ran".to_owned()];
    let (result, observations) = observe_creation(|| {
        job.spawn_assigned(Path::new(r"C:\Windows\System32\cmd.exe"), &args, root.path())
    });
    assert!(result.is_err(), "injected pre-adoption failure");
    assert_eq!(observations, [(true, true)], "root must already belong to Job before adoption");
    assert!(!root.path().join("ran").exists(), "root must never execute");
}

#[test]
fn component_root_is_in_job_even_if_parent_fails_before_secret_delivery() {
    let root = tempfile::tempdir().expect("temporary component launch");
    let job = ComponentJob::create().expect("owned component Job");
    let args = ["/d".to_owned(), "/c".to_owned(), "echo forbidden > ran".to_owned()];
    let (result, observations) = observe_creation(|| {
        job.spawn_sidecar(
            Path::new(r"C:\Windows\System32\cmd.exe"),
            &args,
            root.path(),
            &[0x52; 32],
        )
    });
    assert!(result.is_err(), "injected pre-secret failure");
    assert_eq!(
        observations,
        [(true, true)],
        "component must already belong to Job before bootstrap"
    );
    assert!(!root.path().join("ran").exists());
}
