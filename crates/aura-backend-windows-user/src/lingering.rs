//! Process groups whose stop could not be confirmed when the call that started them ended. Each is
//! kept open, under that call, until no process is left in it: the call's locks wait for that,
//! because what it was changing may still be changing. Closing a group's last handle stops what is
//! left in it, so a group kept here has already been told to stop, and is closed once it is seen
//! empty or when Core exits.
use std::cell::Cell;
use std::collections::BTreeMap;
use std::sync::Mutex;

/// The call a process group was started for: its operation's identity.
pub type Scope = [u8; 16];

/// A process group that may still have processes in it.
pub(crate) trait Group: Send {
    /// Whether a process may still be running in it; a count that cannot be read counts as one.
    fn running(&self) -> bool;
}

impl Group for aura_windows_platform::RootJob {
    fn running(&self) -> bool {
        !matches!(self.active_processes(), Ok(0))
    }
}

/// A process that was not created inside its group: nothing can show that it stopped, so its call
/// keeps its locks until Core restarts.
pub(crate) struct Untracked;

impl Group for Untracked {
    fn running(&self) -> bool {
        true
    }
}

thread_local! {
    static CURRENT: Cell<Option<Scope>> = const { Cell::new(None) };
}

static KEPT: Mutex<BTreeMap<Scope, Vec<Box<dyn Group>>>> = Mutex::new(BTreeMap::new());

/// While held, the groups this thread leaves unconfirmed are kept under its call.
pub struct Entered(Option<Scope>);

impl Drop for Entered {
    fn drop(&mut self) {
        CURRENT.with(|current| current.set(self.0));
    }
}

/// Keeps the groups this thread leaves unconfirmed under `scope` until the returned value drops.
#[must_use]
pub fn enter(scope: Scope) -> Entered {
    Entered(CURRENT.with(|current| current.replace(Some(scope))))
}

/// Keeps a group whose stop was not confirmed under the current call. Outside a call it is closed
/// at once, which stops what is left in it.
pub(crate) fn keep(group: impl Group + 'static) {
    let Some(scope) = CURRENT.with(Cell::get) else {
        return;
    };
    KEPT.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(scope)
        .or_default()
        .push(Box::new(group));
}

/// Whether no process of the groups `scope`'s call left is still running; the groups found empty
/// are closed. A call that left none is settled.
#[must_use]
pub fn settled(scope: Scope) -> bool {
    let mut kept = KEPT.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(groups) = kept.get_mut(&scope) else {
        return true;
    };
    groups.retain(|group| group.running());
    if groups.is_empty() {
        kept.remove(&scope);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Fake(Arc<AtomicBool>);

    impl Group for Fake {
        fn running(&self) -> bool {
            self.0.load(Ordering::Acquire)
        }
    }

    #[test]
    fn a_kept_group_holds_its_call_until_no_process_is_left() {
        let scope = [7; 16];
        let running = Arc::new(AtomicBool::new(true));
        {
            let _call = enter(scope);
            keep(Fake(Arc::clone(&running)));
        }
        assert!(!settled(scope), "a process may still be running");
        assert!(!settled(scope), "checking again does not give it up");
        running.store(false, Ordering::Release);
        assert!(settled(scope), "the group is empty");
        assert!(settled(scope), "and was closed");
    }

    #[test]
    fn a_call_that_left_nothing_running_is_settled() {
        assert!(settled([8; 16]));
    }

    #[test]
    fn a_group_left_outside_a_call_is_not_kept() {
        let running = Arc::new(AtomicBool::new(true));
        keep(Fake(Arc::clone(&running)));
        let scope = [9; 16];
        {
            let _call = enter(scope);
        }
        keep(Fake(running));
        assert!(settled(scope));
    }

    #[test]
    fn an_untracked_process_never_settles_its_call() {
        let scope = [10; 16];
        {
            let _call = enter(scope);
            keep(Untracked);
        }
        assert!(!settled(scope));
    }
}
