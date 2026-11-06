//! One started service: the lines Aura sends it, and the thread that reads everything it writes.
//! A line is answered on that thread or, for a request, by Core's runtime; nothing it writes waits
//! on anything else.
use crate::host::{Inner, MAX_IN_FLIGHT};
use crate::launch::ServiceProcess;
use crate::lines::{ErrorLines, Line, LineReader};
use crate::protocol::{
    ChannelAbilities, FromService, MAX_CHANNEL_TEXT_CHARS, MAX_LINE_BYTES, PROTOCOL_VERSION,
    RequestId, ServiceError, ToService, parse_line, shown,
};
use crate::redact::{Admit, LogGate, Redactor};
use aura_backend_windows_user::program::SessionEvent;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Instant;

const STDERR_LINE_CHARS: usize = 4_000;

pub(crate) struct Instance {
    pub generation: u64,
    pub plugin: String,
    pub started: Instant,
    process: Mutex<Option<Box<dyn ServiceProcess>>>,
    in_flight: Mutex<BTreeSet<RequestId>>,
    ready: AtomicBool,
    /// The channels its plugin declares; it may report only these.
    declared: Vec<String>,
    redactor: Redactor,
    gate: Mutex<LogGate>,
    /// Why Aura stopped it, when Aura did.
    stopped_because: Mutex<Option<String>>,
}

/// Why a line could not be queued.
pub(crate) enum Unsent {
    /// The service stopped reading its standard input.
    Stuck,
    Gone,
}

impl Instance {
    pub fn new(
        generation: u64,
        plugin: String,
        started: Instant,
        process: Box<dyn ServiceProcess>,
        declared: Vec<String>,
        redactor: Redactor,
    ) -> Self {
        Self {
            generation,
            plugin,
            started,
            process: Mutex::new(Some(process)),
            in_flight: Mutex::new(BTreeSet::new()),
            ready: AtomicBool::new(false),
            declared,
            redactor,
            gate: Mutex::new(LogGate::default()),
            stopped_because: Mutex::new(None),
        }
    }

    pub fn send(&self, message: &ToService) -> Result<(), Unsent> {
        let process = self.process.lock().map_err(|_| Unsent::Gone)?;
        let process = process.as_ref().ok_or(Unsent::Gone)?;
        process.send(message.encode()).map_err(|error| {
            if error.to_string().contains("backpressure") { Unsent::Stuck } else { Unsent::Gone }
        })
    }

    /// Takes the process out, to be stopped outside any lock; `why` is kept when Aura stops it.
    pub fn take(&self, why: Option<String>) -> Option<Box<dyn ServiceProcess>> {
        if let Some(why) = why
            && let Ok(mut because) = self.stopped_because.lock()
            && because.is_none()
        {
            *because = Some(why);
        }
        self.process.lock().ok()?.take()
    }

    pub fn stopped_because(&self) -> Option<String> {
        self.stopped_because.lock().ok()?.clone()
    }

    fn report(&self, code: &str, message: String) -> Result<(), Unsent> {
        self.send(&ToService::Error { code: code.to_owned(), message })
    }

    fn answer(&self, id: RequestId, error: ServiceError) -> Result<(), Unsent> {
        self.send(&ToService::answer(id, Err(error)))
    }
}

/// Reads everything the service writes until it ended.
pub(crate) fn follow(
    inner: Arc<Inner>,
    instance: Arc<Instance>,
    events: mpsc::Receiver<SessionEvent>,
) {
    let mut lines = LineReader::new(MAX_LINE_BYTES);
    let mut errors = ErrorLines::new(STDERR_LINE_CHARS);
    let mut count = 0_u64;
    let mut exit = None;
    for event in events {
        match event {
            SessionEvent::Stdout(bytes) => {
                for line in lines.push(&bytes) {
                    count += 1;
                    if let Err(Unsent::Stuck) = read_line(&inner, &instance, count, line) {
                        inner.stuck(&instance);
                    }
                }
            }
            SessionEvent::Stderr(bytes) => {
                for line in errors.push(&bytes) {
                    log(&inner, &instance, "stderr", &line);
                }
            }
            SessionEvent::Ended { exit_code, memory_limit_reached, .. } => {
                exit = Some((exit_code, memory_limit_reached));
                break;
            }
        }
    }
    if let Some(line) = errors.finish() {
        log(&inner, &instance, "stderr", &line);
    }
    let (exit_code, memory) = exit.unwrap_or((None, false));
    inner.ended(&instance, exit_code, memory);
}

/// Keeps one line in the plugin's log, without the secrets and within the limit.
pub(crate) fn log(inner: &Inner, instance: &Instance, part: &str, text: &str) {
    let admit = match instance.gate.lock() {
        Ok(mut gate) => gate.admit(inner.clock.now()),
        Err(_) => return,
    };
    match admit {
        Admit::Drop => {}
        Admit::Keep => inner.calls.log(&instance.plugin, part, &instance.redactor.hide(text)),
        Admit::KeepAfter { dropped } => {
            inner.calls.log(
                &instance.plugin,
                "aura",
                &format!("上一分钟有 {dropped} 行日志超过上限，没有记"),
            );
            inner.calls.log(&instance.plugin, part, &instance.redactor.hide(text));
        }
    }
}

fn read_line(
    inner: &Arc<Inner>,
    instance: &Arc<Instance>,
    count: u64,
    line: Line,
) -> Result<(), Unsent> {
    let Line::Text(line) = line else {
        return instance
            .report("protocol.line_too_long", format!("第 {count} 行超过 1 MiB，没有读"));
    };
    match parse_line(&line) {
        Err(problem) => match problem.id {
            Some(id) => instance.answer(id, ServiceError::new(problem.code, problem.message)),
            None => instance.report(problem.code, format!("第 {count} 行：{}", problem.message)),
        },
        Ok(FromService::Log { level, text }) => {
            log(inner, instance, level.name(), &text);
            Ok(())
        }
        Ok(FromService::Ready { protocol, channels }) => ready(inner, instance, protocol, channels),
        Ok(FromService::Request { id, method, params }) => {
            if !instance.ready.load(Ordering::Acquire) {
                return instance.answer(
                    id,
                    ServiceError::new("protocol.not_ready", "先发 ready，Aura 才处理请求"),
                );
            }
            {
                let Ok(mut in_flight) = instance.in_flight.lock() else { return Err(Unsent::Gone) };
                if in_flight.contains(&id) {
                    drop(in_flight);
                    return instance.answer(
                        id,
                        ServiceError::new("protocol.duplicate_id", "这个 id 的请求还没有回应"),
                    );
                }
                if in_flight.len() >= MAX_IN_FLIGHT {
                    drop(in_flight);
                    return instance.answer(
                        id,
                        ServiceError::new(
                            "protocol.busy",
                            format!("同时在处理的请求最多 {MAX_IN_FLIGHT} 个，等前面的回应了再发"),
                        ),
                    );
                }
                in_flight.insert(id.clone());
            }
            let call = inner.calls.call(&instance.plugin, &method, params);
            let (inner, instance) = (Arc::clone(inner), Arc::clone(instance));
            inner.runtime.clone().spawn(async move {
                let outcome = call.await;
                if let Ok(mut in_flight) = instance.in_flight.lock() {
                    in_flight.remove(&id);
                }
                if let Err(Unsent::Stuck) = instance.send(&ToService::answer(id, outcome)) {
                    inner.stuck(&instance);
                }
            });
            Ok(())
        }
    }
}

fn ready(
    inner: &Inner,
    instance: &Instance,
    protocol: u64,
    reported: Vec<ChannelAbilities>,
) -> Result<(), Unsent> {
    if instance.ready.load(Ordering::Acquire) {
        return instance.report("protocol.bad_message", "已经发过 ready 了".into());
    }
    if protocol != u64::from(PROTOCOL_VERSION) {
        let why = format!("服务说的协议版本是 {protocol}，这个 Aura 只认 {PROTOCOL_VERSION}");
        let sent = instance.report("protocol.version", why.clone());
        inner.stop_for_good(instance, why);
        return sent;
    }
    let mut channels: Vec<ChannelAbilities> = Vec::new();
    for channel in reported {
        let problem = if !instance.declared.contains(&channel.id) {
            Some(format!("渠道 {} 没有在清单的 [[channels]] 里声明，不认", shown(&channel.id)))
        } else if channels.iter().any(|known| known.id == channel.id) {
            Some(format!("渠道 {} 报了两次", channel.id))
        } else if !(1..=MAX_CHANNEL_TEXT_CHARS).contains(&channel.max_text_chars) {
            Some(format!(
                "渠道 {} 的 max_text_chars 要在 1 到 {MAX_CHANNEL_TEXT_CHARS} 之间",
                channel.id
            ))
        } else {
            None
        };
        match problem {
            Some(message) => instance.report("protocol.bad_message", message)?,
            None => channels.push(channel),
        }
    }
    instance.ready.store(true, Ordering::Release);
    inner.running(instance, &channels);
    Ok(())
}
