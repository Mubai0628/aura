use super::super::*;
use aura_windows_user_contract::ExecutionFileBinding;
use std::{ffi::OsString, path::Path, sync::atomic::AtomicBool, time::Duration};

pub(super) fn command_image() -> ExecutionFileBinding {
    crate::file_binding::capture_trusted_system_cmd().expect("verified Windows CMD")
}
pub(super) fn environment(root: &Path, ordinary_network: bool) -> ProgramEnvironment {
    let system = aura_windows_platform::windows_directory().expect("Windows directory");
    let block = [
        ("SystemRoot".into(), system.as_os_str().to_owned()),
        ("WINDIR".into(), system.as_os_str().to_owned()),
        ("PATH".into(), system.join("System32").into_os_string()),
        ("USERPROFILE".into(), root.as_os_str().to_owned()),
        ("APPDATA".into(), root.as_os_str().to_owned()),
        ("LOCALAPPDATA".into(), root.as_os_str().to_owned()),
        ("TEST_SECRET".into(), OsString::from("temporary-test-value-never-inherited")),
    ];
    ProgramEnvironment::from_block(block, &[], ordinary_network, root)
        .expect("only synthetic environment")
}
pub(super) fn command(
    root: &Path,
    script: &str,
    cancel: &AtomicBool,
    timeout: Duration,
    limit: usize,
    input: ProgramInput<'_>,
    io: &mut dyn ProgramIo,
) -> ProgramOutput {
    let image = command_image();
    let environment = environment(root, false);
    let args = vec!["/d".into(), "/c".into(), script.into()];
    run(
        ProgramSpec {
            image: &image,
            arguments: &args,
            working_directory: root,
            environment: environment.entries(),
            input,
            cancelled: cancel,
            timeout,
            max_output_bytes: limit,
            limits: None,
        },
        io,
        &mut |_| {},
    )
    .expect("creation-time user Job")
}
