//! The client end of a named pipe, read on one thread while written on another. A handle opened
//! for synchronous I/O lets one operation through at a time, so a read waiting for Core would
//! hold back every write; this one is opened for overlapped I/O, and each call waits for its own
//! operation to finish.
use std::io;
use std::sync::Arc;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_BROKEN_PIPE, ERROR_IO_PENDING, ERROR_PIPE_NOT_CONNECTED, HANDLE,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_READ_ATTRIBUTES, FILE_READ_DATA, FILE_SHARE_NONE,
    FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, OPEN_EXISTING, ReadFile, SECURITY_IDENTIFICATION,
    SECURITY_SQOS_PRESENT, WriteFile,
};
use windows::Win32::System::IO::{GetOverlappedResult, OVERLAPPED};
use windows::Win32::System::Threading::CreateEventW;
use windows::core::{HSTRING, PCWSTR};

struct Handle(HANDLE);

// SAFETY: a pipe handle may be used from any thread; each operation brings its own OVERLAPPED.
unsafe impl Send for Handle {}
// SAFETY: as above; concurrent operations on an overlapped handle are allowed.
unsafe impl Sync for Handle {}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle was opened by `connect` and is closed once, here.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// One end of a pipe, for a reader and a writer on two threads.
#[derive(Clone)]
pub(crate) struct Pipe(Arc<Handle>);

impl Pipe {
    /// Opens an existing pipe with only the rights to read and write its data: never the right
    /// to create an instance, and the server may identify this process but not act as it.
    pub(crate) fn connect(name: &str) -> io::Result<Self> {
        // SAFETY: the name is a valid string for the call; the handle is owned from here on.
        let handle = unsafe {
            CreateFileW(
                &HSTRING::from(name),
                (FILE_READ_DATA | FILE_WRITE_DATA | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES).0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                None,
            )
        }
        .map_err(io::Error::from)?;
        Ok(Self(Arc::new(Handle(handle))))
    }

    fn transfer(&self, read: Option<&mut [u8]>, write: Option<&[u8]>) -> io::Result<usize> {
        // SAFETY: a manual-reset event, owned below.
        let event =
            unsafe { CreateEventW(None, true, false, PCWSTR::null()) }.map_err(io::Error::from)?;
        let event = Handle(event);
        let mut overlapped = OVERLAPPED { hEvent: event.0, ..Default::default() };
        // SAFETY: the buffer and `overlapped` outlive the operation: it is waited for below before
        // either goes out of scope.
        let started = unsafe {
            match (read, write) {
                (Some(buffer), _) => {
                    ReadFile(self.0.0, Some(buffer), None, Some(&raw mut overlapped))
                }
                (None, Some(buffer)) => {
                    WriteFile(self.0.0, Some(buffer), None, Some(&raw mut overlapped))
                }
                (None, None) => return Ok(0),
            }
        };
        if let Err(error) = started
            && error.code() != ERROR_IO_PENDING.to_hresult()
        {
            return closed_as_end(error);
        }
        let mut moved = 0_u32;
        // SAFETY: `overlapped` belongs to the operation started above, which this waits for.
        match unsafe { GetOverlappedResult(self.0.0, &raw const overlapped, &raw mut moved, true) }
        {
            Ok(()) => Ok(moved as usize),
            Err(error) => closed_as_end(error),
        }
    }
}

/// A pipe the other end closed reads as its end and writes as a broken pipe.
fn closed_as_end(error: windows::core::Error) -> io::Result<usize> {
    let code = error.code();
    if code == ERROR_BROKEN_PIPE.to_hresult() || code == ERROR_PIPE_NOT_CONNECTED.to_hresult() {
        return Err(io::Error::from(io::ErrorKind::BrokenPipe));
    }
    Err(io::Error::from(error))
}

impl io::Read for Pipe {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let length = buffer.len().min(u32::MAX as usize);
        match self.transfer(Some(&mut buffer[..length]), None) {
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(0),
            other => other,
        }
    }
}

impl io::Write for Pipe {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if buffer.is_empty() {
            return Ok(0);
        }
        let length = buffer.len().min(u32::MAX as usize);
        self.transfer(None, Some(&buffer[..length]))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
