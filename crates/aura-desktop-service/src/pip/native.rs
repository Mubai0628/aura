use super::{Opened, PipBackend, StartupEntry};
use crate::{
    ProgramHost,
    child::{ChildHost, ChildOpening, Expected, listen},
};
use aura_desktop_domain::pip::{PIP_SWITCH, PipOptions};
use std::{path::PathBuf, sync::Arc};

pub struct NativePip {
    pub program: PathBuf,
    pub folder: PathBuf,
}
struct Startup(aura_windows_platform::ChildRunOnce);
impl StartupEntry for Startup {
    fn remove(&mut self) -> Result<(), String> {
        self.0.remove().map_err(|error| format!("临时启动项清理失败：{error}"))
    }
}
impl PipBackend for NativePip {
    fn open(&self, child: ChildHost, stop_keys: &str) -> Result<Opened, String> {
        if !aura_windows_platform::child_sessions_enabled() {
            return Err("Windows 未启用子会话，小窗不可用；没有执行".into());
        }
        aura_desktop_domain::pip::stop_chord(stop_keys)?;
        let opening = ChildOpening::new()?;
        let listening = listen(&opening)?;
        let name = format!("AuraChild-{}", opening.pipe().rsplit('-').next().unwrap_or_default());
        let startup = Startup(
            aura_windows_platform::ChildRunOnce::install(
                &name,
                &opening.command_line(&self.program),
            )
            .map_err(|error| format!("没能设置临时启动项：{error}"))?,
        );
        let (events, received) = tokio::sync::mpsc::unbounded_channel();
        let mut arguments = vec![PIP_SWITCH.to_owned()];
        arguments.extend(
            PipOptions { stop_keys: stop_keys.to_owned(), ..PipOptions::default() }.arguments(),
        );
        let window = ProgramHost { program: self.program.clone(), folder: self.folder.clone() }
            .start_arguments(events, &arguments)?;
        let expected = Expected {
            program: self.program.clone(),
            session: Arc::new(aura_windows_platform::child_session_id),
        };
        let login = Box::pin(async move {
            let supervisor = listening.accept(&expected, super::LOGIN_WITHIN).await?;
            let id = supervisor.session();
            child.attach(supervisor);
            Ok(id)
        });
        Ok(Opened { window, events: received, login, startup: Box::new(startup) })
    }
}
