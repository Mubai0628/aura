use std::{io, ptr};

use windows_sys::Win32::{
    Foundation::{CloseHandle, DuplicateHandle, HANDLE, INVALID_HANDLE_VALUE},
    System::Threading::GetCurrentProcess,
};

const DUPLICATE_SAME_ACCESS: u32 = 2;

pub(crate) struct OwnedHandle(HANDLE);

// Windows kernel object handles may be used from any thread while their owned
// handle remains open. `OwnedHandle` never exposes ownership or closes early.
unsafe impl Send for OwnedHandle {}
unsafe impl Sync for OwnedHandle {}

impl OwnedHandle {
    pub(crate) fn from_raw(handle: HANDLE) -> io::Result<Self> {
        if handle.is_null() || handle == INVALID_HANDLE_VALUE {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(handle))
        }
    }

    pub(crate) const fn as_raw(&self) -> HANDLE {
        self.0
    }

    pub(crate) fn try_clone(&self) -> io::Result<Self> {
        let mut duplicated = ptr::null_mut();
        // SAFETY: GetCurrentProcess is a process-lifetime pseudo-handle. The
        // source handle is owned and the duplicate is adopted immediately.
        let process = unsafe { GetCurrentProcess() };
        if unsafe {
            DuplicateHandle(process, self.0, process, &mut duplicated, 0, 0, DUPLICATE_SAME_ACCESS)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Self::from_raw(duplicated)
    }

    pub(crate) fn into_raw(self) -> HANDLE {
        let handle = self.0;
        std::mem::forget(self);
        handle
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: `OwnedHandle` is only constructed from a unique, valid owned
        // handle and never exposes ownership. Drop runs exactly once.
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// A non-owning view of a handle someone else closes.
///
/// This exists so a caller that already owns a `File` can run the identity and
/// final-path queries against it without creating a second close owner. It is a
/// different type from [`OwnedHandle`] rather than a flag, so the two cannot be
/// confused: there is no `Drop` here, no `into_raw`, and no way to turn a
/// `BorrowedHandle` into an owned one. A failed query therefore cannot leave the
/// handle un-closed or doubly closed — the borrow simply ends, and the original
/// owner still closes exactly once.
#[derive(Clone, Copy)]
pub(crate) struct BorrowedHandle<'a>(HANDLE, std::marker::PhantomData<&'a ()>);

impl<'a> BorrowedHandle<'a> {
    /// Borrow the handle a `File`-like owner holds for `'a`.
    pub(crate) fn borrow(owner: &'a impl std::os::windows::io::AsRawHandle) -> Self {
        Self(owner.as_raw_handle(), std::marker::PhantomData)
    }

    pub(crate) const fn as_raw(self) -> HANDLE {
        self.0
    }
}

impl OwnedHandle {
    /// A view of this handle that borrows it for `'a` instead of owning it.
    pub(crate) fn borrow(&self) -> BorrowedHandle<'_> {
        BorrowedHandle(self.0, std::marker::PhantomData)
    }
}
