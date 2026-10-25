use aura_windows_platform::NativePathInfo;
use aura_workspace_domain::{FsSkippedScope, FsSkippedScopeReason};
use std::{
    cell::RefCell,
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

type Classifier = dyn Fn(&NativePathInfo) -> Option<FsSkippedScopeReason> + Send + Sync;

/// A trusted caller installs this only for its own synchronous file invocation.
/// Search workers inherit the same scope explicitly; unrelated Main reads do not.
#[derive(Clone)]
pub struct FileReadScope(Arc<State>);
struct State {
    classifier: Arc<Classifier>,
    skipped: Mutex<BTreeSet<FsSkippedScope>>,
}
thread_local! {
    static CURRENT: RefCell<Option<FileReadScope>> = const { RefCell::new(None) };
}

impl FileReadScope {
    pub fn new(
        classifier: impl Fn(&NativePathInfo) -> Option<FsSkippedScopeReason> + Send + Sync + 'static,
    ) -> Self {
        Self(Arc::new(State {
            classifier: Arc::new(classifier),
            skipped: Mutex::new(BTreeSet::new()),
        }))
    }
    pub fn install(&self) -> FileReadScopeGuard {
        FileReadScopeGuard(CURRENT.with(|slot| slot.replace(Some(self.clone()))))
    }
    pub fn skipped(&self) -> Vec<FsSkippedScope> {
        self.0.skipped.lock().unwrap_or_else(|e| e.into_inner()).iter().cloned().collect()
    }
    fn allows(&self, info: &NativePathInfo) -> bool {
        let Some(reason) = (self.0.classifier)(info) else { return true };
        self.skip(&info.final_path, reason);
        false
    }
    fn skip(&self, path: &std::path::Path, reason: FsSkippedScopeReason) {
        self.0
            .skipped
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(FsSkippedScope { canonical_path: path.display().to_string(), reason });
    }
}
pub struct FileReadScopeGuard(Option<FileReadScope>);
impl Drop for FileReadScopeGuard {
    fn drop(&mut self) {
        CURRENT.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}
pub(crate) fn current() -> Option<FileReadScope> {
    CURRENT.with(|slot| slot.borrow().clone())
}
pub(crate) fn allows(info: &NativePathInfo) -> bool {
    current().is_none_or(|scope| scope.allows(info))
}
pub(crate) fn unavailable(path: &std::path::Path) {
    if let Some(scope) = current() {
        scope.skip(path, FsSkippedScopeReason::TargetUnavailable);
    }
}
