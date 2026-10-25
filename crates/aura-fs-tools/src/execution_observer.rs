//! Optional, information-only start facts copied from a fully admitted file execution.
//! A scope belongs to one synchronous carrier. It holds no permission or resource commit.
use aura_workspace_domain::ResourceRequest;
use std::{cell::RefCell, sync::Arc};

pub type FileExecutionObserver = Arc<dyn Fn(&[ResourceRequest]) -> Box<dyn Send> + Send + Sync>;

thread_local! {
    static CURRENT: RefCell<Option<FileExecutionObserver>> = const { RefCell::new(None) };
}

pub fn with_file_execution_observer<T>(
    observer: Option<FileExecutionObserver>,
    work: impl FnOnce() -> T,
) -> T {
    struct Restore(Option<FileExecutionObserver>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.with(|slot| {
                slot.replace(self.0.take());
            });
        }
    }
    let _restore = Restore(CURRENT.with(|slot| slot.replace(observer)));
    work()
}

/// Called by the actual owner after complete admission and before the first file effect.
/// Publisher errors cannot change the file operation's outcome.
pub fn observe_file_execution(resources: &[ResourceRequest]) -> Option<Box<dyn Send>> {
    let observer = CURRENT.with(|slot| slot.borrow().clone())?;
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer(resources))).ok()
}
