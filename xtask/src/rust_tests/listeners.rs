//! Test programs that listen on a real network address. Windows Defender Firewall asks the user
//! about every new program path that does so, and a test program's file name changes whenever its
//! dependencies change; these run from a hard link with a fixed name beside the program, so the
//! question comes once for each target folder.
use std::{
    fs, io,
    path::{Path, PathBuf},
};

use super::artifacts::{Kind, Program};

/// The test programs whose tests listen beyond loopback: the Core's own tests start an HTTP
/// server on the computer's LAN address.
const LISTENERS: &[(&str, Kind)] = &[("aura-core", Kind::Lib)];

/// The path to start `program` from: a listener's fixed-name link, made again for this build,
/// or the program itself (also when the link cannot be made, with a warning).
pub(super) fn launch_path(program: &Program) -> PathBuf {
    if !is_listener(&program.package, program.kind) {
        return program.executable.clone();
    }
    let link = fixed_name(&program.executable);
    match relink(&program.executable, &link) {
        Ok(()) => link,
        Err(error) => {
            eprintln!(
                "warning: {} runs under its own name, so Windows Firewall may ask about it: \
                 {} could not be made ({error})",
                program.executable.display(),
                link.display()
            );
            program.executable.clone()
        }
    }
}

pub(super) fn is_listener(package: &str, kind: Kind) -> bool {
    LISTENERS.iter().any(|(listener, listener_kind)| *listener == package && *listener_kind == kind)
}

/// `aura_core-0123456789abcdef.exe` becomes `aura_core-tests.exe` in the same folder, so what the
/// tests find beside themselves does not change.
pub(super) fn fixed_name(executable: &Path) -> PathBuf {
    let stem = executable.file_stem().and_then(|stem| stem.to_str()).unwrap_or_default();
    let name = stem.rsplit_once('-').map_or(stem, |(name, _)| name);
    executable.with_file_name(format!("{name}-tests.exe"))
}

/// The link is made again every run: cargo writes a rebuilt program as a new file, and the old
/// link would keep the old one.
fn relink(executable: &Path, link: &Path) -> io::Result<()> {
    match fs::remove_file(link) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::hard_link(executable, link)
}
