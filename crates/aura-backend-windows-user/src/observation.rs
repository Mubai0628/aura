//! Optional information about a real Shell worker. This owns no execution authority.
use std::sync::Arc;

/// Implemented by the information index, not by the executor's state machine.
pub trait ShellExecutionObserver: Send + Sync {
    fn started(&self);
    fn foreground_finished(&self);
    fn worker_finished(&self) {}
}

/// Kept by the actual worker, including native blocking work that outlives its waiter.
/// Panics in an information-only callback must not change a Shell result.
pub(crate) struct ExecutionObservation {
    sink: Option<Arc<dyn ShellExecutionObserver>>,
    foreground_finished: bool,
}

impl ExecutionObservation {
    pub(crate) fn capture(
        port: &dyn crate::WindowsUserCorePort,
        request: &aura_execution_contract::DispatchRequest,
    ) -> Self {
        Self {
            sink: std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                port.execution_observer(request)
            }))
            .ok()
            .flatten(),
            foreground_finished: false,
        }
    }

    pub(crate) fn started(&self) {
        if let Some(observer) = &self.sink {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| observer.started()));
        }
    }

    pub(crate) fn finish(&mut self) {
        if !self.foreground_finished
            && let Some(observer) = &self.sink
        {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                observer.foreground_finished();
            }));
        }
        self.foreground_finished = true;
    }
}

impl Drop for ExecutionObservation {
    fn drop(&mut self) {
        self.finish();
        if let Some(observer) = self.sink.take() {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                observer.worker_finished()
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Default)]
    struct Counts {
        start: AtomicUsize,
        foreground: AtomicUsize,
        worker: AtomicUsize,
    }
    impl ShellExecutionObserver for Counts {
        fn started(&self) {
            self.start.fetch_add(1, Ordering::SeqCst);
        }
        fn foreground_finished(&self) {
            self.foreground.fetch_add(1, Ordering::SeqCst);
        }
        fn worker_finished(&self) {
            self.worker.fetch_add(1, Ordering::SeqCst);
        }
    }
    #[test]
    fn foreground_completion_does_not_end_the_managed_worker() {
        let counts = Arc::new(Counts::default());
        let mut observer =
            ExecutionObservation { sink: Some(counts.clone()), foreground_finished: false };
        observer.started();
        observer.finish();
        observer.finish();
        assert_eq!(counts.start.load(Ordering::SeqCst), 1);
        assert_eq!(counts.foreground.load(Ordering::SeqCst), 1);
        assert_eq!(counts.worker.load(Ordering::SeqCst), 0);
        drop(observer);
        assert_eq!(counts.worker.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn not_started_does_not_invent_a_start_and_unwind_still_cleans_up() {
        let counts = Arc::new(Counts::default());
        let output = counts.clone();
        let failed = std::panic::catch_unwind(move || {
            let _worker = ExecutionObservation { sink: Some(output), foreground_finished: false };
            panic!("bounded worker fault");
        });
        assert!(failed.is_err());
        assert_eq!(counts.start.load(Ordering::SeqCst), 0);
        assert_eq!(counts.foreground.load(Ordering::SeqCst), 1);
        assert_eq!(counts.worker.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn information_callback_panics_do_not_escape_into_the_executor() {
        struct Bad;
        impl ShellExecutionObserver for Bad {
            fn started(&self) {
                panic!("information start");
            }
            fn foreground_finished(&self) {
                panic!("information end");
            }
            fn worker_finished(&self) {
                panic!("information cleanup");
            }
        }
        let observer =
            ExecutionObservation { sink: Some(Arc::new(Bad)), foreground_finished: false };
        observer.started();
        drop(observer);
    }
}
