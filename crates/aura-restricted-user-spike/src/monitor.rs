use anyhow::{Result, ensure};
use aura_windows_platform::{
    NativeSecretEnvironmentEntry, RestrictedUserProcess, SecretBytes, spawn_restricted_user_process,
};
use serde_json::{Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};

pub fn launch(
    sid: &str,
    executable: &Path,
    args: &[String],
    root: &Path,
    device_install_id: Option<&str>,
    managed_runtime: Option<&Path>,
) -> Result<RestrictedUserProcess> {
    let environment =
        aura_backend_windows_user::environment::baseline(&root.join("allowed"), managed_runtime)?;
    let secret = NativeSecretEnvironmentEntry::from_utf8_secret(
        "AURA_R06_TRANSIENT".into(),
        SecretBytes::new(b"R06_ENV_CANARY".to_vec()),
    )?;
    Ok(spawn_restricted_user_process(
        sid,
        executable,
        &args.iter().map(|v| v.encode_utf16().collect()).collect::<Vec<_>>(),
        &root.join("allowed"),
        &environment,
        Some(&secret),
        aura_windows_platform::RestrictedUserLaunchOptions { device_install_id, cmd_script: None },
    )?)
}

pub fn collect(process: &mut RestrictedUserProcess, cancel: bool, limit: usize) -> Result<Value> {
    collect_until(process, cancel, limit, Duration::from_secs(10))
}

pub fn collect_until(
    process: &mut RestrictedUserProcess,
    cancel: bool,
    limit: usize,
    timeout: Duration,
) -> Result<Value> {
    let started = Instant::now();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut reason = "exited";
    process.close_stdin();
    loop {
        let remaining = limit.saturating_sub(stdout.len() + stderr.len());
        stdout.extend(process.drain_stdout(remaining + 1)?);
        let remaining = limit.saturating_sub(stdout.len() + stderr.len());
        stderr.extend(process.drain_stderr(remaining + 1)?);
        if stdout.len() + stderr.len() > limit {
            reason = "output_limit";
            process.terminate(91)?;
            break;
        }
        if cancel && decode_diagnostic(&stdout).contains("child_ready") {
            reason = "cancelled";
            process.terminate(92)?;
            break;
        }
        if process.wait_timeout(Duration::from_millis(10))?.is_some()
            && process.active_processes_in_job()? == 0
        {
            break;
        }
        if started.elapsed() > timeout {
            reason = "timeout";
            process.terminate(93)?;
            break;
        }
    }
    ensure!(
        process.wait_for_process_tree_exit(Duration::from_secs(3))?,
        "process tree remains alive"
    );
    let exit = process.wait_timeout(Duration::from_secs(1))?;
    if reason == "exited" {
        let remaining = limit.saturating_sub(stdout.len() + stderr.len());
        stdout.extend(process.drain_stdout(remaining + 1)?);
        stderr.extend(process.drain_stderr(limit.saturating_sub(stdout.len() + stderr.len()) + 1)?);
        ensure!(stdout.len() + stderr.len() <= limit, "late output exceeds limit");
    }
    // Fixed probes emit UTF-8 (PowerShell) or UTF-16LE (CMD /u).
    // This diagnostic heuristic is not the production mixed-stream codec.
    let payload = serde_json::from_str::<Value>(&decode_diagnostic(&stdout)).ok();
    Ok(
        json!({"termination":reason,"exit_code":exit,"stdout_bytes":stdout.len(),"stderr_bytes":stderr.len(),"tree_empty":true,"payload":payload,"stderr_excerpt":diagnostic(&stderr)}),
    )
}

pub(crate) fn diagnostic(bytes: &[u8]) -> String {
    // Fixed development fixtures only; redact before truncation so a canary
    // straddling the display boundary cannot leak a prefix into the report.
    let text = decode_diagnostic(bytes)
        .replace("R06_ENV_CANARY", "[redacted]")
        .replace("R06_STDIN_CANARY", "[redacted]");
    text.chars().take(4096).collect()
}

fn decode_diagnostic(bytes: &[u8]) -> String {
    // Windows PowerShell startup errors use UTF-16LE even without a BOM.
    // Infer that encoding only from a substantial ASCII/NUL pattern. An
    // explicit BOM also supports diagnostics containing no ASCII characters.
    let pairs = bytes.chunks_exact(2);
    let ascii_le = pairs.clone().filter(|v| v[1] == 0 && v[0].is_ascii_graphic()).count();
    let bom_le = bytes.starts_with(&[0xff, 0xfe]);
    let bom_be = bytes.starts_with(&[0xfe, 0xff]);
    if bom_le || bom_be || (ascii_le >= 4 && ascii_le * 2 >= pairs.len()) {
        let body = if bom_le || bom_be { &bytes[2..] } else { bytes };
        let words = body
            .chunks_exact(2)
            .map(|v| {
                if bom_be {
                    u16::from_be_bytes([v[0], v[1]])
                } else {
                    u16::from_le_bytes([v[0], v[1]])
                }
            })
            .collect::<Vec<_>>();
        let mut decoded = String::from_utf16_lossy(&words);
        if body.len() % 2 != 0 {
            decoded.push('\u{fffd}');
        }
        decoded
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_redact_before_applying_display_bound() {
        let text = format!("{}R06_ENV_CANARY R06_STDIN_CANARY", "x".repeat(4090));
        let result = diagnostic(text.as_bytes());
        assert_eq!(result.chars().count(), 4096);
        assert!(!result.contains("R06_"));
        assert_eq!(diagnostic(b"R06_ENV_CANARY R06_STDIN_CANARY"), "[redacted] [redacted]");
    }

    #[test]
    fn windows_startup_diagnostics_decode_and_redact_utf16() {
        let message = "启动 CLR 失败，返回 HRESULT 80070005。\r\n";
        let encode = |s: &str| s.encode_utf16().flat_map(u16::to_le_bytes).collect::<Vec<_>>();
        assert_eq!(diagnostic(&encode(message)), message);
        assert_eq!(diagnostic(&encode("R06_ENV_CANARY R06_STDIN_CANARY")), "[redacted] [redacted]");
        let mut bom = vec![0xff, 0xfe];
        bom.extend(encode("拒绝访问"));
        assert_eq!(diagnostic(&bom), "拒绝访问");
        assert_eq!(diagnostic("UTF-8 拒绝访问".as_bytes()), "UTF-8 拒绝访问");
        let long = encode(&format!("{}R06_ENV_CANARY", "x".repeat(4090)));
        assert!(!diagnostic(&long).contains("R06_"));
    }
}
