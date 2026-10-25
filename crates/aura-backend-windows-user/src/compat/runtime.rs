//! One whole-package check per managed runtime in this Core process, started in the background.
use super::CompatBackend;
use anyhow::Result;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};
use tokio::sync::watch;

type Ready = Option<Result<Arc<CompatBackend>, String>>;
type Checks = Mutex<BTreeMap<PathBuf, watch::Receiver<Ready>>>;
static CHECKS: OnceLock<Checks> = OnceLock::new();

pub(super) struct RuntimeCheck(watch::Receiver<Ready>);
impl RuntimeCheck {
    pub(super) fn start(root: &Path) -> Self {
        let root = root.to_path_buf();
        let mut checks = CHECKS.get_or_init(Default::default).lock().expect("runtime checks lock");
        if let Some(check) = checks.get(&root) {
            return Self(check.clone());
        }
        let (ready, check) = watch::channel(None);
        let source = root.clone();
        let failure = ready.clone();
        if std::thread::Builder::new()
            .name("aura-shell-prepare".into())
            .spawn(move || {
                let result =
                    CompatBackend::new(&source.join("payload"), &source.join("manifest.json"))
                        .map(Arc::new)
                        .map_err(|_| "compat.runtime_verification_failed".to_owned());
                let _ = ready.send(Some(result));
            })
            .is_err()
        {
            let _ = failure.send(Some(Err("compat.prepare_worker_unavailable".into())));
        }
        checks.insert(root, check.clone());
        Self(check)
    }
    pub(super) fn get(&self) -> Ready {
        self.0.borrow().clone()
    }
    pub(super) async fn wait(&self) -> Result<Arc<CompatBackend>> {
        let mut ready = self.0.clone();
        loop {
            if let Some(result) = ready.borrow().clone() {
                return result.map_err(anyhow::Error::msg);
            }
            ready.changed().await.map_err(|_| anyhow::anyhow!("compat.prepare_worker_lost"))?;
        }
    }
    /// Cancels only this waiter. The shared package-verification thread continues.
    pub(super) async fn wait_with_cancel(
        &self,
        mut cancelled: watch::Receiver<bool>,
    ) -> Result<Arc<CompatBackend>> {
        tokio::select! {
            biased;
            _ = cancelled.wait_for(|value| *value) => anyhow::bail!("terminal.open_cancelled"),
            result = self.wait() => result,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancelling_a_runtime_waiter_preserves_the_shared_verification() {
        let (ready, receiver) = watch::channel(None);
        let check = RuntimeCheck(receiver);
        let (cancel, cancelled) = watch::channel(false);
        let waiting = check.wait_with_cancel(cancelled);
        tokio::pin!(waiting);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), &mut waiting).await.is_err()
        );
        cancel.send_replace(true);
        assert_eq!(waiting.await.err().unwrap().to_string(), "terminal.open_cancelled");
        assert!(check.get().is_none(), "cancelling one waiter does not settle the shared check");
        ready.send_replace(Some(Err("temporary verification result".into())));
        assert_eq!(check.wait().await.err().unwrap().to_string(), "temporary verification result");
    }
}
