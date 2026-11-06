//! The desktop host's lines as Core and the host write and read them.
use aura_desktop_domain::*;
use serde_json::json;

#[test]
fn a_request_and_its_reply_read_back_as_written() {
    let app = AppEntry {
        id: "Microsoft.WindowsNotepad_8wekyb3d8bbwe!App".into(),
        name: "记事本".into(),
        kind: AppKind::Packaged,
        path: None,
    };
    let request = HostRequest { id: 7, command: HostCommand::Launch { app, timeout_ms: 15_000 } };
    let line = serde_json::to_value(&request).expect("written");
    assert_eq!(line["command"]["op"], "launch");
    assert_eq!(line["command"]["app"]["kind"], "packaged");
    assert!(line["command"]["app"].get("path").is_none(), "{line}");
    assert_eq!(serde_json::from_value::<HostRequest>(line).expect("read"), request);

    let reply = HostMessage::Reply {
        id: 7,
        outcome: HostOutcome::Failed(HostError::new(
            HostErrorCode::LaunchFailed,
            "没有出现新窗口",
            Delivery::Uncertain,
        )),
    };
    let line = serde_json::to_value(&reply).expect("written");
    assert_eq!(
        line,
        json!({"type": "reply", "id": 7, "outcome": {"failed": {
            "code": "launch_failed", "message": "没有出现新窗口", "delivery": "uncertain"}}})
    );
    assert_eq!(serde_json::from_value::<HostMessage>(line).expect("read"), reply);
}

#[test]
fn a_line_with_a_field_no_one_wrote_is_refused() {
    let line = json!({"id": 1, "command": {"op": "catalog", "manual": [], "extra": true}});
    assert!(serde_json::from_value::<HostRequest>(line).is_err());
    let line = json!({"type": "ready", "protocol": 1, "pid": 4});
    assert!(serde_json::from_value::<HostMessage>(line).is_err());
}

#[test]
fn a_window_ref_names_its_handle_and_nothing_else() {
    assert_eq!(window_ref(1312), "w1312");
    assert_eq!(window_handle("w1312"), Some(1312));
    for bad in ["1312", "w", "w0", "wx", "e12", "w-"] {
        assert_eq!(window_handle(bad), None, "{bad}");
    }
}

#[test]
fn a_rectangle_holds_its_top_left_and_not_its_far_edges() {
    let rect = Rect { x: -1920, y: 0, width: 1920, height: 1080 };
    assert!(rect.contains(-1920, 0));
    assert!(rect.contains(-1, 1079));
    assert!(!rect.contains(0, 0));
    assert!(!rect.contains(-1, 1080));
    assert!(Rect { width: 0, ..rect }.is_empty());
}

#[test]
fn a_window_command_names_its_action_and_no_other() {
    let app =
        AppEntry {
            id: "notepad".into(), name: "记事本".into(), kind: AppKind::Manual, path: None
        };
    for (action, name) in [
        (WindowAction::Front, "front"),
        (WindowAction::Minimize, "minimize"),
        (WindowAction::Maximize, "maximize"),
        (WindowAction::Restore, "restore"),
        (WindowAction::Close, "close"),
    ] {
        let request = HostRequest {
            id: 3,
            command: HostCommand::Window { app: app.clone(), window: None, action },
        };
        let line = serde_json::to_value(&request).expect("written");
        assert_eq!(
            (&line["command"]["op"], &line["command"]["action"]),
            (&json!("window"), &json!(name))
        );
        assert_eq!(serde_json::from_value::<HostRequest>(line).expect("read"), request);
    }
    let shaken = json!({"id": 3, "command": {"op": "window", "action": "shake",
        "app": {"id": "notepad", "name": "记事本", "kind": "manual"}}});
    assert!(serde_json::from_value::<HostRequest>(shaken).is_err());
}

#[test]
fn an_action_of_a_turn_names_its_hold_and_the_host_says_when_it_gave_one_back() {
    let app =
        AppEntry {
            id: "notepad".into(), name: "记事本".into(), kind: AppKind::Manual, path: None
        };
    let action = HostCommand::Window { app, window: None, action: WindowAction::Front };
    let held = HostRequest {
        id: 4,
        command: HostCommand::Guarded {
            command: Box::new(action.clone()),
            hold: Some("trn_1".into()),
            stop_keys: Some("Ctrl+Alt+Q".into()),
        },
    };
    let line = serde_json::to_value(&held).expect("written");
    assert_eq!(
        (&line["command"]["op"], &line["command"]["hold"], &line["command"]["stop_keys"]),
        (&json!("guarded"), &json!("trn_1"), &json!("Ctrl+Alt+Q"))
    );
    assert_eq!(serde_json::from_value::<HostRequest>(line).expect("read"), held);
    let own = HostCommand::Guarded { command: Box::new(action), hold: None, stop_keys: None };
    let line = serde_json::to_value(&own).expect("written");
    assert!(line.get("hold").is_none() && line.get("stop_keys").is_none(), "{line}");
    assert_eq!(serde_json::from_value::<HostCommand>(line).expect("read"), own);
    let release = HostCommand::Release { hold: "trn_1".into() };
    assert_eq!(
        serde_json::to_value(&release).expect("written"),
        json!({"op": "release", "hold": "trn_1"})
    );
    for (reason, name) in [(HoldEnd::TakenOver, "taken_over"), (HoldEnd::Idle, "idle")] {
        let ended = HostMessage::HoldEnded { hold: "trn_1".into(), reason, cause: None };
        let line = serde_json::to_value(&ended).expect("written");
        assert_eq!(line, json!({"type": "hold_ended", "hold": "trn_1", "reason": name}));
        assert_eq!(serde_json::from_value::<HostMessage>(line).expect("read"), ended);
    }
    let clicked = HostMessage::HoldEnded {
        hold: "trn_1".into(),
        reason: HoldEnd::TakenOver,
        cause: Some("点了鼠标".into()),
    };
    let line = serde_json::to_value(&clicked).expect("written");
    assert_eq!(line["cause"], json!("点了鼠标"));
    assert_eq!(serde_json::from_value::<HostMessage>(line).expect("read"), clicked);
    let paused = json!({"op": "guarded", "pause_on_input": true,
        "command": {"op": "displays"}});
    assert!(serde_json::from_value::<HostCommand>(paused).is_err(), "no pause any more");
    assert_eq!(
        serde_json::to_value(HostResult::Released).expect("written"),
        json!({"kind": "released"})
    );
}

#[test]
fn a_turn_keeps_its_desktop_while_the_model_thinks_and_reads() {
    assert_eq!(DESKTOP_PROTOCOL_VERSION, 6);
    assert_eq!(HOLD_IDLE_MS, 300_000, "a model step can take minutes");
    let keep = HostCommand::Keep { hold: "trn_1".into() };
    let line = serde_json::to_value(&keep).expect("written");
    assert_eq!(line, json!({"op": "keep", "hold": "trn_1"}));
    assert_eq!(serde_json::from_value::<HostCommand>(line).expect("read"), keep);
    assert!(keep.acts_on().is_none() && !keep.takes_foreground(), "keeping is no action");
    for kept in [true, false] {
        let result = HostResult::Kept { kept };
        let line = serde_json::to_value(&result).expect("written");
        assert_eq!(line, json!({"kind": "kept", "kept": kept}));
        assert_eq!(serde_json::from_value::<HostResult>(line).expect("read"), result);
    }
}

#[test]
fn opening_a_program_brings_its_window_to_the_front() {
    let app =
        AppEntry { id: "mspaint".into(), name: "画图".into(), kind: AppKind::Manual, path: None };
    let open = HostCommand::Launch { app, timeout_ms: 15_000 };
    assert!(open.acts_on().is_some() && open.takes_foreground());
}

#[test]
fn a_takeover_tells_the_model_what_the_user_did_when_it_is_known() {
    assert_eq!(
        taken_over_message(Some("点了鼠标")),
        "用户已接管电脑（点了鼠标），用户交还操作权之前不要再操作电脑"
    );
    assert_eq!(taken_over_message(None), "用户已接管电脑，用户交还操作权之前不要再操作电脑");
}
