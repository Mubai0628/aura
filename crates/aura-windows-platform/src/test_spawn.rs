//! Helper processes for this crate's tests. `std::process::Command` gives the process it starts
//! every handle of this process that is inheritable at that moment. A native launch's
//! standard-stream handles are inheritable from the moment its pipes exist until its child has
//! been created, and a render reads its output until every holder of the write end has closed it:
//! a helper started inside that window keeps the render open until the helper itself ends. Native
//! launches hold `launch_window` for that window, and helpers start outside it.
use std::{
    io,
    process::{Child, Command, ExitStatus, Output, Stdio},
    ptr,
    sync::{PoisonError, RwLock, RwLockReadGuard, mpsc},
    time::Duration,
};

use windows_sys::Win32::System::Pipes::PeekNamedPipe;

use crate::{
    handle::OwnedHandle,
    sandbox::{create_inheritable_pipe, set_non_inheritable},
};

static LAUNCHES: RwLock<()> = RwLock::new(());

/// Held by a native launch while the handles meant for its child are inheritable.
pub(crate) fn launch_window() -> RwLockReadGuard<'static, ()> {
    LAUNCHES.read().unwrap_or_else(PoisonError::into_inner)
}

/// Starts `command` while no native launch has inheritable handles.
pub(crate) fn spawn(command: &mut Command) -> io::Result<Child> {
    let _no_launch = LAUNCHES.write().unwrap_or_else(PoisonError::into_inner);
    command.spawn()
}

/// `Command::output`, started outside every launch window.
pub(crate) fn output(command: &mut Command) -> io::Result<Output> {
    spawn(command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()))?
        .wait_with_output()
}

/// `Command::status`, started outside every launch window.
pub(crate) fn status(command: &mut Command) -> io::Result<ExitStatus> {
    spawn(command)?.wait()
}

fn has_a_writer(read: &OwnedHandle) -> bool {
    let mut available = 0_u32;
    // SAFETY: `read` is a live anonymous-pipe read end; only the byte count is requested. The
    // call fails with a broken pipe once no write end is open anywhere.
    unsafe {
        PeekNamedPipe(
            read.as_raw(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            &mut available,
            ptr::null_mut(),
        ) != 0
    }
}

#[test]
fn a_helper_asked_for_during_a_native_launch_does_not_inherit_its_handles() {
    // A launch's pipe: the end meant for its child is inheritable while the window is held.
    let window = launch_window();
    let (read, write) = create_inheritable_pipe().expect("launch pipe");
    set_non_inheritable(&read).expect("the launch keeps its own end private");
    let (started, helper_started) = mpsc::channel();
    let helper = std::thread::spawn(move || {
        let mut ping = Command::new(r"C:\Windows\System32\PING.EXE");
        ping.args(["-n", "5", "127.0.0.1"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let child = spawn(&mut ping).expect("helper");
        let _ = started.send(());
        child
    });
    // Time for the helper to start if nothing holds it back.
    let _ = helper_started.recv_timeout(Duration::from_secs(1));
    drop(write);
    drop(window);
    let mut child = helper.join().expect("helper thread");
    let held = has_a_writer(&read);
    let _ = child.kill();
    let _ = child.wait();
    assert!(!held, "a helper started during a launch holds the launch's pipe open");
}
