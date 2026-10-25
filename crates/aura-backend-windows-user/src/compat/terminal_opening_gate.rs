//! Compiled only for isolated terminal E2E. It delays user opening; it never launches a child.
use anyhow::{Context, Result, ensure};
use aura_domain_common::SessionId;
use std::{path::Path, time::Duration};
use tokio::sync::watch;

const CONTROL_ENV: &str = "AURA_TERMINAL_TEST_OPENING_GATE";
const MAX_HOLD: Duration = Duration::from_secs(30);

pub(super) async fn wait(session: SessionId, cancelled: watch::Receiver<bool>) -> Result<()> {
    let Some(directory) = std::env::var_os(CONTROL_ENV) else { return Ok(()) };
    wait_at(Path::new(&directory), session, cancelled).await
}

async fn wait_at(
    directory: &Path,
    session: SessionId,
    mut cancelled: watch::Receiver<bool>,
) -> Result<()> {
    tokio::select! {
        biased;
        _ = cancelled.wait_for(|value| *value) => anyhow::bail!("terminal.open_cancelled"),
        result = hold(directory, session) => result,
    }
}

async fn hold(directory: &Path, session: SessionId) -> Result<()> {
    ensure!(directory.is_absolute(), "terminal.test_gate_absolute_directory_required");
    let directory = tokio::fs::canonicalize(directory)
        .await
        .context("terminal.test_gate_directory_unavailable")?;
    let temporary = tokio::fs::canonicalize(std::env::temp_dir()).await?;
    ensure!(
        directory.starts_with(&temporary)
            && directory
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.starts_with("aura-terminal-opening-")),
        "terminal.test_gate_must_be_an_owned_temporary_directory"
    );
    if !tokio::fs::try_exists(directory.join("hold")).await? {
        return Ok(());
    }
    // Only fixture-owned metadata is written. The driver owns cleanup, including
    // cancelled holds, so cancellation never removes a later opening's marker.
    tokio::fs::write(directory.join("entered"), session.to_string()).await?;
    tokio::time::timeout(MAX_HOLD, async {
        loop {
            if tokio::fs::try_exists(directory.join("release")).await? {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("terminal.test_gate_timeout"))?
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn directory() -> tempfile::TempDir {
        tempfile::Builder::new().prefix("aura-terminal-opening-").tempdir().unwrap()
    }

    #[tokio::test]
    async fn a_directory_without_a_hold_preserves_the_normal_path() {
        let directory = directory();
        hold(directory.path(), SessionId::new()).await.unwrap();
        assert!(!directory.path().join("entered").exists());
    }

    #[tokio::test]
    async fn the_owned_file_gate_can_be_cancelled_then_released_for_the_next_open() {
        let directory = directory();
        tokio::fs::write(directory.path().join("hold"), b"").await.unwrap();
        let first = SessionId::new();
        let first_identity = first.to_string();
        let (cancel, cancelled) = watch::channel(false);
        let waiting = wait_at(directory.path(), first, cancelled);
        tokio::pin!(waiting);
        tokio::select! {
            result = &mut waiting => panic!("held opening completed before release: {result:?}"),
            entered = tokio::time::timeout(Duration::from_secs(2), async {
                while tokio::fs::read_to_string(directory.path().join("entered")).await.ok().as_deref() != Some(first_identity.as_str()) {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }) => entered.unwrap(),
        }
        assert_eq!(
            tokio::fs::read_to_string(directory.path().join("entered")).await.unwrap(),
            first.to_string()
        );
        cancel.send_replace(true);
        assert_eq!(waiting.await.unwrap_err().to_string(), "terminal.open_cancelled");
        assert!(
            directory.path().join("hold").exists(),
            "the driver keeps ownership of its fixture"
        );
        tokio::fs::write(directory.path().join("release"), b"").await.unwrap();
        let next = SessionId::new();
        tokio::time::timeout(Duration::from_secs(1), hold(directory.path(), next))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            tokio::fs::read_to_string(directory.path().join("entered")).await.unwrap(),
            next.to_string()
        );
    }
}
