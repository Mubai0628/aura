//! The output pump remains alive through ClosePseudoConsole, including failed launches.
use super::*;
use std::{
    io::Write,
    sync::{Arc, Mutex},
    thread::JoinHandle,
};
use windows_sys::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, HPCON, ResizePseudoConsole,
};

struct PseudoConsole(HPCON);
impl Drop for PseudoConsole {
    fn drop(&mut self) {
        // SAFETY: unique console ownership; output is being drained on another thread.
        unsafe { ClosePseudoConsole(self.0) };
    }
}
struct State {
    console: Option<PseudoConsole>,
    creation_peers: Option<(OwnedHandle, OwnedHandle)>,
    size: TerminalSize,
}
pub(super) struct ConsoleSession {
    state: Mutex<State>,
    input: Mutex<Option<NativeProcessStdin>>,
    output: Mutex<Option<JoinHandle<io::Result<()>>>>,
    output_error: Arc<Mutex<Option<String>>>,
}
impl ConsoleSession {
    pub(super) fn create(
        size: TerminalSize,
        sink: TerminalOutputCallback,
        wake: Arc<wait::TerminalWake>,
    ) -> io::Result<Self> {
        let coord = size.coord()?;
        #[cfg(test)]
        let launch_window = crate::test_spawn::launch_window();
        let (input_read, input_write) = create_inheritable_pipe()?;
        let (output_read, output_write) = create_inheritable_pipe()?;
        for handle in [&input_read, &input_write, &output_read, &output_write] {
            set_non_inheritable(handle)?;
        }
        #[cfg(test)]
        drop(launch_window);
        let mut raw = 0;
        // SAFETY: synchronous pipe ends, positive dimensions; no inherited-cursor query.
        let result = unsafe {
            CreatePseudoConsole(coord, input_read.as_raw(), output_write.as_raw(), 0, &mut raw)
        };
        if result < 0 {
            return Err(hresult_error("terminal.create_pseudoconsole", result));
        }
        if raw == 0 {
            return Err(io::Error::other("terminal.empty_pseudoconsole"));
        }
        let console = PseudoConsole(raw);
        let output_error = Arc::new(Mutex::new(None));
        let failure = Arc::clone(&output_error);
        let output =
            std::thread::Builder::new().name("aura-terminal-output".into()).spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    drain(output_read, sink, &failure, &wake)
                }))
                .unwrap_or_else(|_| Err(io::Error::other("terminal.output_reader_panicked")));
                if let Err(error) = &result
                    && let Ok(mut failure) = failure.lock()
                {
                    *failure = Some(error.to_string());
                    let _ = wake.signal();
                }
                result
            });
        let output = match output {
            Ok(output) => output,
            Err(error) => {
                drop(input_read);
                drop(output_write);
                drop(input_write);
                drop(console);
                return Err(error);
            }
        };
        Ok(Self {
            state: Mutex::new(State {
                console: Some(console),
                creation_peers: Some((input_read, output_write)),
                size,
            }),
            input: Mutex::new(Some(NativeProcessStdin(input_write))),
            output: Mutex::new(Some(output)),
            output_error,
        })
    }
    pub(super) fn handle(&self) -> io::Result<HPCON> {
        self.state
            .lock()
            .map_err(|_| io::Error::other("terminal.console_lock"))?
            .console
            .as_ref()
            .map(|console| console.0)
            .ok_or_else(|| io::Error::other("terminal.closed"))
    }
    pub(super) fn release_creation_peers(&self) -> io::Result<()> {
        self.state
            .lock()
            .map_err(|_| io::Error::other("terminal.console_lock"))?
            .creation_peers
            .take();
        Ok(())
    }
    pub(super) fn write(&self, bytes: &[u8]) -> io::Result<()> {
        self.input
            .lock()
            .map_err(|_| io::Error::other("terminal.input_lock"))?
            .as_mut()
            .ok_or_else(|| io::Error::other("terminal.closed"))?
            .write_all(bytes)
    }
    pub(super) fn resize(&self, size: TerminalSize) -> io::Result<bool> {
        let coord = size.coord()?;
        let mut state = self.state.lock().map_err(|_| io::Error::other("terminal.console_lock"))?;
        let console = state.console.as_ref().ok_or_else(|| io::Error::other("terminal.closed"))?;
        if state.size == size {
            return Ok(false);
        }
        // SAFETY: console ownership is protected against concurrent close by this mutex.
        let result = unsafe { ResizePseudoConsole(console.0, coord) };
        if result < 0 {
            return Err(hresult_error("terminal.resize_pseudoconsole", result));
        }
        state.size = size;
        Ok(true)
    }
    pub(super) fn output_error(&self) -> Option<String> {
        self.output_error.lock().ok()?.clone()
    }
    pub(super) fn close(&self) -> io::Result<()> {
        let (peers, console) = {
            let mut state =
                self.state.lock().map_err(|_| io::Error::other("terminal.console_lock"))?;
            (state.creation_peers.take(), state.console.take())
        };
        // Close the parent's original pipe peers before joining, so EOF is observable.
        drop(peers);
        drop(console);
        self.input.lock().map_err(|_| io::Error::other("terminal.input_lock"))?.take();
        if let Some(output) =
            self.output.lock().map_err(|_| io::Error::other("terminal.output_lock"))?.take()
        {
            output.join().map_err(|_| io::Error::other("terminal.output_reader_panicked"))??;
        }
        Ok(())
    }
}
impl Drop for ConsoleSession {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
fn drain(
    output: OwnedHandle,
    sink: TerminalOutputCallback,
    failure: &Mutex<Option<String>>,
    wake: &wait::TerminalWake,
) -> io::Result<()> {
    let mut bytes = [0u8; 65_536];
    let mut first_failure = None;
    loop {
        let mut count = 0;
        // SAFETY: owned synchronous pipe and exactly sized writable buffer.
        let read = unsafe {
            ReadFile(output.as_raw(), bytes.as_mut_ptr(), 65_536, &mut count, ptr::null_mut())
        };
        if read == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error().is_some_and(|code| {
                code == ERROR_BROKEN_PIPE as i32 || code == ERROR_NO_DATA as i32
            }) {
                break;
            }
            return Err(error);
        }
        if count == 0 {
            break;
        }
        if first_failure.is_none()
            && let Err(error) = sink(&bytes[..count as usize])
        {
            if let Ok(mut failure) = failure.lock() {
                *failure = Some(error.to_string());
            }
            first_failure = Some(error);
            let _ = wake.signal();
        }
        // Even a failed consumer cannot prevent teardown from draining the final frame.
    }
    first_failure.map_or(Ok(()), Err)
}
