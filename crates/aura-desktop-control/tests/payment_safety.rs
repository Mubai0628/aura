mod support;

use aura_desktop_control::host::Desktop;
use aura_desktop_domain::*;
use support::*;

#[test]
fn payment_buttons_cannot_be_invoked() {
    let mut fixture = OwnFixture::new("payment-safety");
    let window = fixture.open((70, 70));
    let mut desktop = desktop();
    let elements = read_all(&mut desktop, &fixture.app);
    for label in ["付款", "转账"] {
        let target = named(&elements, Role::Button, label);
        let command = HostCommand::Act {
            app: fixture.app.clone(),
            window: window.window.clone(),
            steps: vec![ActStep::Click { element: target.key.clone() }],
            timeout_ms: 8_000,
        };
        let inspected =
            done(desktop.run(HostCommand::Inspect { command: Box::new(command.clone()) }));
        assert_eq!(inspected, HostResult::Inspected { safety: "payment".into() });
        let error = failed(desktop.run(command));
        assert_eq!(error.delivery, Delivery::NotDelivered);
        assert_eq!(error.code, HostErrorCode::Refused);
        assert!(error.message.contains("没有执行"));
    }
    assert!(
        !fixture
            .since_ready()
            .iter()
            .any(|event| event == "click pay" || event == "click transfer")
    );
}

#[test]
fn neutral_controls_inside_a_payment_dialog_are_refused_without_hiding_reads() {
    let mut fixture = OwnFixture::new("payment-dialog");
    let window = fixture.open_with((90, 90), &["--title", "确认付款"]);
    let mut desktop = desktop();
    let elements = read_all(&mut desktop, &fixture.app);
    let target = named(&elements, Role::Button, "确定");
    let error = failed(act(
        &mut desktop,
        &fixture.app,
        &window.window,
        vec![ActStep::Click { element: target.key.clone() }],
    ));
    assert_eq!(error.code, HostErrorCode::Refused);
    assert_eq!(error.delivery, Delivery::NotDelivered);
    assert!(fixture.since_ready().is_empty());
}

#[test]
#[ignore = "takes the foreground; owner must be idle and desktop unlocked"]
fn payment_buttons_cannot_be_clicked_by_coordinates() {
    let mut fixture = OwnFixture::new("payment-coordinates");
    let window = fixture.open((70, 70));
    let mut desktop = desktop();
    let handle = window_handle(&window.window).expect("handle");
    assert!(aura_desktop_control::native::focus::bring_to_front(handle));
    for name in ["pay", "transfer"] {
        let at = center(fixture.place(name));
        let error = failed(desktop.run(HostCommand::Input {
            app: fixture.app.clone(),
            steps: vec![InputStep::Click { at, button: MouseButton::Left, count: 1 }],
            window: None,
            system_keys: false,
            clipboard: false,
        }));
        assert_eq!(error.delivery, Delivery::NotDelivered);
        assert_eq!(error.code, HostErrorCode::Refused);
    }
    assert!(fixture.since_ready().is_empty());
}
