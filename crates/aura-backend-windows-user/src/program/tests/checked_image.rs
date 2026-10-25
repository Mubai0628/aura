//! A package's or a hook's program, the user's own, is read in full the first
//! time it runs, then only compared while it stays as it was; one that failed to start or crashed
//! is checked in full again and leaves a record.
use super::{super::*, support::*};
use std::{path::Path, sync::atomic::AtomicBool, time::Duration};

fn run_image(
    image: &aura_windows_user_contract::ExecutionFileBinding,
    root: &Path,
    arguments: &[String],
) -> ProgramOutput {
    let environment = environment(root, false);
    run(
        ProgramSpec {
            image,
            arguments,
            working_directory: root,
            environment: environment.entries(),
            input: ProgramInput::Closed,
            cancelled: &AtomicBool::new(false),
            timeout: Duration::from_secs(20),
            max_output_bytes: 65_536,
            limits: None,
        },
        &mut |_: &[u8]| Ok(vec![]),
        &mut |_| {},
    )
    .expect("creation-time user Job")
}

#[test]
fn a_program_is_read_in_full_once_then_only_compared() {
    let root = tempfile::tempdir().expect("temporary directory");
    // A program of this test's own, so no other test shares its record.
    let program = root.path().join("hostname.exe");
    let system = aura_windows_platform::windows_directory().expect("Windows directory");
    std::fs::copy(system.join("System32").join("hostname.exe"), &program).expect("copied");
    let (image, _) = crate::file_binding::capture(&program, true).expect("bound");
    for _ in 0..2 {
        let output = run_image(&image, root.path(), &[]);
        assert_eq!((output.reason, output.exit_code), (ProgramStop::Exited, Some(0)), "{output:?}");
    }
    assert!(crate::checked_program::is_unchanged(&image), "held as checked");
    assert_eq!(crate::checked_program::full_checks(&image), 1, "read in full once");
}

#[test]
fn a_program_that_crashed_is_checked_again_and_says_so() {
    let root = tempfile::tempdir().expect("temporary directory");
    let image = command_image();
    // 0xC0000005, the status a crashed program ends with.
    let arguments = ["/d", "/c", "exit -1073741819"].map(String::from);
    let output = run_image(&image, root.path(), &arguments);
    assert_eq!(output.exit_code, Some(0xC000_0005), "{output:?}");
    assert!(
        crate::checked_program::logged_for_tests().iter().any(|(event, fields)| {
            event == "program.checked_after_failure"
                && fields["program"] == image.path
                && fields["failure"] == "exited_abnormally"
        }),
        "a record of the check after the crash"
    );
}
