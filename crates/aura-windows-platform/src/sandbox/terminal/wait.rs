//! One blocking process wait. An owned manual-reset event wakes close/error paths.
use super::*;
use windows_sys::Win32::System::Threading::{
    CreateEventW, INFINITE, SetEvent, WaitForMultipleObjects,
};

pub(super) struct TerminalWake(OwnedHandle);
impl TerminalWake {
    pub(super) fn new() -> io::Result<Self> {
        // SAFETY: unnamed, non-inheritable event owned for the terminal lifetime.
        Ok(Self(OwnedHandle::from_raw(unsafe { CreateEventW(ptr::null(), 1, 0, ptr::null()) })?))
    }
    pub(super) fn signal(&self) -> io::Result<()> {
        if unsafe { SetEvent(self.0.as_raw()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    pub(super) fn wait(&self, process: &OwnedHandle) -> io::Result<bool> {
        let handles = [process.as_raw(), self.0.as_raw()];
        // SAFETY: both handles remain owned throughout the wait.
        match unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, INFINITE) } {
            WAIT_OBJECT_0 => Ok(true),
            value if value == WAIT_OBJECT_0 + 1 => Ok(false),
            _ => Err(io::Error::last_os_error()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocking_terminal_wait_wakes_on_close_event_and_process_exit() {
        for close in [true, false] {
            let wake = Arc::new(TerminalWake::new().expect("close event"));
            let process = Arc::new(TerminalWake::new().expect("process signal fixture"));
            let waiting = Arc::clone(&wake);
            let awaited = Arc::clone(&process);
            let (sender, receiver) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                sender.send(waiting.wait(&awaited.0)).expect("wait receiver alive")
            });
            assert!(matches!(
                receiver.recv_timeout(Duration::from_millis(20)),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout)
            ));
            if close {
                wake.signal().expect("close signal");
            } else {
                process.signal().expect("process exit signal");
            }
            assert_eq!(
                receiver
                    .recv_timeout(Duration::from_secs(2))
                    .expect("wait wakes within deadline")
                    .expect("native wait succeeds"),
                !close
            );
            worker.join().expect("waiter completed");
        }
    }
}
