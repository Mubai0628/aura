//! A bounded model-facing view of command output, separate from the private
//! Runner receipt. Exit status and side-effect evidence remain authoritative.
use aura_windows_user_contract::{RunnerObservation, RunnerStopReason};

const STREAM_BYTES: usize = 2048;

fn excerpt(bytes: &[u8]) -> String {
    let mut text = String::new();
    let prefix = &bytes[..bytes.len().min(STREAM_BYTES)];
    let mut truncated = bytes.len() > prefix.len();
    for ch in String::from_utf8_lossy(prefix).chars() {
        if ch.is_control() && !matches!(ch, '\n' | '\r' | '\t') {
            continue;
        }
        if text.len() + ch.len_utf8() > STREAM_BYTES {
            truncated = true;
            break;
        }
        text.push(ch);
    }
    if truncated {
        text.push_str("\n[output excerpt truncated]");
    }
    text
}

pub(super) fn failure_summary(observation: &RunnerObservation) -> Option<String> {
    let reason = match observation.stop_reason {
        RunnerStopReason::Exited if observation.exit_code != Some(0) => "nonzero exit",
        RunnerStopReason::OutputLimit => "output limit reached",
        _ => return None,
    };
    let code = observation.exit_code.map_or_else(|| "unavailable".into(), |v| v.to_string());
    Some(format!(
        "Command failed: {reason}; exit code: {code}. Recorded side effects remain authoritative; do not retry automatically.\nCommand stdout excerpt:\n{}\nCommand stderr excerpt:\n{}",
        excerpt(&observation.stdout),
        excerpt(&observation.stderr)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excerpts_bound_invalid_utf8_and_remove_terminal_controls() {
        assert_eq!(excerpt(b"out\n\t\x00\x1berror\xff"), "out\n\terror\u{fffd}");
        let value = excerpt(&vec![0xff; 6000]);
        assert!(value.ends_with("[output excerpt truncated]"));
        assert!(value.len() < STREAM_BYTES + 30);
        assert!(excerpt(&"中".repeat(3000).into_bytes()).is_char_boundary(2046));
    }
}
