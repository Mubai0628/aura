//! The child session pipe's lines: whose each is, by its first byte, and what the supervisor says
//! and is told.
use aura_desktop_domain::child::{
    CHILD_PIPE_PREFIX, CHILD_PROTOCOL_VERSION, ChildLine, SupervisorOrder, SupervisorSays,
    host_line, read_child_line, supervisor_line, valid_child_pipe, valid_child_token,
};

#[test]
fn only_the_pipe_names_and_tokens_core_makes_are_taken() {
    let part = "0123456789abcdef";
    assert!(valid_child_pipe(&format!("{CHILD_PIPE_PREFIX}{part}")));
    assert!(valid_child_pipe(&format!("{CHILD_PIPE_PREFIX}{}", "a".repeat(64))));
    for name in [
        format!("{CHILD_PIPE_PREFIX}{}", "a".repeat(15)),
        format!("{CHILD_PIPE_PREFIX}{}", "a".repeat(65)),
        format!("{CHILD_PIPE_PREFIX}0123456789ABCDEF"),
        format!("{CHILD_PIPE_PREFIX}0123456789abcdeg"),
        format!(r"\\.\pipe\aura-other-{part}"),
        format!(r"\\remote\pipe\aura-desktop-child-{part}"),
        format!("{CHILD_PIPE_PREFIX}{part}\\..\\x"),
    ] {
        assert!(!valid_child_pipe(&name), "{name}");
    }
    assert!(valid_child_token(&"f".repeat(32)));
    assert!(valid_child_token(&"0".repeat(128)));
    for token in ["f".repeat(31), "f".repeat(129), "F".repeat(32), format!("{}-", "f".repeat(32))] {
        assert!(!valid_child_token(&token), "{token}");
    }
}

fn without_break(line: &[u8]) -> &[u8] {
    line.strip_suffix(b"\n").expect("a line ends with its break")
}

#[test]
fn a_host_line_passes_through_unchanged_after_its_byte() {
    let line = br#"{"type":"ready","protocol":4}"#;
    let framed = host_line(line);
    assert_eq!(framed[0], b'H');
    let read = read_child_line::<SupervisorSays>(without_break(&framed)).expect("a host line");
    assert_eq!(read, ChildLine::Host(&line[..]));
}

#[test]
fn what_the_supervisor_says_and_is_told_reads_back() {
    for says in [
        SupervisorSays::Hello { token: "abc123".into(), protocol: CHILD_PROTOCOL_VERSION },
        SupervisorSays::Started,
        SupervisorSays::NotStarted { why: "找不到程序".into() },
        SupervisorSays::Ended,
    ] {
        let framed = supervisor_line(&says);
        assert_eq!(framed[0], b'S');
        let read = read_child_line::<SupervisorSays>(without_break(&framed)).expect("says");
        assert_eq!(read, ChildLine::Supervisor(says));
    }
    for order in [SupervisorOrder::Start, SupervisorOrder::Stop] {
        let framed = supervisor_line(&order);
        let read = read_child_line::<SupervisorOrder>(without_break(&framed)).expect("order");
        assert_eq!(read, ChildLine::Supervisor(order));
    }
    assert_eq!(
        String::from_utf8(supervisor_line(&SupervisorOrder::Start)).expect("text"),
        "S{\"order\":\"start\"}\n"
    );
}

#[test]
fn other_lines_are_refused() {
    assert!(read_child_line::<SupervisorSays>(b"").is_err());
    assert!(read_child_line::<SupervisorSays>(b"X{}").is_err());
    assert!(read_child_line::<SupervisorSays>(br#"S{"says":"shout"}"#).is_err());
    assert!(read_child_line::<SupervisorSays>(br#"S{"says":"hello","token":"a"}"#).is_err());
    assert!(
        read_child_line::<SupervisorSays>(br#"S{"says":"hello","token":"a","protocol":1,"x":1}"#)
            .is_err()
    );
    // An order is not something the supervisor says, nor the other way round.
    assert!(read_child_line::<SupervisorSays>(br#"S{"order":"start"}"#).is_err());
    assert!(read_child_line::<SupervisorOrder>(br#"S{"says":"ended"}"#).is_err());
}
