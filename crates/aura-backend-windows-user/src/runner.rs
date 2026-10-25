//! One authenticated dispatch per short-lived dedicated-account process.
use anyhow::{Result, ensure};
use aura_domain_common::UtcTimestamp;
use aura_windows_user_contract::*;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};

async fn reject(pipe: &mut (impl tokio::io::AsyncWrite + Unpin), reason: &str) -> Result<()> {
    super::wire::write(pipe, &RunnerCoreMessage::Rejected { reason: reason.into() }).await
}

fn same_file(actual: &ExecutionFileBinding, expected: &ExecutionFileBinding) -> bool {
    actual.path == expected.path
        && actual.volume_serial == expected.volume_serial
        && actual.file_id == expected.file_id
}

pub async fn serve(root: &Path, core_pid: u32, pipe_name: &str) -> Result<()> {
    aura_windows_platform::suppress_execution_error_dialogs();
    ensure!(!aura_windows_platform::current_process_is_elevated()?, "runner.elevated_identity");
    // Connect before the identity check so Core's process-start deadline is
    // not spent hashing the published package. Execute is sent only after Ready.
    let core = aura_windows_platform::retain_process(core_pid)?;
    let mut pipe = aura_windows_platform::open_secure_byte_pipe_client(pipe_name)?;
    aura_windows_platform::validate_named_pipe_server(&pipe, core.identity())?;
    let installation = match super::installation::Installation::open_for_runner(root) {
        Ok(value) => value,
        Err(_) => {
            reject(&mut pipe, "runner.installation_unverified").await?;
            return Ok(());
        }
    };
    let (sid, restricted) = match aura_windows_platform::current_user_token_identity() {
        Ok(value) => value,
        Err(_) => {
            reject(&mut pipe, "runner.account_identity").await?;
            return Ok(());
        }
    };
    if restricted
        || !(sid == installation.manifest.offline.sid
            || installation.manifest.online.as_ref().is_some_and(|v| v.sid == sid))
    {
        reject(&mut pipe, "runner.account_identity").await?;
        return Ok(());
    }
    let (image, _held) = match super::file_binding::capture(&std::env::current_exe()?, true) {
        Ok(value) => value,
        Err(_) => {
            reject(&mut pipe, "runner.image_changed").await?;
            return Ok(());
        }
    };
    if image != installation.manifest.runner_image {
        reject(&mut pipe, "runner.image_changed").await?;
        return Ok(());
    }
    let (core_image, _held_core) =
        match super::file_binding::capture(&core.identity().executable_path, false) {
            Ok(value) => value,
            Err(error) => {
                reject(&mut pipe, &classify_core_image_capture(&error)).await?;
                return Ok(());
            }
        };
    let core_sid = aura_windows_platform::process_account_sid(core.identity());
    if let Some(reason) = core_identity_handshake_reject(
        Ok(&core_image),
        core_sid.as_deref(),
        &installation.manifest.core_image,
        &installation.manifest.owner_sid,
    ) {
        reject(&mut pipe, &reason).await?;
        return Ok(());
    }
    super::wire::write(&mut pipe, &RunnerCoreMessage::Ready).await?;
    let message: CoreRunnerMessage =
        tokio::time::timeout(Duration::from_secs(30), super::wire::read(&mut pipe)).await??;
    let CoreRunnerMessage::Execute { envelope } = message else {
        anyhow::bail!("runner.execute_required")
    };
    let preparation = envelope.validate(UtcTimestamp::now())?;
    ensure!(
        preparation.installation_fingerprint == installation.fingerprint
            && preparation.account_sid == sid,
        "runner.preparation_changed"
    );
    let online = preparation.mode == aura_security_contract::PermissionMode::FullAccess
        && envelope.dispatch.operation.content.network
            == aura_execution_contract::OperationNetwork::BroadProcess;
    ensure!(installation.account(online)?.sid == sid, "runner.network_identity_mismatch");
    let attempt = envelope.dispatch.binding.attempt.clone();
    let cancel = Arc::new(AtomicU8::new(0));
    let worker_cancel = cancel.clone();
    let (observations, mut outgoing) = tokio::sync::mpsc::unbounded_channel();
    let (mut reader, mut writer) = tokio::io::split(pipe);
    let (messages, mut incoming) = tokio::sync::mpsc::channel(2);
    let reader_task = tokio::spawn(async move {
        loop {
            let message = super::wire::read::<CoreRunnerMessage>(&mut reader).await;
            let failed = message.is_err();
            if messages.send(message).await.is_err() || failed {
                break;
            }
        }
    });
    let mut worker = tokio::task::spawn_blocking(move || {
        super::runner_execution::execute(
            *envelope,
            preparation,
            installation,
            worker_cancel,
            observations,
        )
    });
    loop {
        tokio::select! {
            result = &mut worker => {
                // Drain ordered Started/Completed/Background frames before the
                // final worker status. A very short script cannot overtake Started.
                while let Ok(message) = outgoing.try_recv() {
                    super::wire::write(&mut writer, &message).await?;
                }
                if let Err(error) = result? {
                    // Never include commands, credentials or paths from an error chain.
                    super::wire::write(
                        &mut writer,
                        &RunnerCoreMessage::Rejected { reason: rejected_reason(&error) },
                    )
                    .await?;
                }
                break;
            }
            Some(message) = outgoing.recv() => {
                if super::wire::write(&mut writer, &message).await.is_err() {
                    cancel.store(2, Ordering::Release);
                    let _ = worker.await;
                    break;
                }
            }
            message = incoming.recv() => {
                match message {
                    Some(Ok(CoreRunnerMessage::Cancel { attempt: target })) if target == attempt => { cancel.store(1, Ordering::Release); }
                    _ => {
                        cancel.store(2, Ordering::Release);
                        let _ = worker.await;
                        break;
                    }
                }
            }
        }
    }
    reader_task.abort();
    Ok(())
}

fn classify_core_image_capture(error: &anyhow::Error) -> String {
    let stage = error.chain().find_map(|cause| {
        let text = cause.to_string();
        if text.contains("runner.capture.inspect") {
            Some("inspect")
        } else if text.contains("runner.capture.open") {
            Some("open")
        } else {
            None
        }
    });
    if let Some(code) = error.chain().find_map(|cause| {
        cause.downcast_ref::<std::io::Error>().and_then(std::io::Error::raw_os_error)
    }) {
        return match stage {
            Some(stage) => format!("runner.core_image_capture.{stage}.os:{code}"),
            None => format!("runner.core_image_capture.os:{code}"),
        };
    }
    for cause in error.chain() {
        let text = cause.to_string();
        if let Some(rest) = text.strip_prefix("runner.") {
            let code = rest.split(['\n', ';', ' ']).next().unwrap_or(rest);
            if matches!(
                code,
                "relative_path"
                    | "reparse_path"
                    | "path_changed"
                    | "aliased_file"
                    | "expected_file"
            ) {
                return format!("runner.core_image_capture.{code}");
            }
        }
    }
    "runner.core_image_capture.unknown".into()
}

fn classify_core_process_sid(error: &std::io::Error) -> String {
    if let Some(code) = error.raw_os_error() {
        return format!("runner.core_process_sid.os:{code}");
    }
    if error.to_string() == "process identity changed" {
        return "runner.core_process_sid.identity_changed".into();
    }
    "runner.core_process_sid.unknown".into()
}

fn core_file_mismatch_reason(
    actual: &ExecutionFileBinding,
    expected: &ExecutionFileBinding,
) -> Option<String> {
    if same_file(actual, expected) {
        return None;
    }
    let mut parts = Vec::new();
    if actual.path != expected.path {
        parts.push("path");
    }
    if actual.volume_serial != expected.volume_serial {
        parts.push("volume");
    }
    if actual.file_id != expected.file_id {
        parts.push("file_id");
    }
    Some(format!("runner.core_file_mismatch.{}", parts.join(".")))
}

pub(crate) fn core_identity_handshake_reject(
    capture: Result<&ExecutionFileBinding, &anyhow::Error>,
    sid: Result<&str, &std::io::Error>,
    expected_image: &ExecutionFileBinding,
    expected_owner: &str,
) -> Option<String> {
    match capture {
        Err(error) => Some(classify_core_image_capture(error)),
        Ok(actual) => match sid {
            Err(error) => Some(classify_core_process_sid(error)),
            Ok(owner) => {
                if let Some(reason) = core_file_mismatch_reason(actual, expected_image) {
                    Some(reason)
                } else if owner != expected_owner {
                    Some("runner.core_owner_mismatch".into())
                } else {
                    None
                }
            }
        },
    }
}

#[cfg(test)]
fn diagnostic_charset_ok(reason: &str) -> bool {
    reason.len() <= 128
        && reason
            .bytes()
            .all(|unit| unit.is_ascii_alphanumeric() || matches!(unit, b'.' | b'_' | b':'))
}

fn rejected_reason(error: &anyhow::Error) -> String {
    if let Some(code) = error.chain().find_map(|cause| {
        cause.downcast_ref::<std::io::Error>().and_then(std::io::Error::raw_os_error).or_else(
            || {
                cause
                    .source()
                    .and_then(|inner| inner.downcast_ref::<std::io::Error>())
                    .and_then(std::io::Error::raw_os_error)
            },
        )
    }) {
        return format!("runner.execution_failed.os:{code}");
    }
    error
        .chain()
        .find_map(|cause| stable_runner_reason(&cause.to_string()))
        .unwrap_or_else(|| "runner.execution_failed".into())
}

fn stable_runner_reason(text: &str) -> Option<String> {
    let first = text.split(['\n', ';']).next()?.trim();
    if first.starts_with("runner.")
        && first.len() <= 96
        && !first.contains("..")
        && first
            .bytes()
            .all(|unit| unit.is_ascii_alphanumeric() || matches!(unit, b'.' | b'_' | b':'))
    {
        Some(first.to_owned())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{
        core_identity_handshake_reject, diagnostic_charset_ok, rejected_reason,
        stable_runner_reason,
    };
    use aura_windows_user_contract::ExecutionFileBinding;

    fn binding(path: &str, volume: &str, file_id: u8) -> ExecutionFileBinding {
        ExecutionFileBinding {
            path: path.into(),
            volume_serial: volume.into(),
            file_id: [file_id; 16],
            sha256: None,
        }
    }

    #[test]
    fn rejected_reason_keeps_stable_runner_codes() {
        assert_eq!(
            rejected_reason(&anyhow::anyhow!("runner.invalid_cmd_image")),
            "runner.invalid_cmd_image"
        );
        let wrapped = anyhow::Error::from(std::io::Error::other("runner.invalid_cmd_image"));
        assert_eq!(rejected_reason(&wrapped), "runner.invalid_cmd_image");
        assert_eq!(
            rejected_reason(&anyhow::Error::from(std::io::Error::from_raw_os_error(5))),
            "runner.execution_failed.os:5"
        );
        assert_eq!(
            rejected_reason(&anyhow::anyhow!(r"failed C:\secret")),
            "runner.execution_failed"
        );
        assert_eq!(
            stable_runner_reason("runner.invalid_cmd_image"),
            Some("runner.invalid_cmd_image".into())
        );
        assert_eq!(stable_runner_reason(r"runner.path C:\Windows"), None);
    }

    #[test]
    fn core_handshake_splits_four_reject_reasons_before_ready() {
        let expected = binding(r"\\?\C:\Aura\aura-core.exe", "1", 1);
        let matching = binding(r"\\?\C:\Aura\aura-core.exe", "1", 1);
        let path_mismatch = binding(r"\\?\C:\Users\Public\aura-core.exe", "1", 1);
        let volume_mismatch = binding(r"\\?\C:\Aura\aura-core.exe", "2", 1);
        let file_mismatch = binding(r"\\?\C:\Aura\aura-core.exe", "1", 9);
        let all_mismatch = binding(r"\\?\D:\other.exe", "9", 7);

        let capture_denied = anyhow::Error::from(std::io::Error::from_raw_os_error(5));
        let capture_alias = anyhow::anyhow!("runner.aliased_file");
        let capture_unknown = anyhow::anyhow!("unexpected capture");
        let sid_denied = std::io::Error::from_raw_os_error(5);
        let sid_changed = std::io::Error::other("process identity changed");
        let sid_unknown = std::io::Error::other("token query failed");

        let capture_reason = core_identity_handshake_reject(
            Err(&capture_denied),
            Err(&sid_denied),
            &expected,
            "S-1-5-21-1",
        )
        .expect("capture fail rejects before SID");
        assert_eq!(capture_reason, "runner.core_image_capture.os:5");
        assert!(diagnostic_charset_ok(&capture_reason));
        assert_ne!(capture_reason, "runner.core_identity");

        let inspect_denied = anyhow::Error::from(std::io::Error::from_raw_os_error(5))
            .context("runner.capture.inspect");
        let inspect_reason = core_identity_handshake_reject(
            Err(&inspect_denied),
            Err(&sid_denied),
            &expected,
            "S-1-5-21-1",
        )
        .expect("inspect fail rejects before SID");
        assert_eq!(inspect_reason, "runner.core_image_capture.inspect.os:5");
        assert!(diagnostic_charset_ok(&inspect_reason));

        let alias_reason = core_identity_handshake_reject(
            Err(&capture_alias),
            Ok("S-1-5-21-1"),
            &expected,
            "S-1-5-21-1",
        )
        .expect("aliased capture");
        assert_eq!(alias_reason, "runner.core_image_capture.aliased_file");
        assert!(diagnostic_charset_ok(&alias_reason));

        let unknown_capture = core_identity_handshake_reject(
            Err(&capture_unknown),
            Ok("S-1-5-21-1"),
            &expected,
            "S-1-5-21-1",
        )
        .expect("unknown capture");
        assert_eq!(unknown_capture, "runner.core_image_capture.unknown");
        assert!(diagnostic_charset_ok(&unknown_capture));

        let sid_reason = core_identity_handshake_reject(
            Ok(&matching),
            Err(&sid_denied),
            &expected,
            "S-1-5-21-1",
        )
        .expect("SID fail after capture");
        assert_eq!(sid_reason, "runner.core_process_sid.os:5");
        assert!(diagnostic_charset_ok(&sid_reason));

        let sid_changed_reason = core_identity_handshake_reject(
            Ok(&matching),
            Err(&sid_changed),
            &expected,
            "S-1-5-21-1",
        )
        .expect("identity changed");
        assert_eq!(sid_changed_reason, "runner.core_process_sid.identity_changed");
        assert!(diagnostic_charset_ok(&sid_changed_reason));

        let sid_unknown_reason = core_identity_handshake_reject(
            Ok(&matching),
            Err(&sid_unknown),
            &expected,
            "S-1-5-21-1",
        )
        .expect("unknown SID");
        assert_eq!(sid_unknown_reason, "runner.core_process_sid.unknown");
        assert!(diagnostic_charset_ok(&sid_unknown_reason));

        let path_reason = core_identity_handshake_reject(
            Ok(&path_mismatch),
            Ok("S-1-5-21-1"),
            &expected,
            "S-1-5-21-1",
        )
        .expect("path mismatch");
        assert_eq!(path_reason, "runner.core_file_mismatch.path");
        assert!(!path_reason.contains('\\'));
        assert!(diagnostic_charset_ok(&path_reason));

        let volume_reason = core_identity_handshake_reject(
            Ok(&volume_mismatch),
            Ok("S-1-5-21-1"),
            &expected,
            "S-1-5-21-1",
        )
        .expect("volume mismatch");
        assert_eq!(volume_reason, "runner.core_file_mismatch.volume");

        let file_reason = core_identity_handshake_reject(
            Ok(&file_mismatch),
            Ok("S-1-5-21-1"),
            &expected,
            "S-1-5-21-1",
        )
        .expect("file_id mismatch");
        assert_eq!(file_reason, "runner.core_file_mismatch.file_id");

        let combined = core_identity_handshake_reject(
            Ok(&all_mismatch),
            Ok("S-1-5-21-1"),
            &expected,
            "S-1-5-21-1",
        )
        .expect("combined mismatch");
        assert_eq!(combined, "runner.core_file_mismatch.path.volume.file_id");
        assert!(diagnostic_charset_ok(&combined));

        let owner_reason = core_identity_handshake_reject(
            Ok(&matching),
            Ok("S-1-5-21-99"),
            &expected,
            "S-1-5-21-1",
        )
        .expect("owner mismatch");
        assert_eq!(owner_reason, "runner.core_owner_mismatch");
        assert!(diagnostic_charset_ok(&owner_reason));

        assert_eq!(
            core_identity_handshake_reject(
                Ok(&matching),
                Ok("S-1-5-21-1"),
                &expected,
                "S-1-5-21-1"
            ),
            None,
            "matching identity may send Ready and must not reject"
        );
    }
}
