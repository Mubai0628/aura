//! What a configured server runs as: the command resolved on the user's PATH, its content
//! hash, the environment and, for a `.cmd`/`.bat` program, the script CMD runs. The trusted
//! fingerprint covers exactly these facts.
use aura_backend_windows_user::program::ProgramEnvironment;
use aura_mcp::{
    McpServerConfig, McpTransport, ResolvedCommand, config_fingerprint, validate_server,
};
use aura_windows_user_contract::ExecutionFileBinding;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};

const PROGRAM_EXTENSIONS: [&str; 4] = ["exe", "com", "cmd", "bat"];

/// A configuration checked against the computer now.
#[derive(Clone, Debug)]
pub struct Checked {
    pub fingerprint: Option<String>,
    pub resolved: Option<ResolvedCommand>,
    pub problems: Vec<String>,
}

/// A file as last seen: its path, length and modification time.
type FileIdentity = (PathBuf, u64, Option<SystemTime>);

/// Hashes already read, by file identity, so listing servers does not read every program.
#[derive(Default)]
pub(crate) struct HashCache(Mutex<HashMap<FileIdentity, String>>);

fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 256 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

impl HashCache {
    fn hash(&self, path: &Path) -> std::io::Result<String> {
        let metadata = std::fs::metadata(path)?;
        let key = (path.to_path_buf(), metadata.len(), metadata.modified().ok());
        if let Ok(cache) = self.0.lock()
            && let Some(hash) = cache.get(&key)
        {
            return Ok(hash.clone());
        }
        let hash = sha256_file(path)?;
        if let Ok(mut cache) = self.0.lock() {
            if cache.len() > 256 {
                cache.clear();
            }
            cache.insert(key, hash.clone());
        }
        Ok(hash)
    }
}

fn user_path() -> Option<OsString> {
    aura_windows_platform::current_user_environment()
        .ok()?
        .into_iter()
        .find(|(name, _)| name.to_string_lossy().eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| value)
}

fn program_extension(path: &Path) -> Option<String> {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .filter(|extension| PROGRAM_EXTENSIONS.contains(&extension.as_str()))
}

/// The program a command names: an absolute path, or a bare name found on the user's PATH
/// (PATHEXT order for the four program kinds). The current folder is never searched.
pub(crate) fn resolve_program(command: &str) -> Result<PathBuf, String> {
    let named = Path::new(command.trim());
    let usable = |path: &Path| {
        path.is_file().then(|| path.to_path_buf()).filter(|path| program_extension(path).is_some())
    };
    if named.is_absolute() {
        return usable(named).ok_or_else(|| {
            format!("{} 不存在，或不是 .exe、.com、.cmd、.bat 程序", named.display())
        });
    }
    if command.contains(['\\', '/']) {
        return Err("command 必须是完整路径，或 PATH 里的程序名".into());
    }
    let candidates: Vec<String> = if named.extension().is_some() {
        vec![command.trim().to_owned()]
    } else {
        ["exe", "com", "bat", "cmd"].iter().map(|ext| format!("{}.{ext}", command.trim())).collect()
    };
    let path = user_path().unwrap_or_default();
    for directory in std::env::split_paths(&path).filter(|d| d.is_absolute()) {
        for candidate in &candidates {
            if let Some(found) = usable(&directory.join(candidate)) {
                return Ok(found);
            }
        }
    }
    Err(format!("在 PATH 里找不到 {}（只认 .exe、.com、.cmd、.bat）", command.trim()))
}

/// Checks a configuration and computes what trusting it would mean. Nothing is started.
pub(crate) fn check(server: &McpServerConfig, hashes: &HashCache) -> Checked {
    let mut problems = validate_server(server);
    let resolved = match &server.transport {
        // A Streamable HTTP server is reached only through Core's network gateway; an address
        // in the cloud metadata range is never one.
        McpTransport::Http { url, .. } => {
            let metadata = url::Url::parse(url).ok().is_some_and(|url| {
                aura_network_control::McpHttpRequestGrant::destination_classes(&url).is_ok_and(
                    |classes| {
                        classes.contains(&aura_security_contract::NetworkDestinationClass::Metadata)
                    },
                )
            });
            if metadata {
                problems.push("这个地址属于云主机的元数据服务，不能作为 MCP 服务器".into());
            }
            None
        }
        McpTransport::Stdio { command, arguments, .. } => match resolve_program(command) {
            Err(problem) => {
                problems.push(problem);
                None
            }
            Ok(path) => {
                let script = matches!(program_extension(&path).as_deref(), Some("cmd" | "bat"));
                if script && arguments.iter().any(|a| a.contains(['"', '%', '\r', '\n'])) {
                    problems.push("通过 .cmd/.bat 启动时，参数里不能有 \"、% 或换行".into());
                }
                match hashes.hash(&path) {
                    Ok(sha256) => {
                        Some(ResolvedCommand { path: path.display().to_string(), sha256 })
                    }
                    Err(error) => {
                        problems.push(format!("{} 无法读取：{error}", path.display()));
                        None
                    }
                }
            }
        },
    };
    let resolvable = matches!(server.transport, McpTransport::Http { .. }) || resolved.is_some();
    let fingerprint = (problems.is_empty() && resolvable)
        .then(|| config_fingerprint(&server.transport, resolved.as_ref()));
    Checked { fingerprint, resolved, problems }
}

/// Everything a stdio session needs, built only for a trusted, unchanged configuration.
pub(crate) struct StdioPlan {
    pub image: ExecutionFileBinding,
    pub cmd_script: Option<PathBuf>,
    pub arguments: Vec<String>,
    pub working_directory: PathBuf,
    pub environment: ProgramEnvironment,
    /// A plugin's limits for the whole tree; none for the user's own servers.
    pub limits: Option<aura_windows_platform::JobLimits>,
    /// Held until the session starts, so the checked file is the started file.
    pub _held: Box<dyn std::any::Any + Send>,
}

fn quoted(value: &str) -> String {
    format!("\"{value}\"")
}

/// Builds the launch for a server whose current fingerprint matched its trust. `scratch` is
/// the server's own folder under Aura's data root (TEMP and the CMD wrapper live there);
/// `secrets` are its secret environment variables, set after the declared ones; `limits` are
/// a plugin server's.
pub(crate) fn stdio_plan(
    server: &McpServerConfig,
    resolved: &ResolvedCommand,
    scratch: &Path,
    secrets: &[(String, zeroize::Zeroizing<String>)],
    limits: Option<aura_windows_platform::JobLimits>,
) -> anyhow::Result<StdioPlan> {
    let McpTransport::Stdio { arguments, working_directory, environment, .. } = &server.transport
    else {
        anyhow::bail!("mcp.not_stdio");
    };
    std::fs::create_dir_all(scratch)?;
    let program = PathBuf::from(&resolved.path);
    // Other hard links are allowed: the file stays held against writes
    // until the session has started. It is the user's own program: read in full the first time,
    // and again only after it changed or a launch of it failed.
    let (binding, held) = aura_backend_windows_user::checked_program::capture(
        &program,
        aura_backend_windows_user::checked_program::Links::Allowed,
    )?;
    anyhow::ensure!(
        binding.sha256.as_ref().is_some_and(|hash| hash.hex.eq_ignore_ascii_case(&resolved.sha256)),
        "mcp.config_changed: 服务器程序在信任之后变了，需要在设置里重新信任"
    );
    let mut declared: Vec<(String, String)> =
        environment.iter().map(|entry| (entry.name.clone(), entry.value.clone())).collect();
    declared.extend(secrets.iter().map(|(name, value)| (name.clone(), value.to_string())));
    let environment = ProgramEnvironment::current(&declared, true, scratch);
    for (_, value) in &mut declared {
        zeroize::Zeroize::zeroize(value);
    }
    let environment = environment?;
    let working_directory =
        working_directory.as_ref().map(PathBuf::from).unwrap_or_else(|| scratch.to_path_buf());
    if matches!(program_extension(&program).as_deref(), Some("cmd" | "bat")) {
        // CMD takes no arguments after a script: the call line is Core-written, with every
        // argument quoted; `"`, `%` and line breaks were refused when the server was checked.
        let script = scratch.join("launch.cmd");
        let mut line = format!("@call {}", quoted(&resolved.path));
        for argument in arguments {
            line.push(' ');
            line.push_str(&quoted(argument));
        }
        std::fs::write(&script, format!("{line}\r\n"))?;
        let cmd = aura_backend_windows_user::file_binding::capture_trusted_system_cmd()?;
        return Ok(StdioPlan {
            image: cmd,
            cmd_script: Some(script),
            arguments: Vec::new(),
            working_directory,
            environment,
            limits,
            _held: Box::new(held),
        });
    }
    Ok(StdioPlan {
        image: binding,
        cmd_script: None,
        arguments: arguments.clone(),
        working_directory,
        environment,
        limits,
        _held: Box::new(held),
    })
}
