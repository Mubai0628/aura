//! Elements of a read and the steps that act on them, as Core and the host write and read them.
use aura_desktop_domain::*;
use serde_json::json;

fn key(name: &str) -> ElementKey {
    ElementKey {
        runtime_id: vec![42, 1312, 4, 7],
        control_type: 50_000,
        automation_id: String::new(),
        name: name.into(),
    }
}

#[test]
fn control_types_are_read_in_ui_automations_numbering() {
    assert_eq!(Role::of_control_type(50_000), Role::Button);
    assert_eq!(Role::of_control_type(50_004), Role::Edit);
    assert_eq!(Role::of_control_type(50_020), Role::Text);
    assert_eq!(Role::of_control_type(50_033), Role::Pane);
    assert_eq!(Role::of_control_type(50_040), Role::AppBar);
    for unknown in [0, 49_999, 50_041, i32::MIN, i32::MAX] {
        assert_eq!(Role::of_control_type(unknown), Role::Other, "{unknown}");
    }
}

#[test]
fn a_read_and_an_act_read_back_as_written() {
    let app = AppEntry {
        id: r"C:\Tools\fixture.exe".into(),
        name: "测试窗口".into(),
        kind: AppKind::Manual,
        path: Some(r"C:\Tools\fixture.exe".into()),
    };
    let read = HostRequest {
        id: 3,
        command: HostCommand::Elements {
            app: app.clone(),
            window: Some("w1312".into()),
            root: Some(key("列表")),
            max_elements: 300,
            max_depth: 32,
            timeout_ms: 8_000,
        },
    };
    let line = serde_json::to_value(&read).expect("written");
    assert_eq!(line["command"]["op"], "elements");
    assert_eq!(
        line["command"]["root"],
        json!({"runtime_id": [42, 1312, 4, 7], "control_type": 50000, "name": "列表"})
    );
    assert_eq!(serde_json::from_value::<HostRequest>(line).expect("read"), read);

    let act = HostRequest {
        id: 4,
        command: HostCommand::Act {
            app,
            window: "w1312".into(),
            steps: vec![
                ActStep::SetValue { element: key(""), value: "你好".into() },
                ActStep::Scroll { element: key("列表"), down: 3, right: 0 },
            ],
            timeout_ms: 8_000,
        },
    };
    let line = serde_json::to_value(&act).expect("written");
    assert_eq!(line["command"]["steps"][0]["action"], "set_value");
    assert_eq!(line["command"]["steps"][1]["action"], "scroll");
    assert_eq!(serde_json::from_value::<HostRequest>(line).expect("read"), act);
    let without_amounts = json!({"action": "scroll", "element": {"runtime_id": [1], "control_type": 50008, "name": ""}});
    assert_eq!(
        serde_json::from_value::<ActStep>(without_amounts).expect("read"),
        ActStep::Scroll {
            element: ElementKey {
                runtime_id: vec![1],
                control_type: 50_008,
                automation_id: String::new(),
                name: String::new()
            },
            down: 0,
            right: 0
        }
    );
}

#[test]
fn an_element_writes_only_what_it_has() {
    let element = Element {
        key: key("确定"),
        role: Role::Button,
        depth: 0,
        rect: Rect { x: 10, y: 20, width: 120, height: 32 },
        value: None,
        enabled: true,
        focused: false,
        password: false,
        checked: None,
        expanded: None,
        selected: None,
        abilities: vec![Ability::Click, Ability::Focus],
        more: false,
    };
    let line = serde_json::to_value(&element).expect("written");
    assert_eq!(
        line,
        json!({"key": {"runtime_id": [42, 1312, 4, 7], "control_type": 50000, "name": "确定"},
            "role": "button", "depth": 0, "rect": {"x": 10, "y": 20, "width": 120, "height": 32},
            "enabled": true, "abilities": ["click", "focus"]})
    );
    assert_eq!(serde_json::from_value::<Element>(line).expect("read"), element);
}

#[test]
fn text_is_cut_by_characters_not_bytes() {
    assert_eq!(clip("苹果香蕉", 2), "苹果");
    assert_eq!(clip("苹果", 2), "苹果");
    assert_eq!(clip("", 3), "");
    assert_eq!(clip("abc", 0), "");
}

#[test]
fn every_step_names_its_element() {
    let steps = [
        ActStep::Click { element: key("a") },
        ActStep::Check { element: key("b") },
        ActStep::Focus { element: key("c") },
        ActStep::Scroll { element: key("d"), down: 1, right: 0 },
    ];
    let names: Vec<&str> = steps.iter().map(|step| step.element().name.as_str()).collect();
    assert_eq!(names, ["a", "b", "c", "d"]);
}
