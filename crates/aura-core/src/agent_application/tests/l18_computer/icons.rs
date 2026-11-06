//! The programs' icons for the composer's list, its program blocks and the approval card: read
//! through the host for the catalog's programs only, once each, and none while computer control
//! is off.
use super::*;

#[tokio::test]
async fn icons_are_read_once_for_the_catalogs_programs_and_never_while_control_is_off() {
    let desktop = Desktop::new(Scenario::Text, PermissionMode::WorkspaceReadWrite, as_is).await;
    let asked = json!({"app_ids": [NOTEPAD, r"C:\Windows\System32\cmd.exe"]});
    let success = crate::r03_tests::success;
    let off = success(&desktop.runtime, "computer.icons", asked.clone()).await;
    assert_eq!(off, json!({"icons": []}));
    assert!(desktop.played.sent().is_empty(), "off: the host is not asked");

    switch(&desktop.runtime, true).await;
    let on = success(&desktop.runtime, "computer.icons", asked.clone()).await;
    assert_eq!(
        on,
        json!({"icons": [{"app_id": NOTEPAD, "png": PNG_BASE64}]}),
        "a path the catalog does not have is not read"
    );
    assert_eq!(success(&desktop.runtime, "computer.icons", asked).await, on);
    let read: Vec<(Vec<String>, u32)> = desktop
        .played
        .sent()
        .into_iter()
        .filter_map(|command| match command {
            HostCommand::Icons { apps, size } => {
                Some((apps.into_iter().map(|app| app.id).collect(), size))
            }
            _ => None,
        })
        .collect();
    assert_eq!(read, [(vec![NOTEPAD.to_owned()], aura_desktop_service::ICON_SIZE)], "read once");
}
