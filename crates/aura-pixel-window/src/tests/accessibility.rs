//! What a screen reader finds in a window through UI Automation, and pressing a button that way.
use std::{thread, time::Duration};

use super::{install_page, one_window_at_a_time};
use crate::{
    page::Action,
    testing::{Part, invoke, parts, wait_for_window},
    window::PixelWindow,
};

fn part<'a>(parts: &'a [Part], automation_id: &str) -> &'a Part {
    parts
        .iter()
        .find(|part| part.automation_id == automation_id)
        .unwrap_or_else(|| panic!("{automation_id} in {parts:#?}"))
}

#[test]
fn every_part_has_its_name_role_and_state_and_buttons_can_be_invoked() {
    let _one = one_window_at_a_time();
    let thread = thread::spawn(|| {
        let mut page = install_page();
        let mut window = PixelWindow::open(&page).expect("the window opens");
        window.show(&mut page).expect("the page runs")
    });
    let window = wait_for_window(Duration::from_secs(10)).expect("the window appears");
    let found = parts(window).expect("UI Automation reads the window");

    let ids: Vec<&str> = found.iter().map(|part| part.automation_id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "mark-0",
            "mark-1",
            "mark-2",
            "heading",
            "field-0",
            "button-change",
            "check-shortcut",
            "link-licenses",
            "button-cancel",
            "button-install",
            "close"
        ],
        "reading order: the steps, the heading, the page, the footer, the close button"
    );
    let heading = part(&found, "heading");
    assert_eq!((heading.role, heading.name.as_str()), ("text", "安装 Aura 0.3.0"));
    let mark = part(&found, "mark-0");
    assert_eq!((mark.name.as_str(), mark.status.as_str()), ("位置", "进行中"));
    let field = part(&found, "field-0");
    assert_eq!((field.role, field.name.as_str()), ("edit", "安装位置"));
    assert_eq!(field.value.as_deref(), Some(r"C:\Users\14153\AppData\Local\Programs\Aura"));
    assert_eq!(part(&found, "button-change").name, "更改安装位置");
    let check = part(&found, "check-shortcut");
    assert_eq!(
        (check.role, check.name.as_str(), check.checked),
        ("check box", "在桌面创建快捷方式", Some(false))
    );
    assert_eq!(
        (part(&found, "link-licenses").role, part(&found, "link-licenses").name.as_str()),
        ("link", "许可协议与第三方声明")
    );
    for (id, name) in [("button-cancel", "取消"), ("button-install", "安装"), ("close", "关闭")]
    {
        let button = part(&found, id);
        assert_eq!((button.role, button.name.as_str(), button.enabled), ("button", name, true));
    }

    invoke(window, "button-install").expect("the button can be invoked");
    assert_eq!(thread.join().expect("ends"), Action::Button("install"));
}
