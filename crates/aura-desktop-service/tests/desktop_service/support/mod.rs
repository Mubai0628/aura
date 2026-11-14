//! A stand-in for the desktop host: it says it is ready (or not), then answers each request as
//! the test's script says, and records what it was sent.
#![allow(dead_code)]

pub mod turns;

use aura_desktop_domain::{
    DESKTOP_PROTOCOL_VERSION, HostCommand, HostMessage, HostOutcome, HostRequest,
};
use aura_desktop_service::{HostEvent, HostLauncher, HostProcess};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;

/// How the stand-in answers one request.
pub enum Answer {
    /// Says how many steps are done, then answers a little later.
    Progress {
        app_id: String,
        steps_done: u32,
        outcome: HostOutcome,
    },
    Now(HostOutcome),
    /// Says the messages, then answers.
    Saying(Vec<HostMessage>, HostOutcome),
    After(Duration, HostOutcome),
    /// Never answers.
    Silent,
    /// Ends, as a host that crashed.
    Crash,
}

type Script = dyn Fn(&HostCommand) -> Answer + Send + Sync;

pub struct Stand {
    /// The protocol of its first line; `None`: it never says it is ready.
    pub ready: Option<u32>,
    script: Box<Script>,
    pub starts: AtomicUsize,
    pub stops: AtomicUsize,
    pub sent: Mutex<Vec<HostCommand>>,
    in_flight: AtomicUsize,
    pub most_in_flight: AtomicUsize,
    /// The running host's lines to Core.
    lines: Mutex<Option<mpsc::UnboundedSender<HostEvent>>>,
}

impl Stand {
    pub fn new(script: impl Fn(&HostCommand) -> Answer + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self {
            ready: Some(DESKTOP_PROTOCOL_VERSION),
            script: Box::new(script),
            starts: AtomicUsize::new(0),
            stops: AtomicUsize::new(0),
            sent: Mutex::new(Vec::new()),
            in_flight: AtomicUsize::new(0),
            most_in_flight: AtomicUsize::new(0),
            lines: Mutex::new(None),
        })
    }

    pub fn never_ready(
        script: impl Fn(&HostCommand) -> Answer + Send + Sync + 'static,
    ) -> Arc<Self> {
        let mut stand = Arc::into_inner(Self::new(script)).expect("new");
        stand.ready = None;
        Arc::new(stand)
    }

    pub fn of_protocol(protocol: u32) -> Arc<Self> {
        let mut stand = Arc::into_inner(Self::new(|_| Answer::Silent)).expect("new");
        stand.ready = Some(protocol);
        Arc::new(stand)
    }

    pub fn starts(&self) -> usize {
        self.starts.load(Ordering::SeqCst)
    }

    pub fn stops(&self) -> usize {
        self.stops.load(Ordering::SeqCst)
    }

    pub fn sent(&self) -> Vec<HostCommand> {
        self.sent.lock().expect("sent").clone()
    }

    /// The running host says `message` by itself, between replies.
    pub fn say(&self, message: &HostMessage) {
        let lines = self.lines.lock().expect("lines");
        let _ = lines.as_ref().expect("a running host").send(line(message));
    }
}

pub struct Launcher(pub Arc<Stand>);

struct Process {
    stand: Arc<Stand>,
    events: mpsc::UnboundedSender<HostEvent>,
}

fn line(message: &HostMessage) -> HostEvent {
    HostEvent::Line(serde_json::to_vec(message).expect("line"))
}

impl HostLauncher for Launcher {
    fn start(
        &self,
        events: mpsc::UnboundedSender<HostEvent>,
    ) -> Result<Box<dyn HostProcess>, String> {
        self.0.starts.fetch_add(1, Ordering::SeqCst);
        *self.0.lines.lock().expect("lines") = Some(events.clone());
        if let Some(protocol) = self.0.ready {
            let _ = events.send(line(&HostMessage::Ready { protocol }));
        }
        Ok(Box::new(Process { stand: Arc::clone(&self.0), events }))
    }
}

impl HostProcess for Process {
    fn send(&self, line_bytes: Vec<u8>) -> anyhow::Result<()> {
        let request: HostRequest = serde_json::from_slice(&line_bytes)?;
        self.stand.sent.lock().expect("sent").push(request.command.clone());
        let now = self.stand.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.stand.most_in_flight.fetch_max(now, Ordering::SeqCst);
        let reply = |outcome| line(&HostMessage::Reply { id: request.id, outcome });
        let stand = Arc::clone(&self.stand);
        let events = self.events.clone();
        match (self.stand.script)(&request.command) {
            Answer::Progress { app_id, steps_done, outcome } => {
                let _ =
                    events.send(line(&HostMessage::Control { id: request.id, app_id, steps_done }));
                let event = reply(outcome);
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(200));
                    stand.in_flight.fetch_sub(1, Ordering::SeqCst);
                    let _ = events.send(event);
                });
            }
            Answer::Now(outcome) => {
                stand.in_flight.fetch_sub(1, Ordering::SeqCst);
                let _ = events.send(reply(outcome));
            }
            Answer::Saying(messages, outcome) => {
                for message in &messages {
                    let _ = events.send(line(message));
                }
                stand.in_flight.fetch_sub(1, Ordering::SeqCst);
                let _ = events.send(reply(outcome));
            }
            Answer::After(wait, outcome) => {
                let event = reply(outcome);
                std::thread::spawn(move || {
                    std::thread::sleep(wait);
                    stand.in_flight.fetch_sub(1, Ordering::SeqCst);
                    let _ = events.send(event);
                });
            }
            Answer::Silent => {}
            Answer::Crash => {
                stand.in_flight.fetch_sub(1, Ordering::SeqCst);
                let _ = events.send(HostEvent::Ended);
            }
        }
        Ok(())
    }

    fn stop(&mut self) {
        self.stand.stops.fetch_add(1, Ordering::SeqCst);
        self.stand.in_flight.store(0, Ordering::SeqCst);
    }
}
