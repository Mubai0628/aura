//! Test Hook: reads the whole JSON envelope from stdin, then answers as told.
//!
//! `answer <json>` prints the JSON; `set-argument <key> <value>` returns the tool arguments
//! with one field set; `sleep <ms>` sleeps before continuing; `exit <code>` exits without
//! answering; `stderr <text>` writes to stderr and continues; `ignore-input` answers
//! without reading stdin; `block-once <reason>` blocks only while `payload.blocks_so_far` is 0;
//! anything else continues.
use serde_json::{Value, json};
use std::io::{self, Read as _, Write as _};

fn main() -> io::Result<()> {
    let arguments = std::env::args().skip(1).collect::<Vec<_>>();
    let mode = arguments.first().map(String::as_str).unwrap_or("continue");
    let mut input = String::new();
    if mode != "ignore-input" {
        io::stdin().read_to_string(&mut input)?;
    }
    let envelope: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
    let answer = match mode {
        "answer" => arguments.get(1).cloned().unwrap_or_default(),
        "set-argument" => {
            let mut arguments_value =
                envelope["payload"]["arguments"].as_object().cloned().unwrap_or_default();
            arguments_value.insert(
                arguments.get(1).cloned().unwrap_or_default(),
                json!(arguments.get(2).cloned().unwrap_or_default()),
            );
            json!({"decision": "transform", "arguments": arguments_value}).to_string()
        }
        // Sends a held answer back once, then accepts it (`turn.before_complete`).
        "block-once" => {
            if envelope["payload"]["blocks_so_far"].as_u64() == Some(0) {
                json!({"decision": "block", "reason": arguments.get(1).cloned().unwrap_or_default()})
                    .to_string()
            } else {
                json!({"decision": "continue"}).to_string()
            }
        }
        "sleep" => {
            let millis = arguments.get(1).and_then(|value| value.parse().ok()).unwrap_or(0);
            std::thread::sleep(std::time::Duration::from_millis(millis));
            json!({"decision": "continue"}).to_string()
        }
        "exit" => {
            std::process::exit(arguments.get(1).and_then(|value| value.parse().ok()).unwrap_or(1))
        }
        "stderr" => {
            io::stderr().write_all(arguments.get(1).cloned().unwrap_or_default().as_bytes())?;
            json!({"decision": "continue"}).to_string()
        }
        _ => json!({"decision": "continue"}).to_string(),
    };
    let mut stdout = io::stdout().lock();
    stdout.write_all(answer.as_bytes())?;
    stdout.flush()
}
