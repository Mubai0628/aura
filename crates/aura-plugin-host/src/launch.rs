//! What a background service is started as, and the program session that runs it: the same
//! program rules and process tree as an MCP program server, under the plugin's limits.
use aura_backend_windows_user::checked_program::{self, Links};
use aura_backend_windows_user::program::{
    ProgramEnvironment, ProgramSession, SessionEvent, SessionSpec, program_extension,
    resolve_program,
};
use aura_plugins::PluginChannel;
use aura_windows_platform::JobLimits;
use std::{collections::BTreeMap, path::PathBuf, sync::mpsc};
use zeroize::Zeroizing;

/// Everything a service is started with.
#[derive(Clone)]
pub struct ServiceLaunch {
    pub plugin: String,
    /// What it is started as: its trusted content and its settings. A different identity
    /// restarts it.
    pub identity: String,
    /// The program, with the plugin folder filled in, or a program name found on PATH.
    pub command: String,
    pub arguments: Vec<String>,
    /// The service's own folder: where it runs and keeps what it writes.
    pub data_folder: PathBuf,
    /// Aura's own folder for it: TEMP and the script that starts a `.cmd` program.
    pub scratch: PathBuf,
    pub limits: Option<JobLimits>,
    pub channels: Vec<PluginChannel>,
    /// Text settings, given in the first line.
    pub settings: BTreeMap<String, String>,
    /// Secret settings, given as environment variables: name → value.
    pub secrets: Vec<(String, Zeroizing<String>)>,
}

/// A started service: lines go to its standard input; stopping it ends its whole process tree.
pub trait ServiceProcess: Send {
    /// Queues one line. A service that stopped reading is refused, never waited on.
    fn send(&self, line: Vec<u8>) -> anyhow::Result<()>;
    fn stop(&mut self);
}

pub trait Launcher: Send + Sync + 'static {
    /// Starts the service; its output and its end arrive on `events`.
    fn start(
        &self,
        launch: &ServiceLaunch,
        events: mpsc::Sender<SessionEvent>,
    ) -> Result<Box<dyn ServiceProcess>, String>;
}

/// Starts services as program sessions, each in a Job of its own.
pub struct ProgramLauncher;

impl ServiceProcess for ProgramSession {
    fn send(&self, line: Vec<u8>) -> anyhow::Result<()> {
        ProgramSession::send(self, line)
    }

    fn stop(&mut self) {
        ProgramSession::stop(self);
    }
}

fn quoted(value: &str) -> String {
    format!("\"{value}\"")
}

impl Launcher for ProgramLauncher {
    fn start(
        &self,
        launch: &ServiceLaunch,
        events: mpsc::Sender<SessionEvent>,
    ) -> Result<Box<dyn ServiceProcess>, String> {
        let program = resolve_program(&launch.command)?;
        let temp = launch.scratch.join("temp");
        for folder in [&launch.data_folder, &temp] {
            std::fs::create_dir_all(folder)
                .map_err(|error| format!("无法建立文件夹 {}：{error}", folder.display()))?;
        }
        let (binding, held) = checked_program::capture(&program, Links::Allowed)
            .map_err(|error| format!("{} 无法读取：{error}", program.display()))?;
        let mut declared = vec![
            ("AURA_PLUGIN_NAME".to_owned(), launch.plugin.clone()),
            ("AURA_PLUGIN_DATA".to_owned(), launch.data_folder.display().to_string()),
            ("AURA_PLUGIN_PROTOCOL".to_owned(), crate::PROTOCOL_VERSION.to_string()),
            // Python would write compiled files next to the plugin's own modules, and a changed
            // plugin folder is no longer the trusted one.
            ("PYTHONDONTWRITEBYTECODE".to_owned(), "1".to_owned()),
        ];
        declared
            .extend(launch.secrets.iter().map(|(name, value)| (name.clone(), value.to_string())));
        let environment = ProgramEnvironment::current(&declared, true, &temp);
        for (_, value) in &mut declared {
            zeroize::Zeroize::zeroize(value);
        }
        let environment = environment.map_err(|error| format!("环境变量不对：{error}"))?;
        let script = matches!(program_extension(&program).as_deref(), Some("cmd" | "bat"));
        let (image, cmd_script, arguments) = if script {
            // CMD takes no arguments after a script: the call line is Aura's own, every argument
            // quoted, and an argument CMD would read differently is refused.
            if launch.arguments.iter().any(|a| a.contains(['"', '%', '\r', '\n'])) {
                return Err("通过 .cmd/.bat 启动时，参数里不能有 \"、% 或换行".into());
            }
            let mut line = format!("@call {}", quoted(&program.display().to_string()));
            for argument in &launch.arguments {
                line.push(' ');
                line.push_str(&quoted(argument));
            }
            let path = launch.scratch.join("launch.cmd");
            std::fs::write(&path, format!("{line}\r\n"))
                .map_err(|error| format!("无法写启动脚本：{error}"))?;
            let cmd = aura_backend_windows_user::file_binding::capture_trusted_system_cmd()
                .map_err(|error| format!("找不到 Windows 的 cmd.exe：{error}"))?;
            (cmd, Some(path), Vec::new())
        } else {
            (binding, None, launch.arguments.clone())
        };
        let session = ProgramSession::start(
            SessionSpec {
                image: &image,
                cmd_script: cmd_script.as_deref(),
                arguments: &arguments,
                working_directory: &launch.data_folder,
                environment: environment.entries(),
                other_links_allowed: true,
                limits: launch.limits,
            },
            events,
        )
        .map_err(|error| format!("没能启动 {}：{error}", program.display()))?;
        drop(held);
        Ok(Box::new(session))
    }
}
