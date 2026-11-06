//! While Aura keeps the desktop, the user can still work with Aura itself: a click or the wheel
//! on the window of the Aura that started the host is no takeover. The test window starts the
//! host as Core does, so to the host it is Aura; the stand-ins for physical input go to it. These
//! run one at a time only in the owner's idle, unlocked desktop window.
mod support;
use aura_desktop_control::native::input;
use aura_desktop_domain::*;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::time::Duration;
use support::*;

/// One request to the host and its reply; control lines before the reply are skipped.
fn ask(input: &mut impl Write, output: &mut impl BufRead, id: u64, command: Value) -> Value {
    writeln!(input, "{}", json!({"id": id, "command": command})).expect("sent");
    loop {
        let mut line = String::new();
        output.read_line(&mut line).expect("reply");
        let message: Value = serde_json::from_str(&line).expect("json");
        if message["type"] == "reply" && message["id"] == id {
            return message["outcome"].clone();
        }
    }
}

#[test]
#[ignore = "takes the foreground and synthesizes a click and the wheel; owner idle for 10 minutes"]
fn a_click_or_the_wheel_on_auras_own_window_is_no_takeover() {
    for (name, emit) in [("aura-click", "user-click:3000"), ("aura-wheel", "user-wheel:3000")] {
        let mut target = OwnFixture::new(&format!("{name}-target"));
        target.open((760, 420));
        let mut aura = OwnFixture::new(name);
        let mut started = Command::new(&aura.app.id)
            .arg("--log")
            .arg(&aura.log)
            .args(["--at", "60,60", "--emit", emit, "--host", HOST])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("the window that is Aura");
        aura.processes.push(started.id());
        let mut input_line = started.stdin.take().expect("stdin");
        let mut output = BufReader::new(started.stdout.take().expect("stdout"));
        let mut line = String::new();
        output.read_line(&mut line).expect("ready");
        assert!(
            aura.wait_for(|events| events.iter().any(|e| e == "ready"), Duration::from_secs(20))
        );
        let on_aura = center(aura.place("client"));
        input::move_to(
            aura_desktop_control::native::screen::virtual_screen(),
            on_aura.x,
            on_aura.y,
        );
        let wait = HostCommand::Guarded {
            command: Box::new(HostCommand::Input {
                app: target.app.clone(),
                steps: vec![InputStep::Wait { ms: 6_000 }],
                window: None,
                system_keys: false,
                clipboard: false,
            }),
            hold: Some(format!("h-{name}")),
            stop_keys: Some(pip::PIP_STOP_KEYS.into()),
        };
        let wait = serde_json::to_value(&wait).expect("command");
        let outcome = ask(&mut input_line, &mut output, 1, wait);
        let typed = HostOutcome::Done(HostResult::Input { steps_done: 1 });
        assert_eq!(outcome, serde_json::to_value(typed).expect("outcome"), "{name}: {outcome}");
        let tick = aura.events().into_iter().any(|e| e.starts_with("test-input-tick "));
        assert!(tick, "{name}: the stand-in input was sent");
        drop(input_line);
        let _ = started.kill();
        let _ = started.wait();
    }
}
