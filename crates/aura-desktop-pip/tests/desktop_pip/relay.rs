//! The supervisor's relay between Core's pipe and the hosts it starts, with anonymous pipes for
//! Core's pipe and a stand-in host that answers each line it is sent. No window, pointer or key
//! is touched.
use aura_desktop_domain::child::{SupervisorOrder, host_line, supervisor_line};
use aura_desktop_pip::child::{Started, Workers, relay};
use std::io::{self, BufRead, BufReader, PipeReader, PipeWriter, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread::JoinHandle;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

/// Hosts that say `ready <n>` and answer each line with `<n>: <line>`; the line `exit` ends one.
#[derive(Default)]
struct Stand {
    started: AtomicUsize,
    stopped: Arc<AtomicUsize>,
    refuse: bool,
}

impl Workers for Stand {
    fn start(&self) -> io::Result<Started> {
        if self.refuse {
            return Err(io::Error::other("找不到程序"));
        }
        let number = self.started.fetch_add(1, Ordering::SeqCst) + 1;
        let (requests, input) = io::pipe()?;
        let (output, mut replies) = io::pipe()?;
        std::thread::spawn(move || {
            let _ = writeln!(replies, "ready {number}");
            for line in BufReader::new(requests).lines() {
                let Ok(line) = line else { break };
                if line == "exit" {
                    break;
                }
                let _ = writeln!(replies, "{number}: {line}");
            }
        });
        let stopped = Arc::clone(&self.stopped);
        Ok(Started {
            input: Box::new(input),
            output: Box::new(output),
            stop: Box::new(move || {
                stopped.fetch_add(1, Ordering::SeqCst);
            }),
        })
    }
}

struct Core {
    to: Option<PipeWriter>,
    from: BufReader<PipeReader>,
    relay: JoinHandle<io::Result<()>>,
}

impl Core {
    fn start(stand: Arc<Stand>) -> Self {
        let (pipe, to) = io::pipe().expect("Core's side");
        let (from, out) = io::pipe().expect("the supervisor's side");
        let relay = std::thread::spawn(move || relay(pipe, Box::new(out), TOKEN, stand.as_ref()));
        Self { to: Some(to), from: BufReader::new(from), relay }
    }

    fn order(&mut self, order: SupervisorOrder) {
        let to = self.to.as_mut().expect("open");
        to.write_all(&supervisor_line(&order)).expect("order");
    }

    fn request(&mut self, line: &str) {
        let to = self.to.as_mut().expect("open");
        to.write_all(&host_line(line.as_bytes())).expect("request");
    }

    fn raw(&mut self, bytes: &[u8]) {
        self.to.as_mut().expect("open").write_all(bytes).expect("raw");
    }

    fn next(&mut self) -> String {
        let mut line = String::new();
        assert!(self.from.read_line(&mut line).expect("a line") > 0, "the pipe ended");
        line.trim_end_matches('\n').to_owned()
    }

    fn close(mut self) -> (io::Result<()>, Vec<String>) {
        drop(self.to.take());
        let result = self.relay.join().expect("relay thread");
        let rest = self.from.lines().map(|line| line.expect("line")).collect();
        (result, rest)
    }
}

#[test]
fn hello_first_then_the_host_lines_between_started_and_ended() {
    let stand = Arc::new(Stand::default());
    let mut core = Core::start(Arc::clone(&stand));
    assert_eq!(core.next(), format!(r#"S{{"says":"hello","token":"{TOKEN}","protocol":1}}"#));

    core.order(SupervisorOrder::Start);
    assert_eq!(core.next(), r#"S{"says":"started"}"#);
    assert_eq!(core.next(), "Hready 1");
    core.request(r#"{"id":1}"#);
    assert_eq!(core.next(), r#"H1: {"id":1}"#);

    core.order(SupervisorOrder::Stop);
    assert_eq!(core.next(), r#"S{"says":"ended"}"#);
    assert_eq!(stand.stopped.load(Ordering::SeqCst), 1);

    // Without a host a request goes nowhere; the next start is a new host.
    core.request(r#"{"id":2}"#);
    core.order(SupervisorOrder::Start);
    assert_eq!(core.next(), r#"S{"says":"started"}"#);
    assert_eq!(core.next(), "Hready 2");
    core.request(r#"{"id":3}"#);
    assert_eq!(core.next(), r#"H2: {"id":3}"#);

    let (result, rest) = core.close();
    result.expect("served until Core closed the pipe");
    assert_eq!(rest, vec![r#"S{"says":"ended"}"#.to_owned()], "the host ended with the pipe");
    assert_eq!(stand.stopped.load(Ordering::SeqCst), 2);
}

#[test]
fn a_host_that_ends_by_itself_says_so_and_starts_again_on_the_next_order() {
    let stand = Arc::new(Stand::default());
    let mut core = Core::start(Arc::clone(&stand));
    core.next();
    core.order(SupervisorOrder::Start);
    assert_eq!(core.next(), r#"S{"says":"started"}"#);
    assert_eq!(core.next(), "Hready 1");
    core.request("exit");
    assert_eq!(core.next(), r#"S{"says":"ended"}"#);

    core.order(SupervisorOrder::Start);
    assert_eq!(core.next(), r#"S{"says":"started"}"#);
    assert_eq!(core.next(), "Hready 2");
    let (result, _) = core.close();
    result.expect("served");
}

#[test]
fn a_start_while_a_host_runs_ends_that_one_before_the_new_one_starts() {
    let stand = Arc::new(Stand::default());
    let mut core = Core::start(Arc::clone(&stand));
    core.next();
    core.order(SupervisorOrder::Start);
    assert_eq!(core.next(), r#"S{"says":"started"}"#);
    assert_eq!(core.next(), "Hready 1");

    core.order(SupervisorOrder::Start);
    assert_eq!(core.next(), r#"S{"says":"ended"}"#);
    assert_eq!(core.next(), r#"S{"says":"started"}"#);
    assert_eq!(core.next(), "Hready 2");
    assert_eq!(stand.stopped.load(Ordering::SeqCst), 1);
    let (result, _) = core.close();
    result.expect("served");
}

#[test]
fn a_host_that_cannot_start_is_said_with_why() {
    let stand = Arc::new(Stand { refuse: true, ..Stand::default() });
    let mut core = Core::start(stand);
    core.next();
    core.order(SupervisorOrder::Start);
    assert_eq!(core.next(), r#"S{"says":"not_started","why":"找不到程序"}"#);
    let (result, rest) = core.close();
    result.expect("served");
    assert!(rest.is_empty(), "{rest:?}");
}

#[test]
fn lines_that_are_neither_an_order_nor_a_request_are_ignored() {
    let stand = Arc::new(Stand::default());
    let mut core = Core::start(Arc::clone(&stand));
    core.next();
    core.raw(b"X nonsense\n\nS{\"order\":\"dance\"}\nS{\"says\":\"started\"}\n");
    core.order(SupervisorOrder::Start);
    assert_eq!(core.next(), r#"S{"says":"started"}"#);
    assert_eq!(core.next(), "Hready 1");
    let (result, _) = core.close();
    result.expect("served");
    assert_eq!(stand.started.load(Ordering::SeqCst), 1);
}

#[test]
fn the_pipe_closing_before_any_order_ends_the_relay_without_a_host() {
    let stand = Arc::new(Stand::default());
    let mut core = Core::start(Arc::clone(&stand));
    assert!(core.next().starts_with(r#"S{"says":"hello""#));
    let (result, rest) = core.close();
    result.expect("served");
    assert!(rest.is_empty(), "{rest:?}");
    assert_eq!(stand.started.load(Ordering::SeqCst), 0);
}
