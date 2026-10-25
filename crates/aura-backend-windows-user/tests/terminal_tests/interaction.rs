use super::support::*;
use aura_backend_windows_user::compat::TerminalEndReason;
use aura_windows_platform::{NativeTerminal, TerminalSize};
use aura_windows_user_contract::ManagedShell;

#[test]
fn real_ps7_and_cmd_echo_unicode_clear_resize_and_exit() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    let backend = backend();
    for shell in [ManagedShell::PowerShell, ManagedShell::Cmd] {
        let root = tempfile::tempdir().expect("isolated terminal");
        std::fs::write(root.path().join("unicode.txt"), "中文 宽字符 界 😀\n").expect("test data");
        let terminal = backend
            .launch_terminal(&options(shell, root.path()), Some("http://127.0.0.1:9"))
            .expect("real ConPTY");
        marker(&terminal, shell, "READY");
        match shell {
            ManagedShell::PowerShell => line(
                &terminal,
                "Get-Content -Encoding utf8 unicode.txt; Clear-Host; Get-Content -Encoding utf8 unicode.txt",
            ),
            ManagedShell::Cmd => line(&terminal, "type unicode.txt & cls & type unicode.txt"),
        }
        let text = until(&terminal, "中文 宽字符 界 😀");
        assert!(text.contains("中文 宽字符 界 😀"));
        assert!(
            terminal.read(0, 1024 * 1024).expect("raw VT").bytes.contains(&27),
            "ConPTY returns a VT stream"
        );
        for index in 0..50 {
            assert!(
                terminal
                    .resize(TerminalSize {
                        columns: if index % 2 == 0 { 48 } else { 160 },
                        rows: 30
                    })
                    .expect("resize")
            );
        }
        assert!(!terminal.resize(TerminalSize { columns: 160, rows: 30 }).expect("same size"));
        let size_file = root.path().join("size.txt");
        let measure = format!(
            "[IO.File]::WriteAllText('{}', [Console]::WindowWidth.ToString() + ',' + [Console]::WindowHeight.ToString())",
            size_file.display().to_string().replace('\'', "''")
        );
        if shell == ManagedShell::PowerShell {
            line(&terminal, &measure);
        } else {
            line(
                &terminal,
                &format!(
                    "\"{}\" -MTA -NoLogo -NoProfile -NonInteractive -Command \"{measure}\"",
                    aura_windows_platform::dos_drive_path(backend.powershell_path())
                        .expect("CMD executable spelling")
                        .display()
                ),
            );
        }
        let output = marker(&terminal, shell, "RESIZED");
        assert!(size_file.exists(), "dimension command must succeed for {shell:?}: {output}");
        assert_eq!(
            std::fs::read_to_string(root.path().join("size.txt"))
                .expect("actual console dimensions"),
            "160,30"
        );
        line(&terminal, if shell == ManagedShell::Cmd { "exit /b 17" } else { "exit 17" });
        let end = terminal.wait_ended(std::time::Duration::from_secs(10)).expect("natural exit");
        assert_eq!(end.reason, TerminalEndReason::Exited);
        assert_eq!(end.exit_code, Some(17));
        assert!(end.stop_confirmed);
        assert!(terminal.write(b"ignored\r").is_err());
    }
}

#[test]
fn real_ctrl_c_interrupts_both_shells_without_closing_the_terminal() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    interrupt_both_shells(&backend());
}

/// Restores normal Ctrl+C processing for the rest of this test process.
struct RestoreCtrlC;
impl Drop for RestoreCtrlC {
    fn drop(&mut self) {
        let _ = NativeTerminal::inherit_ignored_ctrl_c_for_test(false);
    }
}

#[test]
fn ctrl_c_still_reaches_programs_when_core_inherited_an_ignored_ctrl_c() {
    let _serial = SERIAL.lock().unwrap_or_else(|error| error.into_inner());
    // A Core started in a new process group, or by a parent that ignores Ctrl+C, carries that
    // flag, and Windows passes it on to every program the terminal starts.
    NativeTerminal::inherit_ignored_ctrl_c_for_test(true)
        .expect("ignore Ctrl+C like such a parent");
    let _restore = RestoreCtrlC;
    interrupt_both_shells(&backend());
}

fn interrupt_both_shells(backend: &Backend) {
    for shell in [ManagedShell::PowerShell, ManagedShell::Cmd] {
        let root = tempfile::tempdir().expect("isolated interrupt");
        let terminal = backend
            .launch_terminal(&options(shell, root.path()), Some("http://127.0.0.1:9"))
            .expect("ConPTY");
        marker(&terminal, shell, "BEFORE");
        match shell {
            ManagedShell::PowerShell => line(
                &terminal,
                &format!(
                    "[IO.File]::WriteAllText('{}','ready'); Start-Sleep -Seconds 60; [IO.File]::WriteAllText('{}','bad')",
                    root.path().join("sleep.started").display().to_string().replace('\'', "''"),
                    root.path().join("sleep.finished").display().to_string().replace('\'', "''")
                ),
            ),
            ManagedShell::Cmd => {
                line(&terminal, "echo ready>sleep.started & timeout /t 60 /nobreak >nul")
            }
        }
        await_file(&root.path().join("sleep.started"));
        std::thread::sleep(std::time::Duration::from_millis(100));
        let offset = terminal.read(0, 1024 * 1024).expect("before interrupt").next_offset;
        terminal.interrupt().expect("Ctrl+C");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let page = terminal.read(offset, 1024 * 1024).expect("interrupt output");
            let output = strip_vt(&page.bytes);
            if output.trim_end().ends_with('>') {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Ctrl+C must return to prompt for {shell:?}: {output}"
            );
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        marker(&terminal, shell, "AFTER-INTERRUPT");
        assert!(!root.path().join("sleep.finished").exists());
        let end = terminal.close(TerminalEndReason::UserClosed).expect("close");
        assert!(end.stop_confirmed);
    }
}
