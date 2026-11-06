use aura_desktop_domain::pip::{
    PIP_DESKTOP, PIP_STOP_KEYS, PipCommand, PipMessage, PipOptions, stop_chord,
};

#[test]
fn window_options_round_trip_and_reject_ambiguous_or_out_of_range_input() {
    let defaults = PipOptions::default();
    assert_eq!((defaults.width, defaults.height), PIP_DESKTOP);
    assert_eq!(defaults.stop_keys, PIP_STOP_KEYS);
    assert_eq!(PipOptions::parse(&defaults.arguments()).expect("default options"), defaults);
    for words in [
        vec!["--size", "639x800"],
        vec!["--size", "1280x2161"],
        vec!["--size"],
        vec!["--unknown", "x"],
        vec!["--size", "1280x800", "--size", "640x480"],
    ] {
        assert!(
            PipOptions::parse(&words.iter().map(ToString::to_string).collect::<Vec<_>>()).is_err(),
            "{words:?}"
        );
    }
}

#[test]
fn stop_keys_require_a_modifier_and_do_not_steal_system_chords() {
    for keys in ["Ctrl+Alt+Q", "Ctrl+Shift+Q", "Alt+F9"] {
        assert!(stop_chord(keys).is_ok(), "{keys}");
    }
    for keys in ["Q", "Ctrl", "Shift+Q", "Alt+Tab", "Ctrl+Alt+Delete", "Win+L"] {
        assert!(stop_chord(keys).is_err(), "{keys}");
    }
}

#[test]
fn window_messages_are_strict_and_round_trip() {
    for message in [
        PipMessage::Ready { protocol: 1 },
        PipMessage::SignedIn { session: 3 },
        PipMessage::TakenOver,
        PipMessage::Closed { signed_out: false },
        PipMessage::Disconnected { reason: 7 },
    ] {
        assert_eq!(
            serde_json::from_str::<PipMessage>(
                &serde_json::to_string(&message).expect("encode message")
            )
            .expect("decode message"),
            message
        );
    }
    for input in
        [r#"{"op":"hold","on":true,"extra":1}"#, r#"{"op":"hold","on":"true"}"#, r#"{"op":"run"}"#]
    {
        assert!(serde_json::from_str::<PipCommand>(input).is_err());
    }
}
