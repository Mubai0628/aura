use super::*;

/// The writer is transferred to a bounded input task; dropping it sends EOF.
/// It carries no process or Job handle and cannot outlive a killed pipe peer.
pub struct NativeProcessStdin(pub(super) OwnedHandle);
impl io::Write for NativeProcessStdin {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = u32::try_from(bytes.len()).unwrap_or(u32::MAX);
        let mut written = 0;
        // SAFETY: exclusive writer owns the valid synchronous pipe handle;
        // input and byte-count output remain live for the call.
        if unsafe {
            WriteFile(self.0.as_raw(), bytes.as_ptr(), count, &mut written, ptr::null_mut())
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(written as usize)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
