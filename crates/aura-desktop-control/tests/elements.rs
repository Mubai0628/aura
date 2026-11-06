//! Reading Aura's test window through UI Automation. The window opens without taking the front
//! and nothing here moves the pointer or presses a key.
mod support;

use aura_desktop_control::host::Desktop;
use aura_desktop_domain::*;
use std::time::{Duration, Instant};
use support::*;

#[test]
fn a_read_lists_the_test_windows_controls_with_their_state_in_its_order() {
    let mut own = OwnFixture::new("read");
    let window = own.open((60, 60));
    let mut desktop = desktop();
    let (read, elements, complete) =
        elements_of(read(&mut desktop, &own.app, None, 500, 32, 8_000));
    assert_eq!(read.window, window.window);
    assert!(complete, "{}", outline(&elements));

    let ok = named(&elements, Role::Button, "确定");
    assert!(ok.enabled && ok.abilities.contains(&Ability::Click), "{ok:?}");
    assert_eq!(ok.rect, own.place("ok"));
    named(&elements, Role::Text, "还没点");
    let check = named(&elements, Role::CheckBox, "选项");
    assert_eq!(check.checked, Some(Checked::Off));
    assert!(check.abilities.contains(&Ability::Toggle));
    let edit = at_place(&elements, own.place("edit"));
    assert_eq!(edit.role, Role::Edit);
    assert!(edit.abilities.contains(&Ability::SetValue), "{edit:?}");
    assert!(edit.value.as_deref().unwrap_or_default().is_empty(), "{edit:?}");
    for name in ["付款", "卡住", "换水果"] {
        assert!(named(&elements, Role::Button, name).enabled);
    }
    assert!(!named(&elements, Role::Button, "不能点").enabled);
    let combo = at_place(&elements, own.place("combo"));
    assert_eq!(combo.role, Role::ComboBox);
    assert_eq!(combo.expanded, Some(false));

    let list = at_place(&elements, own.place("list"));
    assert_eq!(list.role, Role::List);
    let fruits: Vec<&Element> =
        elements.iter().filter(|element| element.role == Role::ListItem).take(3).collect();
    let names: Vec<&str> = fruits.iter().map(|item| item.key.name.as_str()).collect();
    assert_eq!(names, ["苹果", "香蕉", "橙子"]);
    for item in &fruits {
        assert_eq!(item.depth, list.depth + 1);
        assert!(item.abilities.contains(&Ability::Select));
    }
    let position =
        |name: &str| elements.iter().position(|element| element.key.name == name).expect("listed");
    assert!(position("确定") < position("还没点"));
    assert!(position("选项") < position("苹果"));
    assert!(position("苹果") < position("香蕉") && position("香蕉") < position("橙子"));

    // Hidden, scrolled out of view, or the window's own title bar: not on the list.
    for absent in ["看不见", "第 20 项", "关闭", "Close", "最小化", "Minimize"] {
        assert!(
            elements.iter().all(|element| element.key.name != absent),
            "{absent} in\n{}",
            outline(&elements)
        );
    }
    named(&elements, Role::ListItem, "第 1 项");
}

#[test]
fn a_read_one_level_down_marks_what_it_did_not_open_and_a_read_from_there_finishes_it() {
    let mut own = OwnFixture::new("read-depth");
    own.open((80, 80));
    let mut desktop = desktop();
    let (_, top, complete) = elements_of(read(&mut desktop, &own.app, None, 500, 1, 8_000));
    assert!(!complete);
    let list = at_place(&top, own.place("list")).clone();
    assert!(list.more, "{}", outline(&top));
    assert!(top.iter().all(|element| element.role != Role::ListItem), "{}", outline(&top));

    let (_, inside, complete) =
        elements_of(read(&mut desktop, &own.app, Some(list.key), 500, 32, 8_000));
    assert!(complete);
    let names: Vec<(u32, &str)> =
        inside.iter().map(|element| (element.depth, element.key.name.as_str())).collect();
    assert_eq!(names, [(0, "苹果"), (0, "香蕉"), (0, "橙子")]);
}

#[test]
fn a_read_stops_at_its_element_limit_and_says_it_is_not_complete() {
    let mut own = OwnFixture::new("read-limit");
    own.open((100, 100));
    let mut desktop = desktop();
    let (_, elements, complete) = elements_of(read(&mut desktop, &own.app, None, 3, 32, 8_000));
    assert!(!complete);
    assert_eq!(elements.len(), 3, "{}", outline(&elements));
}

#[test]
fn a_read_from_an_element_that_is_gone_is_stale() {
    let mut own = OwnFixture::new("read-stale");
    own.open((120, 120));
    let mut desktop = desktop();
    let mut list = at_place(&read_all(&mut desktop, &own.app), own.place("list")).key.clone();
    list.name = "不是这个名字".into();
    let error = failed(read(&mut desktop, &own.app, Some(list.clone()), 500, 32, 8_000));
    assert_eq!(error.code, HostErrorCode::Stale);
    list.runtime_id = vec![42, 4242, 4242];
    let error = failed(read(&mut desktop, &own.app, Some(list), 500, 32, 8_000));
    assert_eq!(error.code, HostErrorCode::Stale);
}

#[test]
fn a_read_of_a_window_that_stopped_answering_ends_in_time() {
    let mut own = OwnFixture::new("read-hung");
    own.open_with((140, 140), &["--hang-at", "300", "--hang-ms", "20000"]);
    assert!(
        own.wait_for(|events| events.iter().any(|event| event == "hang"), Duration::from_secs(10))
    );
    let mut desktop = desktop();
    let started = Instant::now();
    let error = failed(read(&mut desktop, &own.app, None, 500, 32, 2_000));
    let took = started.elapsed();
    assert_eq!(error.code, HostErrorCode::Timeout, "{error:?}");
    assert_eq!(error.delivery, Delivery::NotDelivered);
    assert!(took < Duration::from_millis(3_500), "{took:?}");
}

#[test]
fn a_read_is_only_of_the_programs_own_windows() {
    let mut mine = OwnFixture::new("read-mine");
    let mut other = OwnFixture::new("read-other");
    mine.open((160, 160));
    let theirs = other.open((420, 160));
    let mut desktop = desktop();
    let error = failed(desktop.run(HostCommand::Elements {
        app: mine.app.clone(),
        window: Some(theirs.window),
        root: None,
        max_elements: 500,
        max_depth: 32,
        timeout_ms: 8_000,
    }));
    assert_eq!(error.code, HostErrorCode::WindowNotFound);
    let error = failed(read(&mut desktop, &mine.app, None, 0, 32, 8_000));
    assert_eq!(error.code, HostErrorCode::InvalidRequest);
}
