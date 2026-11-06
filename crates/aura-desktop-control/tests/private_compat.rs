//! Owner-authorized private applications: read structure only and print aggregate counts.
//! Window titles, names, values and serialized element trees must never enter test output.
use aura_desktop_control::desktop::RealDesktop;
use aura_desktop_control::host::Desktop;
use aura_desktop_domain::*;

fn observe(chrome: bool) -> Result<(), String> {
    let mut desktop = RealDesktop::new().map_err(|_| "COM initialization failed")?;
    let apps = match desktop.run(HostCommand::Catalog { manual: vec![] }) {
        HostOutcome::Done(HostResult::Catalog { apps }) => apps,
        _ => return Err("catalog unavailable".into()),
    };
    let app = apps
        .into_iter()
        .find(|app| {
            let path =
                app.path.as_deref().unwrap_or(&app.id).replace('/', "\\").to_ascii_lowercase();
            if chrome {
                path.ends_with("\\chrome.exe") || app.name == "Google Chrome"
            } else {
                path.ends_with("\\weixin.exe")
                    || path.ends_with("\\wechat.exe")
                    || app.name == "微信"
            }
        })
        .ok_or("application not in catalog; no private content read")?;
    let windows = match desktop.run(HostCommand::Windows { apps: vec![app.clone()] }) {
        HostOutcome::Done(HostResult::Windows { windows }) => windows
            .into_iter()
            .filter(|w| w.app_id.as_deref() == Some(&app.id) && !w.minimized)
            .collect::<Vec<_>>(),
        _ => return Err("window listing unavailable".into()),
    };
    if windows.is_empty() {
        return Err("no visible window; no private content read".into());
    }
    let mut all_read = true;
    for (index, window) in windows.into_iter().enumerate() {
        match desktop.run(HostCommand::Elements {
            app: app.clone(),
            window: Some(window.window),
            root: None,
            max_elements: 2_000,
            max_depth: 32,
            timeout_ms: 10_000,
        }) {
            HostOutcome::Done(HostResult::Elements { elements, complete, .. }) => {
                let actionable = elements.iter().filter(|e| !e.abilities.is_empty()).count();
                println!(
                    "private-compat app={} window={} elements={} complete={} declared_actionable={} actual_actions=0",
                    if chrome { "Chrome" } else { "WeChat" },
                    index + 1,
                    elements.len(),
                    complete,
                    actionable
                );
            }
            HostOutcome::Failed(error) => {
                println!(
                    "private-compat app={} window={} error_code={:?} actual_actions=0",
                    if chrome { "Chrome" } else { "WeChat" },
                    index + 1,
                    error.code
                );
                all_read = false;
            }
            _ => {
                all_read = false;
            }
        }
    }
    if all_read {
        Ok(())
    } else {
        Err("some windows could not be read; see aggregate status".into())
    }
}

#[test]
#[ignore = "reads private webpage structure; owner idle for 10 minutes, desktop unlocked"]
fn chrome_private_compatibility_records_counts_without_content() -> Result<(), String> {
    observe(true)
}

#[test]
#[ignore = "reads private chat structure; owner idle for 10 minutes, desktop unlocked; no messages"]
fn wechat_private_compatibility_records_counts_without_content() -> Result<(), String> {
    observe(false)
}
