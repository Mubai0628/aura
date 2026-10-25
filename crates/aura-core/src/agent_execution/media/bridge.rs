//! One active command owns one readable native handle. Main never supplies a filesystem path.
use aura_agent_domain::*;
use aura_domain_common::{CorrelationId, RuntimeInstanceId, SessionId};
use aura_windows_platform::VerifiedFile;
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::oneshot;

pub(super) struct Pending {
    pub runtime: RuntimeInstanceId,
    pub session: SessionId,
    pub size: u64,
    pub file: Arc<VerifiedFile>,
    sender: oneshot::Sender<MediaHostResultV1>,
}

#[derive(Default)]
pub(crate) struct MediaBridge {
    publisher: Mutex<Option<crate::AsyncPriorityQueue>>,
    pub(super) pending: Mutex<HashMap<String, Pending>>,
    pinned: AtomicBool,
}

struct Lifetime<'a> {
    bridge: &'a MediaBridge,
    id: String,
}
impl Drop for Lifetime<'_> {
    fn drop(&mut self) {
        self.bridge.forget(&self.id);
    }
}

impl MediaBridge {
    pub(crate) fn set_publisher(&self, queue: crate::AsyncPriorityQueue) {
        if !self.pinned.load(Ordering::SeqCst)
            && let Ok(mut slot) = self.publisher.lock()
        {
            *slot = Some(queue);
        }
    }

    #[cfg(test)]
    pub(crate) fn pin_publisher(&self, queue: crate::AsyncPriorityQueue) {
        self.set_publisher(queue);
        self.pinned.store(true, Ordering::SeqCst);
    }

    pub(crate) fn complete(&self, result: MediaHostResultV1) -> bool {
        let Ok(mut pending) = self.pending.lock() else { return false };
        let Some(entry) = pending.get(&result.command_id) else { return false };
        if entry.runtime != result.runtime_instance_id || entry.session != result.session_id {
            return false;
        }
        pending.remove(&result.command_id).is_some_and(|entry| entry.sender.send(result).is_ok())
    }

    fn forget(&self, id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(id);
        }
    }

    pub(crate) async fn send(
        &self,
        mut command: MediaHostCommandV1,
        file: VerifiedFile,
        mut stop: tokio::sync::watch::Receiver<bool>,
    ) -> anyhow::Result<MediaHostResultV1> {
        anyhow::ensure!(!*stop.borrow(), "media.stopped: 这一步已停止。");
        anyhow::ensure!(
            command.timeout_ms > 0 && command.timeout_ms <= MEDIA_TIMEOUT_MS,
            "media.timeout_invalid: 视频解码的时间上限不可用。"
        );
        anyhow::ensure!(
            !file.info().is_directory && file.file().metadata()?.len() == command.file_size,
            "media.source_changed: 视频文件已改变，请重新读取。"
        );
        let queue = self.publisher.lock().ok().and_then(|slot| slot.clone()).ok_or_else(|| {
            anyhow::anyhow!("media.not_connected: 视频解码服务未连接，请稍后重试。")
        })?;
        command.command_id = format!("mcm-{}", CorrelationId::new());
        let id = command.command_id.clone();
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().map_err(|_| anyhow::anyhow!("media.interrupted"))?.insert(
            id.clone(),
            Pending {
                runtime: command.runtime_instance_id,
                session: command.session_id,
                size: command.file_size,
                file: Arc::new(file),
                sender,
            },
        );
        let lifetime = Lifetime { bridge: self, id };
        let wait = std::time::Duration::from_millis(u64::from(command.timeout_ms));
        let work = async {
            crate::send_notification(
                &queue,
                crate::QueuePriority::P1,
                "media.command.v1",
                &command,
            )
            .await
            .map_err(|_| anyhow::anyhow!("media.interrupted: 视频解码连接已中断。"))?;
            receiver.await.map_err(|_| anyhow::anyhow!("media.interrupted: 视频解码连接已中断。"))
        };
        let stopped = async {
            while !*stop.borrow_and_update() {
                if stop.changed().await.is_err() {
                    std::future::pending::<()>().await;
                }
            }
        };
        let result = tokio::select! {
            result = tokio::time::timeout(wait, work) => result
                .unwrap_or_else(|_| Err(anyhow::anyhow!("media.no_answer: 视频解码在限定时间内没有回应。"))),
            () = stopped => Err(anyhow::anyhow!("media.stopped: 这一步已停止。")),
            () = queue.closed() => Err(anyhow::anyhow!("media.interrupted: 视频解码连接已中断。")),
        };
        drop(lifetime);
        if result.is_err() {
            let cancel = MediaHostCancelV1 {
                command_id: command.command_id,
                runtime_instance_id: command.runtime_instance_id,
                session_id: command.session_id,
            };
            let _ = tokio::time::timeout(
                std::time::Duration::from_millis(100),
                crate::send_notification(
                    &queue,
                    crate::QueuePriority::P1,
                    "media.cancel.v1",
                    &cancel,
                ),
            )
            .await;
        }
        result
    }
}
