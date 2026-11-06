//! The program a configured command names: an absolute path, or a bare name found on the
//! user's PATH. Only the four kinds Windows starts directly are programs here.
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

const PROGRAM_EXTENSIONS: [&str; 4] = ["exe", "com", "cmd", "bat"];

fn user_path() -> Option<OsString> {
    aura_windows_platform::current_user_environment()
        .ok()?
        .into_iter()
        .find(|(name, _)| name.to_string_lossy().eq_ignore_ascii_case("PATH"))
        .map(|(_, value)| value)
}

/// The program's kind (`exe`, `com`, `cmd` or `bat`), lowercase; none for anything else.
#[must_use]
pub fn program_extension(path: &Path) -> Option<String> {
    path.extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .filter(|extension| PROGRAM_EXTENSIONS.contains(&extension.as_str()))
}

/// The program a command names: an absolute path, or a bare name found on the user's PATH
/// (PATHEXT order for the four program kinds). The current folder is never searched.
pub fn resolve_program(command: &str) -> Result<PathBuf, String> {
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
