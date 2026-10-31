//! One lock per user around every Credential Manager call this crate makes.
//!
//! Credential Manager loses updates when calls on one user's persisted credentials overlap,
//! whether they come from threads of one process or from several processes: a credential that a
//! successful `CredDeleteW` removed comes back later, sometimes with the value it had before a
//! rewrite, and a credential that a successful `CredWriteW` stored can read back as missing.
//! Calls made one at a time lose nothing, so every Aura process of a user takes the same named
//! mutex for the length of each call.
//!
//! The mutex is in the session's own namespace and its name carries the user's SID: a user's
//! Aura processes run in that user's session, and `Local\` names in other sessions are separate
//! objects. It does not order calls made by other programs.
use std::{io, marker::PhantomData, sync::OnceLock, time::Duration};

use windows_sys::Win32::{
    Foundation::{WAIT_ABANDONED, WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{
        CreateMutexExW, MUTEX_MODIFY_STATE, ReleaseMutex, SYNCHRONIZATION_SYNCHRONIZE,
        WaitForSingleObject,
    },
};

use crate::{handle::OwnedHandle, security::ObjectSecurityDescriptor};

/// How long a call waits for the lock before it fails.
///
/// A holder keeps the lock for one Credential Manager call, or for one listing and its deletes,
/// well under a second, so a wait this long means the holder is stuck; the caller gets an error
/// rather than an unordered call.
const WAIT: Duration = Duration::from_secs(30);

/// What every user of the mutex needs: waiting for it and releasing it.
pub(crate) const MUTEX_USE: u32 = SYNCHRONIZATION_SYNCHRONIZE | MUTEX_MODIFY_STATE;

/// The lock, held until dropped.
///
/// Not `Send`: a Windows mutex can only be released by the thread that acquired it. The same
/// thread may hold it more than once.
pub(crate) struct CredentialLock {
    mutex: &'static OwnedHandle,
    _owning_thread: PhantomData<*const ()>,
}

impl Drop for CredentialLock {
    fn drop(&mut self) {
        // SAFETY: this thread owns the mutex, because `hold` returns a lock only after its
        // wait succeeded on this thread, and the lock cannot move to another thread.
        unsafe {
            let _ = ReleaseMutex(self.mutex.as_raw());
        }
    }
}

/// Waits for the user's Credential Manager lock.
pub(crate) fn hold() -> io::Result<CredentialLock> {
    let mutex = mutex()?;
    let millis = u32::try_from(WAIT.as_millis()).unwrap_or(u32::MAX);
    // SAFETY: the handle is an open mutex with SYNCHRONIZE access for the process lifetime.
    match unsafe { WaitForSingleObject(mutex.as_raw(), millis) } {
        // An abandoned mutex was held by a thread that ended inside its call. The lock only
        // orders calls; Credential Manager keeps its own state, so there is nothing to repair.
        WAIT_OBJECT_0 | WAIT_ABANDONED => Ok(CredentialLock { mutex, _owning_thread: PhantomData }),
        WAIT_TIMEOUT => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            format!("the Credential Manager lock stayed held for {} s", WAIT.as_secs()),
        )),
        WAIT_FAILED => Err(io::Error::last_os_error()),
        other => Err(io::Error::other(format!("unexpected wait result {other}"))),
    }
}

/// This process's handle to the user's mutex, opened on first use and kept open.
fn mutex() -> io::Result<&'static OwnedHandle> {
    static MUTEX: OnceLock<OwnedHandle> = OnceLock::new();
    if let Some(mutex) = MUTEX.get() {
        return Ok(mutex);
    }
    let opened = open()?;
    // A thread that loses the race to store its handle closes it: both name the same mutex.
    Ok(MUTEX.get_or_init(|| opened))
}

fn open() -> io::Result<OwnedHandle> {
    let user = crate::security::current_user_sid_string()?;
    let name = wide_null(&mutex_name(&user));
    let mut security = ObjectSecurityDescriptor::for_credential_lock(&user)?;
    // SAFETY: the attributes and the name stay alive for the call. An existing mutex is opened
    // with the same access; the returned handle is owned at once.
    let mutex =
        unsafe { CreateMutexExW(security.attributes_ptr().cast(), name.as_ptr(), 0, MUTEX_USE) };
    OwnedHandle::from_raw(mutex)
}

fn mutex_name(user_sid: &str) -> String {
    format!("Local\\Aura.CredentialManager.{user_sid}")
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{hold, mutex_name};

    #[test]
    fn the_lock_is_per_user_and_in_the_session_namespace() {
        assert_eq!(
            mutex_name("S-1-5-21-1-2-3-1001"),
            "Local\\Aura.CredentialManager.S-1-5-21-1-2-3-1001"
        );
    }

    #[test]
    fn one_thread_may_hold_the_lock_again_and_another_waits_for_both_releases() {
        let outer = hold().expect("the lock");
        let inner = hold().expect("the same thread takes it again");
        let (taken, waiting) = std::sync::mpsc::channel();
        let other = std::thread::spawn(move || {
            let lock = hold().expect("another thread takes it once released");
            taken.send(()).expect("report");
            drop(lock);
        });
        drop(inner);
        assert!(
            waiting.recv_timeout(std::time::Duration::from_millis(300)).is_err(),
            "the lock is still held once"
        );
        drop(outer);
        waiting
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the other thread got the lock after both releases");
        other.join().expect("other thread");
    }
}
