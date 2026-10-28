//! Aura's side of Chrome control: Chrome's registration of the relay, the relay's pipe, pairing,
//! the challenge every connection answers, and the commands of the one connected extension. A
//! tool reaches Chrome only through [`ChromeControl::request`].
mod access;
mod connection;
mod listener;
mod pairing;
mod registration;
pub(crate) mod secret;
pub(crate) mod shares;
mod stale_relays;
#[cfg(test)]
pub(crate) mod test_extension;
#[cfg(test)]
mod tests;

pub(crate) use access::{CallSites, ChromeSessions, PersistedSessions, ResolvedTab};
pub(crate) use secret::ChromeSecrets;
#[cfg(test)]
pub(crate) use secret::MemoryChromeSecrets;

use aura_chrome_domain::protocol::{
    CommandOutcome, CoreMessage, PAIRING_SECRET_BYTES, RELAY_PROGRAM, extension_id_for,
    native_host_name, pipe_name, plain_path,
};
use aura_chrome_domain::{
    ChromeConnectionState, ChromeControlStatus, ChromePairedExtension, ChromePairingCode,
};
use aura_config::{ChromeControlSettings, ChromePairing, SettingsFacade};
use aura_domain_common::{CredentialId, UtcTimestamp};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};

#[derive(Clone, Debug)]
pub(crate) struct ChromeControlOptions {
    /// The native-messaging host name Chrome knows this installation by.
    pub host_name: String,
    /// This installation's manifest for that host.
    pub manifest_path: PathBuf,
    /// The relay Chrome starts, and the only program the pipe accepts.
    pub relay_executable: PathBuf,
    pub pipe_name: String,
    /// The only extension this installation pairs with.
    pub extension_id: String,
    pub pairing_valid_for: Duration,
    pub heartbeat: Duration,
    /// Whether turning Chrome control on writes Chrome's registration; a test of the pipe alone
    /// leaves the user's registry untouched.
    pub register: bool,
}

impl ChromeControlOptions {
    /// The relay installed beside this Core, and this namespace's host name and extension.
    pub(crate) fn for_installation(namespace: &str, local_root: &Path) -> Self {
        // Plain, as Windows reports the relay's image and as the relay names the pipe.
        let core = std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .map(|core| plain_path(&core))
            .unwrap_or_else(|_| PathBuf::from("aura-core.exe"));
        let folder = core.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
        let host_name = native_host_name(namespace);
        // Core's own tests are the relay and Core at once, and never touch the user's registry;
        // every test's Core listens on a pipe of its own.
        let (relay_executable, pipe) = if cfg!(test) {
            (core.clone(), pipe_name(local_root))
        } else {
            (folder.join(RELAY_PROGRAM), pipe_name(&folder))
        };
        Self {
            manifest_path: local_root.join("chrome").join(format!("{host_name}.json")),
            relay_executable,
            pipe_name: pipe,
            extension_id: extension_id_for(namespace).to_owned(),
            host_name,
            pairing_valid_for: Duration::from_secs(300),
            heartbeat: Duration::from_secs(30),
            register: !cfg!(test),
        }
    }
}

/// Why a command did not complete.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ChromeRequestError {
    NotConnected,
    TimedOut,
    /// The connection ended while the command was out; whether it ran is not known.
    Disconnected,
    Failed {
        code: String,
        message: String,
    },
}

#[derive(Clone)]
pub(crate) struct ChromeControl {
    inner: Arc<Inner>,
}

pub(super) struct Inner {
    options: ChromeControlOptions,
    settings: Arc<SettingsFacade>,
    secrets: Arc<dyn ChromeSecrets>,
    state: Mutex<State>,
    /// The conversations a tab can be shared with; set once the Agent service exists.
    sessions: Mutex<Option<Arc<dyn ChromeSessions>>>,
}

#[derive(Default)]
pub(super) struct State {
    listener: Option<tokio::task::JoinHandle<()>>,
    /// The code the settings page shows, and when it stops working.
    pairing: Option<(pairing::PendingCode, UtcTimestamp)>,
    live: Option<Live>,
    next_id: u64,
    shares: shares::Shares,
    /// The conversation using Chrome now, and how many of its calls are running.
    holder: Option<(aura_domain_common::SessionId, usize)>,
    /// How many times the user pressed Stop; a call from before the last Stop holds nothing.
    stops: u64,
    /// What the extension found before an action, by tool call, for admission to read.
    inspections:
        BTreeMap<aura_domain_common::ToolCallId, aura_chrome_domain::commands::ChromeInspection>,
    /// What each call's first resolution read, by tool call.
    calls: BTreeMap<aura_domain_common::ToolCallId, access::CallSites>,
}

/// The one connected extension.
pub(super) struct Live {
    outgoing: mpsc::Sender<CoreMessage>,
    pending: BTreeMap<u64, oneshot::Sender<CommandOutcome>>,
    authenticated: bool,
    close: Option<oneshot::Sender<()>>,
}

impl ChromeControl {
    pub(crate) fn new(
        options: ChromeControlOptions,
        settings: Arc<SettingsFacade>,
        secrets: Arc<dyn ChromeSecrets>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                options,
                settings,
                secrets,
                state: Mutex::new(State::default()),
                sessions: Mutex::new(None),
            }),
        }
    }

    /// Opens the pipe and makes Chrome's registration current when the user turned Chrome control
    /// on; an update moves the relay, so the registration is checked at every start.
    pub(crate) fn start(&self) {
        if self.inner.options.register {
            let stopped = stale_relays::stop_others(&self.inner.options.relay_executable);
            if stopped > 0 {
                crate::append_core_log(
                    "chrome.stale_relays_stopped",
                    serde_json::json!({ "count": stopped }),
                );
            }
        }
        if !self.inner.settings.chrome_settings().settings.enabled {
            return;
        }
        if self.inner.options.register && !registration::is_registered(&self.inner.options) {
            let _ = registration::register(&self.inner.options);
        }
        self.listen();
    }

    fn listen(&self) {
        let mut state = self.inner.lock();
        if state.listener.as_ref().is_none_or(tokio::task::JoinHandle::is_finished) {
            state.listener = Some(tokio::spawn(listener::run(Arc::clone(&self.inner))));
        }
    }

    pub(crate) fn status(&self) -> ChromeControlStatus {
        let snapshot = self.inner.settings.chrome_settings();
        let state = self.inner.lock();
        let connection = match (&state.live, snapshot.settings.enabled) {
            (_, false) => ChromeConnectionState::Off,
            (None, true) => ChromeConnectionState::Waiting,
            (Some(live), true) if live.authenticated => ChromeConnectionState::Connected,
            (Some(_), true) => ChromeConnectionState::Pairing,
        };
        ChromeControlStatus {
            enabled: snapshot.settings.enabled,
            revision: snapshot.revision,
            issue: snapshot.issue,
            registered: self.inner.options.register
                && registration::is_registered(&self.inner.options),
            paired: snapshot.settings.paired.map(|paired| ChromePairedExtension {
                extension_id: paired.extension_id,
                paired_at: paired.paired_at,
            }),
            connection,
            pairing_expires_at: state.pairing.as_ref().map(|(_, expires_at)| *expires_at),
        }
    }

    /// Turns Chrome control on or off. Off also forgets the paired extension and removes Chrome's
    /// registration, so nothing of it stays behind.
    pub(crate) async fn set_enabled(
        &self,
        expected_revision: u64,
        enabled: bool,
    ) -> anyhow::Result<ChromeControlStatus> {
        let current = self.inner.settings.chrome_settings().settings;
        let next = ChromeControlSettings {
            enabled,
            paired: if enabled { current.paired.clone() } else { None },
        };
        self.inner.settings.update_chrome_settings(expected_revision, &next).map_err(|error| {
            anyhow::anyhow!("chrome_settings.not_saved: Chrome 设置没有保存：{error}")
        })?;
        if enabled {
            if self.inner.options.register {
                registration::register(&self.inner.options)?;
            }
            self.listen();
        } else {
            if let Some(paired) = current.paired {
                self.inner.forget_secret(&paired);
            }
            let listener = {
                let mut state = self.inner.lock();
                state.pairing = None;
                state.listener.take()
            };
            self.inner.close(true);
            if let Some(listener) = listener {
                listener.abort();
            }
            if self.inner.options.register {
                registration::deregister(&self.inner.options)?;
            }
        }
        Ok(self.status())
    }

    /// A new pairing code; an earlier one stops working.
    pub(crate) fn begin_pairing(&self) -> anyhow::Result<ChromePairingCode> {
        anyhow::ensure!(
            self.inner.settings.chrome_settings().settings.enabled,
            "chrome.off: 先打开 Chrome 控制，再配对。"
        );
        let valid_for = self.inner.options.pairing_valid_for;
        let pending = pairing::PendingCode::issue(Instant::now(), valid_for)?;
        let code = pending.code().to_owned();
        let expires_at = UtcTimestamp::from_value(UtcTimestamp::now().value() + valid_for);
        self.inner.lock().pairing = Some((pending, expires_at));
        Ok(ChromePairingCode { code, expires_at })
    }

    /// Forgets the paired extension: its secret is deleted and a connected extension is told and
    /// disconnected.
    pub(crate) async fn revoke(&self) -> anyhow::Result<ChromeControlStatus> {
        let snapshot = self.inner.settings.chrome_settings();
        if let (Some(revision), Some(paired)) =
            (snapshot.revision, snapshot.settings.paired.clone())
        {
            let next = ChromeControlSettings { paired: None, ..snapshot.settings };
            self.inner.settings.update_chrome_settings(revision, &next).map_err(|error| {
                anyhow::anyhow!("chrome_settings.not_saved: Chrome 设置没有保存：{error}")
            })?;
            self.inner.forget_secret(&paired);
        }
        self.inner.lock().pairing = None;
        self.inner.close(true);
        Ok(self.status())
    }

    /// Sends one command to the connected extension and waits for its outcome; `origin` is the
    /// site it was authorized for. When the wait ends early - a timeout, or the caller dropping
    /// this future - the extension is told to cancel.
    pub(crate) async fn request(
        &self,
        command: serde_json::Value,
        origin: Option<String>,
        timeout: Duration,
    ) -> Result<serde_json::Value, ChromeRequestError> {
        let (id, outgoing, outcome) = {
            let mut state = self.inner.lock();
            state.next_id += 1;
            let id = state.next_id;
            let live = state
                .live
                .as_mut()
                .filter(|live| live.authenticated)
                .ok_or(ChromeRequestError::NotConnected)?;
            let (sender, receiver) = oneshot::channel();
            live.pending.insert(id, sender);
            (id, live.outgoing.clone(), receiver)
        };
        let mut cancel = CancelOnDrop { inner: Arc::clone(&self.inner), id, armed: true };
        if outgoing.send(CoreMessage::Command { id, command, origin }).await.is_err() {
            cancel.armed = false;
            return Err(ChromeRequestError::Disconnected);
        }
        let result = match tokio::time::timeout(timeout, outcome).await {
            Ok(Ok(CommandOutcome::Done { value })) => Ok(value),
            Ok(Ok(CommandOutcome::Failed { code, message })) => {
                Err(ChromeRequestError::Failed { code, message })
            }
            Ok(Err(_)) => Err(ChromeRequestError::Disconnected),
            Err(_) => return Err(ChromeRequestError::TimedOut),
        };
        cancel.armed = false;
        result
    }
}

/// Tells the extension to stop a command whose caller stopped waiting.
struct CancelOnDrop {
    inner: Arc<Inner>,
    id: u64,
    armed: bool,
}

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let mut state = self.inner.lock();
        if let Some(live) = state.live.as_mut()
            && live.pending.remove(&self.id).is_some()
        {
            let _ = live.outgoing.try_send(CoreMessage::Cancel { id: self.id });
        }
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn forget_secret(&self, paired: &ChromePairing) {
        if let Ok(id) = paired.credential_id.parse::<CredentialId>() {
            let _ = self.secrets.delete(id);
        }
    }

    /// Ends the live connection; `revoked` tells the extension to forget its secret first.
    fn close(&self, revoked: bool) {
        let mut state = self.lock();
        if let Some(mut live) = state.live.take() {
            if revoked {
                let _ = live.outgoing.try_send(CoreMessage::Revoked);
            }
            if let Some(close) = live.close.take() {
                let _ = close.send(());
            }
        }
    }

    /// Pairs `extension_id`: a new secret in the Credential Manager, named in the settings. The
    /// secret of an earlier pairing is deleted once the new one is saved.
    fn pair(&self, extension_id: &str) -> anyhow::Result<Vec<u8>> {
        let mut secret = vec![0_u8; PAIRING_SECRET_BYTES];
        getrandom::fill(&mut secret)
            .map_err(|error| anyhow::anyhow!("chrome.pairing_randomness: {error}"))?;
        let id = CredentialId::new();
        self.secrets
            .write(id, extension_id, &aura_windows_platform::SecretBytes::new(secret.clone()))
            .map_err(|error| anyhow::anyhow!("chrome.secret_not_saved: {error}"))?;
        let snapshot = self.settings.chrome_settings();
        let revision =
            snapshot.revision.ok_or_else(|| anyhow::anyhow!("chrome.settings_unavailable"))?;
        let next = ChromeControlSettings {
            paired: Some(ChromePairing {
                extension_id: extension_id.to_owned(),
                credential_id: id.to_string(),
                paired_at: UtcTimestamp::now().to_string(),
            }),
            ..snapshot.settings.clone()
        };
        if let Err(error) = self.settings.update_chrome_settings(revision, &next) {
            let _ = self.secrets.delete(id);
            anyhow::bail!("chrome.settings_not_saved: {error}");
        }
        if let Some(earlier) = snapshot.settings.paired {
            self.forget_secret(&earlier);
        }
        self.lock().pairing = None;
        Ok(secret)
    }

    /// The secret of the extension paired with this installation, when it is `extension_id`.
    fn secret_of(&self, extension_id: &str) -> Option<Vec<u8>> {
        let paired = self.settings.chrome_settings().settings.paired?;
        let id = paired.credential_id.parse::<CredentialId>().ok()?;
        let (owner, secret) = self.secrets.read(id).ok()??;
        (paired.extension_id == extension_id && owner == extension_id)
            .then(|| secret.expose().to_vec())
    }
}
