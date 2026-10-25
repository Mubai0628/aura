use aura_backend_windows_user::compat::{
    CompatBackend, TerminalKind, TerminalOptions, TerminalSession,
};
use aura_windows_platform::TerminalSize;
use aura_windows_user_contract::ManagedShell;
use std::{
    collections::BTreeMap,
    ops::Deref,
    path::Path,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

pub(super) static SERIAL: Mutex<()> = Mutex::new(());
pub(super) struct Backend {
    raw: CompatBackend,
    _root: tempfile::TempDir,
}
impl Deref for Backend {
    type Target = CompatBackend;
    fn deref(&self) -> &Self::Target {
        &self.raw
    }
}
pub(super) fn backend() -> Backend {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/managed-runtimes/powershell-7.6.5-win-x64");
    let environment_root = tempfile::tempdir().expect("isolated environment");
    let environment = aura_backend_windows_user::environment::baseline(
        environment_root.path(),
        Some(&root.join("payload")),
    )
    .expect("test-owned environment")
    .into_iter()
    .map(|entry| {
        (entry.name.into(), String::from_utf16(&entry.value_utf16).expect("fixture UTF16").into())
    })
    .collect();
    Backend {
        raw: CompatBackend::with_environment(
            &root.join("payload"),
            &root.join("manifest.json"),
            environment,
        )
        .expect("verified managed PS7 / system CMD"),
        _root: environment_root,
    }
}
pub(super) fn options(shell: ManagedShell, root: &Path) -> TerminalOptions {
    TerminalOptions {
        shell,
        kind: TerminalKind::Agent { requires_network: false },
        working_directory: root.to_path_buf(),
        size: TerminalSize { columns: 120, rows: 30 },
        output_limit: 16 * 1024 * 1024,
    }
}
pub(super) fn line(terminal: &TerminalSession, command: &str) {
    terminal.write(format!("{command}\r").as_bytes()).expect("interactive input");
}
pub(super) fn until(terminal: &TerminalSession, marker: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(15);
    static ANSWERED: OnceLock<Mutex<BTreeMap<(u32, u64), usize>>> = OnceLock::new();
    let identity = terminal.identity();
    let key = (identity.process_id, identity.creation_time_100ns);
    loop {
        let bytes = terminal.read(0, 16 * 1024 * 1024).expect("raw VT output").bytes;
        let text = strip_vt(&bytes);
        // The fixture acts as the terminal display; PSReadLine asks for cursor position.
        let raw = String::from_utf8_lossy(&bytes);
        let queries = raw.matches("\x1b[6n").count();
        let mut map = ANSWERED
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let answered = map.get(&key).copied().unwrap_or(0);
        for _ in answered..queries {
            terminal.write(b"\x1b[1;1R").expect("DSR answer");
        }
        map.insert(key, queries);
        drop(map);
        if text.contains(marker) {
            return text;
        }
        assert!(
            Instant::now() < deadline,
            "missing {marker}; end={:?}; raw_bytes={}; text={text}",
            terminal.end().expect("state"),
            bytes.len()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
pub(super) fn strip_vt(bytes: &[u8]) -> String {
    let mut text = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == 27 {
            index += 1;
            match bytes.get(index) {
                Some(b'[') => {
                    index += 1;
                    while index < bytes.len() {
                        let byte = bytes[index];
                        index += 1;
                        if (0x40..=0x7e).contains(&byte) {
                            break;
                        }
                    }
                }
                Some(b']') => {
                    index += 1;
                    while index < bytes.len() {
                        if bytes[index] == 7 {
                            index += 1;
                            break;
                        }
                        if bytes[index] == 27 && bytes.get(index + 1) == Some(&b'\\') {
                            index += 2;
                            break;
                        }
                        index += 1;
                    }
                }
                Some(_) => index += 1,
                None => {}
            }
        } else {
            text.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&text).into_owned()
}
pub(super) fn marker(terminal: &TerminalSession, shell: ManagedShell, suffix: &str) -> String {
    match shell {
        ManagedShell::PowerShell => {
            line(terminal, &format!("[Console]::WriteLine('L02-' + '{suffix}')"))
        }
        ManagedShell::Cmd => {
            line(terminal, &format!("set _L02_MARK={suffix}"));
            line(terminal, "echo L02-%_L02_MARK%");
        }
    }
    until(terminal, &format!("L02-{suffix}"))
}
pub(super) fn await_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(Instant::now() < deadline, "missing {}", path.display());
        std::thread::sleep(Duration::from_millis(10));
    }
}
