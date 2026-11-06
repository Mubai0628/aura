mod support;

use aura_desktop_control::host::Desktop;
use aura_desktop_domain::*;
use support::*;

#[test]
fn password_values_stay_in_host_and_password_writes_are_never_delivered() {
    let mut fixture = OwnFixture::new("password-safety");
    let window = fixture.open_with((80, 80), &["--password", "fixture-password-opaque"]);
    let mut desktop = desktop();
    let result = read(&mut desktop, &fixture.app, None, 500, 32, 8_000);
    let serialized = serde_json::to_string(&result).expect("JSON");
    assert!(!serialized.contains("fixture-password-opaque"));
    let (_, elements, _) = elements_of(result);
    let password = elements.iter().find(|element| element.password).expect("password field");
    assert!(password.value.is_none());
    assert!(!password.abilities.contains(&Ability::SetValue));
    assert!(
        fixture.events().iter().any(|event| event == "seeded-password 23"),
        "prefilled fixture"
    );
    let checked = done(desktop.run(HostCommand::Inspect {
        command: Box::new(HostCommand::Act {
            app: fixture.app.clone(),
            window: window.window.clone(),
            steps: vec![ActStep::SetValue {
                element: password.key.clone(),
                value: "blocked".into(),
            }],
            timeout_ms: 8_000,
        }),
    }));
    assert_eq!(checked, HostResult::Inspected { safety: "password".into() });
    let error = failed(act(
        &mut desktop,
        &fixture.app,
        &window.window,
        vec![ActStep::SetValue { element: password.key.clone(), value: "must-not-arrive".into() }],
    ));
    assert_eq!(error.code, HostErrorCode::Refused);
    assert_eq!(error.delivery, Delivery::NotDelivered);
    assert_eq!(error.steps_done, Some(0));
    assert!(error.message.contains("密码要用户自己输入"));
    assert!(!fixture.since_ready().iter().any(|event| event.starts_with("password ")));
}

#[test]
#[ignore = "takes the foreground and sends input; owner must be idle and desktop unlocked"]
fn typing_deleting_and_pasting_into_password_focus_are_refused_per_step() {
    let mut fixture = OwnFixture::new("password-input");
    let window = fixture.open((80, 80));
    let mut desktop = desktop();
    let password =
        read_all(&mut desktop, &fixture.app).into_iter().find(|e| e.password).expect("password");
    acted(act(
        &mut desktop,
        &fixture.app,
        &window.window,
        vec![ActStep::Focus { element: password.key }],
    ));
    for step in [
        InputStep::Type { text: "must-not-arrive".into() },
        InputStep::Press { keys: "Backspace".into(), repeat: 1 },
        InputStep::Press { keys: "Control+v".into(), repeat: 1 },
        InputStep::Press { keys: "Shift+Insert".into(), repeat: 1 },
    ] {
        let error = failed(desktop.run(HostCommand::Input {
            app: fixture.app.clone(),
            steps: vec![step],
            window: None,
            system_keys: false,
            clipboard: false,
        }));
        assert_eq!(error.code, HostErrorCode::Refused);
        assert_eq!(error.delivery, Delivery::NotDelivered);
        assert!(error.message.contains("密码要用户自己输入"));
    }
    assert!(!fixture.since_ready().iter().any(|event| event.starts_with("password ")));
}
