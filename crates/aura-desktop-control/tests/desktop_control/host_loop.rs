//! The request loop with a stand-in desktop: one reply per request, in order, with its id.
use aura_desktop_control::host::{Desktop, Output, serve};
use aura_desktop_domain::*;
use serde_json::{Value, json};
use std::io::Cursor;
use std::sync::{Arc, Mutex};

struct StandIn {
    seen: Vec<HostCommand>,
}

impl Desktop for StandIn {
    fn run(&mut self, command: HostCommand) -> HostOutcome {
        self.seen.push(command);
        HostOutcome::Done(HostResult::Windows { windows: Vec::new() })
    }
}

fn run(input: &[u8]) -> (Vec<Value>, Vec<HostCommand>) {
    let written = Arc::new(Mutex::new(Vec::<u8>::new()));
    let output: Output = written.clone();
    let mut desktop = StandIn { seen: Vec::new() };
    serve(&mut desktop, Cursor::new(input.to_vec()), &output).expect("served");
    let text = String::from_utf8(written.lock().expect("lines").clone()).expect("utf-8");
    let lines = text.lines().map(|line| serde_json::from_str(line).expect("json line")).collect();
    (lines, desktop.seen)
}

#[test]
fn the_host_says_its_protocol_first_and_answers_each_request_with_its_id() {
    let input = concat!(
        "{\"id\":3,\"command\":{\"op\":\"windows\",\"apps\":[]}}\n",
        "\n",
        "{\"id\":9,\"command\":{\"op\":\"catalog\"}}\n",
    );
    let (lines, seen) = run(input.as_bytes());
    assert_eq!(lines[0], json!({"type": "ready", "protocol": DESKTOP_PROTOCOL_VERSION}));
    let ids: Vec<&Value> = lines[1..].iter().map(|line| &line["id"]).collect();
    assert_eq!(ids, [&json!(3), &json!(9)], "a blank line is no request");
    assert_eq!(
        seen,
        [HostCommand::Windows { apps: vec![] }, HostCommand::Catalog { manual: vec![] }]
    );
}

#[test]
fn a_line_that_is_no_request_is_refused_with_its_id_and_the_loop_goes_on() {
    let input = concat!(
        "{\"id\":4,\"command\":{\"op\":\"dance\"}}\n",
        "not json\n",
        "{\"id\":5,\"command\":{\"op\":\"windows\",\"apps\":[]}}\n",
    );
    let (lines, seen) = run(input.as_bytes());
    assert_eq!(lines.len(), 4, "{lines:?}");
    for (line, id) in lines[1..3].iter().zip([4, 0]) {
        assert_eq!(line["id"], id);
        assert_eq!(line["outcome"]["failed"]["code"], "invalid_request", "{line}");
        assert_eq!(line["outcome"]["failed"]["delivery"], "not_delivered", "{line}");
    }
    assert_eq!(lines[3]["id"], 5);
    assert_eq!(seen.len(), 1, "only the request reached the desktop");
}

#[test]
fn a_line_longer_than_the_limit_is_refused_without_reading_it_as_a_request() {
    let mut input = vec![b'x'; MAX_LINE_BYTES + 10];
    input.push(b'\n');
    input.extend_from_slice(b"{\"id\":6,\"command\":{\"op\":\"catalog\"}}\n");
    let (lines, seen) = run(&input);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert_eq!(lines[1]["outcome"]["failed"]["code"], "invalid_request");
    assert_eq!(lines[2]["id"], 6);
    assert_eq!(seen.len(), 1);
}
