use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};

/// COM for the calling thread, for as long as this lives. A thread that already joined COM in
/// another mode keeps it: the Shell and UI Automation calls here work in either.
pub struct Com {
    joined: bool,
}

impl Com {
    pub fn init() -> anyhow::Result<Self> {
        // SAFETY: no reserved pointer; the matching CoUninitialize runs in Drop on this thread.
        let result = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if result == RPC_E_CHANGED_MODE {
            return Ok(Self { joined: false });
        }
        result.ok()?;
        Ok(Self { joined: true })
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.joined {
            // SAFETY: balances the successful CoInitializeEx of this value's thread.
            unsafe { CoUninitialize() };
        }
    }
}
