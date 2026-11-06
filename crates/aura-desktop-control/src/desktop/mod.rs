//! The host's requests carried out on the user's real desktop.
mod act;
mod apps;
mod icons;
mod input;
mod read;
mod safety;
mod screenshot;
mod target;
mod window;
pub mod worker;

pub use apps::{app_windows, windows_of};
pub use icons::parsing_name;
pub use screenshot::{Shot, shot};

use crate::host::Desktop;
use crate::native;
use aura_desktop_domain::{
    Delivery, HostCommand, HostError, HostErrorCode, HostOutcome, HostResult,
};
use std::time::Duration;

pub struct RealDesktop {
    _com: native::Com,
    output: Option<crate::host::Output>,
    request_id: u64,
}

impl RealDesktop {
    pub fn new() -> anyhow::Result<Self> {
        crate::own::remember(crate::own::starters(
            std::process::id(),
            &native::process::processes(),
        ));
        Ok(Self { _com: native::Com::init()?, output: None, request_id: 0 })
    }

    pub fn with_output(mut self, output: crate::host::Output) -> Self {
        self.output = Some(output);
        self
    }
}

impl Desktop for RealDesktop {
    fn request(&mut self, request: aura_desktop_domain::HostRequest) -> HostOutcome {
        self.request_id = request.id;
        self.run(request.command)
    }

    fn run(&mut self, command: HostCommand) -> HostOutcome {
        let (command, hold, stop_keys) = match command {
            HostCommand::Guarded { command, hold, stop_keys } => (*command, hold, stop_keys),
            command => (command, None, None),
        };
        // Every action keeps the desktop while it runs; one of a hold, between the hold's
        // commands too.
        let app = match &command {
            HostCommand::Input { app, .. }
            | HostCommand::Act { app, .. }
            | HostCommand::Window { app, .. }
            | HostCommand::Launch { app, .. } => Some(app),
            _ => None,
        };
        let _guard = match app
            .map(|app| {
                native::guard::Lease::begin(
                    app,
                    hold.as_deref(),
                    self.output
                        .as_ref()
                        .map(|output| (std::sync::Arc::clone(output), self.request_id)),
                    stop_keys.as_deref(),
                )
            })
            .transpose()
        {
            Ok(guard) => guard,
            Err(error) => return HostOutcome::Failed(error),
        };
        let outcome = match command {
            HostCommand::Guarded { .. } => Err(internal("不能嵌套守护命令".into())),
            HostCommand::Release { hold } => {
                native::guard::release(&hold);
                Ok(HostResult::Released)
            }
            HostCommand::Keep { hold } => Ok(HostResult::Kept { kept: native::guard::keep(&hold) }),
            HostCommand::Inspect { command } => safety::inspect(&command),
            HostCommand::Catalog { manual } => native::apps::start_menu()
                .map(|items| HostResult::Catalog { apps: crate::catalog::catalog(items, &manual) })
                .map_err(|error| internal(format!("读不到开始菜单：{error}"))),
            HostCommand::Windows { apps } => {
                Ok(HostResult::Windows { windows: app_windows(&apps) })
            }
            HostCommand::Launch { app, timeout_ms } => {
                apps::launch(&app, Duration::from_millis(u64::from(timeout_ms)))
            }
            HostCommand::Displays => {
                Ok(HostResult::Displays { monitors: native::screen::monitors() })
            }
            HostCommand::Icons { apps, size } => icons::icons(&apps, size),
            HostCommand::Screenshot { show, paint, max_long_edge, region, marks } => {
                screenshot::take(&show, paint, max_long_edge, region, &marks)
            }
            HostCommand::Input { app, steps, window, system_keys, clipboard } => input::run(
                &app,
                &steps,
                window.as_ref(),
                crate::plan::Allowed { system_keys, clipboard },
            ),
            HostCommand::Elements { app, window, root, max_elements, max_depth, timeout_ms } => {
                read::read(
                    &app,
                    window.as_deref(),
                    read::Read { root, max_elements, max_depth, timeout_ms },
                )
            }
            HostCommand::Act { app, window, steps, timeout_ms } => {
                act::act(&app, &window, steps, timeout_ms)
            }
            HostCommand::Window { app, window, action } => {
                window::window(&app, window.as_deref(), action)
            }
        };
        match outcome {
            Ok(result) => HostOutcome::Done(result),
            Err(error) => HostOutcome::Failed(error),
        }
    }
}

fn internal(message: String) -> HostError {
    HostError::new(HostErrorCode::Internal, message, Delivery::NotDelivered)
}

/// Refuses while the computer is locked or a UAC prompt shows: nothing of the user's desktop
/// can be seen or reached then.
fn user_desktop() -> Result<(), HostError> {
    match native::screen::input_desktop() {
        Some(name) if name.eq_ignore_ascii_case("Default") => Ok(()),
        other => Err(HostError::new(
            HostErrorCode::DesktopLocked,
            format!(
                "电脑锁着或者正显示管理员确认（现在接收输入的桌面是 {}），等用户回来再操作",
                other.unwrap_or_else(|| "读不到的桌面".into())
            ),
            Delivery::NotDelivered,
        )),
    }
}
