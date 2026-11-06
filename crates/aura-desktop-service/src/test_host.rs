//! A host a test plays: it says it is ready, answers each request at once as the test's script
//! says, and records what it was sent. No window, pointer or key is touched.
use crate::launch::{HostEvent, HostLauncher, HostProcess};
use aura_desktop_domain::{
    DESKTOP_PROTOCOL_VERSION, HostCommand, HostMessage, HostOutcome, HostRequest,
};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

type Script = dyn Fn(&HostCommand) -> HostOutcome + Send + Sync;
type Notices = dyn Fn(&HostCommand) -> Vec<HostMessage> + Send + Sync;

pub struct ScriptedHost {
    script: Box<Script>,
    sent: Mutex<Vec<HostCommand>>,
    requests: Mutex<Vec<HostCommand>>,
    before: Mutex<Option<Arc<Notices>>>,
}

impl ScriptedHost {
    pub fn new(script: impl Fn(&HostCommand) -> HostOutcome + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            script: Box::new(script),
            sent: Mutex::new(Vec::new()),
            requests: Mutex::new(Vec::new()),
            before: Mutex::new(None),
        })
    }

    /// What the host was sent, in order, each action out of its hold.
    #[must_use]
    pub fn sent(&self) -> Vec<HostCommand> {
        self.sent.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// What the host was sent, in order, as it came.
    #[must_use]
    pub fn requests(&self) -> Vec<HostCommand> {
        self.requests.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone()
    }

    /// From now on the host says what `notices` gives for a request before it answers it, as
    /// the real host says a hold it gave back before its next reply.
    pub fn say_before(
        &self,
        notices: impl Fn(&HostCommand) -> Vec<HostMessage> + Send + Sync + 'static,
    ) {
        *self.before.lock().unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(Arc::new(notices));
    }
}

pub struct Played(pub Arc<ScriptedHost>);

struct Process {
    host: Arc<ScriptedHost>,
    events: mpsc::UnboundedSender<HostEvent>,
}

fn line(message: &HostMessage) -> HostEvent {
    HostEvent::Line(serde_json::to_vec(message).unwrap_or_default())
}

impl HostLauncher for Played {
    fn start(
        &self,
        events: mpsc::UnboundedSender<HostEvent>,
    ) -> Result<Box<dyn HostProcess>, String> {
        let _ = events.send(line(&HostMessage::Ready { protocol: DESKTOP_PROTOCOL_VERSION }));
        Ok(Box::new(Process { host: Arc::clone(&self.0), events }))
    }
}

impl HostProcess for Process {
    fn send(&self, bytes: Vec<u8>) -> anyhow::Result<()> {
        let request: HostRequest = serde_json::from_slice(&bytes)?;
        self.host
            .requests
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(request.command.clone());
        let before = self.host.before.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let notices = before.as_ref().map(|notices| notices(&request.command));
        drop(before);
        let command = match request.command {
            HostCommand::Guarded { command, .. } => *command,
            command => command,
        };
        self.host
            .sent
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(command.clone());
        for notice in notices.unwrap_or_default() {
            let _ = self.events.send(line(&notice));
        }
        let outcome = (self.host.script)(&command);
        let _ = self.events.send(line(&HostMessage::Reply { id: request.id, outcome }));
        Ok(())
    }

    fn stop(&mut self) {}
}
