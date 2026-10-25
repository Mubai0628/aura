//! Core's end of the channel to the browser host (Electron Main). A command goes out as the
//! `browser.command.v1` notification on Main's connection; Main answers on the Main-only route
//! `browser.host.result`. Core's own timer decides when a command has gone unanswered, and a
//! command Core stops waiting for (timed out, or its turn stopped) is withdrawn with
//! `browser.cancel.v1`.
use aura_browser_domain::{BrowserHostCancelV1, BrowserHostCommandV1, BrowserHostResultV1};
use aura_config::{BrowserSettingsSnapshot, SettingsFacade};
use aura_domain_common::{CorrelationId, SessionId};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::oneshot;

#[derive(Debug, thiserror::Error)]
pub(crate) enum BridgeError {
    #[error("browser.not_connected: 浏览器不可用：Aura 窗口还没有连上 Core，请稍后重试。")]
    NotConnected,
    #[error("browser.no_answer: 浏览器在 {0} 秒内没有回应。")]
    TimedOut(u32),
    #[error("browser.interrupted: 浏览器连接中断。")]
    Interrupted,
    #[error("browser.stopped: 这一步已停止。")]
    Stopped,
}

type Pending = HashMap<String, oneshot::Sender<BrowserHostResultV1>>;

#[derive(Default)]
pub(crate) struct BrowserBridge {
    publisher: Mutex<Option<crate::AsyncPriorityQueue>>,
    pending: Mutex<Pending>,
    /// The address each session's model page was last reported at.
    pages: Mutex<HashMap<SessionId, String>>,
    settings: OnceLock<Arc<SettingsFacade>>,
    /// Set only by a test that plays the host itself, so its own requests do not move the channel.
    pinned: AtomicBool,
}

impl BrowserBridge {
    pub(crate) fn bind_settings(&self, settings: Arc<SettingsFacade>) {
        let _ = self.settings.set(settings);
    }

    /// The browser settings as the user saved them; none before they can be read.
    pub(crate) fn settings(&self) -> Option<BrowserSettingsSnapshot> {
        self.settings.get().map(|settings| settings.browser_settings())
    }

    /// Main's connection, set by every authenticated Agent route call.
    pub(crate) fn set_publisher(&self, queue: crate::AsyncPriorityQueue) {
        if self.pinned.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(mut slot) = self.publisher.lock() {
            *slot = Some(queue);
        }
    }

    /// A test playing the browser host takes the channel for itself.
    #[cfg(test)]
    pub(crate) fn pin_publisher(&self, queue: crate::AsyncPriorityQueue) {
        self.set_publisher(queue);
        self.pinned.store(true, Ordering::SeqCst);
    }

    pub(crate) fn page_url(&self, session: SessionId) -> Option<String> {
        self.pages.lock().ok()?.get(&session).cloned()
    }

    pub(crate) fn note_page(&self, session: SessionId, url: &str) {
        if let Ok(mut pages) = self.pages.lock() {
            pages.insert(session, url.to_owned());
        }
    }

    /// Main's answer. False when nothing is waiting under that id any more.
    pub(crate) fn complete(&self, result: BrowserHostResultV1) -> bool {
        let sender =
            self.pending.lock().ok().and_then(|mut pending| pending.remove(&result.command_id));
        sender.is_some_and(|sender| sender.send(result).is_ok())
    }

    fn forget(&self, id: &str) {
        if let Ok(mut pending) = self.pending.lock() {
            pending.remove(id);
        }
    }

    /// Tells Main that Core no longer waits for the command. Best effort: a lost withdrawal leaves
    /// a command Core has already given up on, which Main's own timers end.
    async fn withdraw(&self, queue: &crate::AsyncPriorityQueue, command: &BrowserHostCommandV1) {
        let cancel = BrowserHostCancelV1 {
            command_id: command.command_id.clone(),
            runtime_instance_id: command.runtime_instance_id,
        };
        let _ =
            crate::send_notification(queue, crate::QueuePriority::P1, "browser.cancel.v1", &cancel)
                .await;
    }

    /// Sends the command and waits for Main's answer, at most the command's own timeout, and
    /// until `stop` turns true (the call was cancelled).
    pub(crate) async fn send(
        &self,
        mut command: BrowserHostCommandV1,
        mut stop: tokio::sync::watch::Receiver<bool>,
    ) -> Result<BrowserHostResultV1, BridgeError> {
        if *stop.borrow() {
            return Err(BridgeError::Stopped);
        }
        command.command_id = format!("bcm-{}", CorrelationId::new());
        let id = command.command_id.clone();
        let (sender, receiver) = oneshot::channel();
        self.pending.lock().map_err(|_| BridgeError::Interrupted)?.insert(id.clone(), sender);
        let queue = self.publisher.lock().ok().and_then(|slot| slot.clone());
        let Some(queue) = queue else {
            self.forget(&id);
            return Err(BridgeError::NotConnected);
        };
        if crate::send_notification(
            &queue,
            crate::QueuePriority::P1,
            "browser.command.v1",
            &command,
        )
        .await
        .is_err()
        {
            self.forget(&id);
            return Err(BridgeError::NotConnected);
        }
        let wait = std::time::Duration::from_millis(u64::from(command.timeout_ms));
        let stopped = async {
            while !*stop.borrow_and_update() {
                if stop.changed().await.is_err() {
                    // Nobody can stop the call any more: wait for the answer or the timer.
                    std::future::pending::<()>().await;
                }
            }
        };
        tokio::select! {
            answer = tokio::time::timeout(wait, receiver) => match answer {
                Ok(Ok(result)) => {
                    self.note_page(command.session_id, &result.page_url);
                    Ok(result)
                }
                Ok(Err(_)) => Err(BridgeError::Interrupted),
                Err(_) => {
                    self.forget(&id);
                    self.withdraw(&queue, &command).await;
                    Err(BridgeError::TimedOut(command.timeout_ms.div_ceil(1000)))
                }
            },
            () = stopped => {
                self.forget(&id);
                self.withdraw(&queue, &command).await;
                Err(BridgeError::Stopped)
            }
        }
    }
}
