//! Chrome starts this program for Aura's extension with the caller's origin as its first argument.
use aura_chrome_host::relay::{RelayTarget, relay, target_beside};
use std::process::ExitCode;

fn target() -> anyhow::Result<RelayTarget> {
    #[cfg(feature = "test-server")]
    if let (Some(pipe), Some(server)) = (
        std::env::var_os("AURA_CHROME_HOST_TEST_PIPE"),
        std::env::var_os("AURA_CHROME_HOST_TEST_SERVER"),
    ) {
        return Ok(RelayTarget {
            pipe_name: pipe.to_string_lossy().into_owned(),
            expected_server: server.into(),
        });
    }
    Ok(target_beside(&std::fs::canonicalize(std::env::current_exe()?)?))
}

fn main() -> ExitCode {
    let origin = std::env::args().nth(1).unwrap_or_default();
    let result = target().and_then(|target| {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
        let result =
            runtime.block_on(relay(&origin, &target, tokio::io::stdin(), tokio::io::stdout()));
        // Chrome's input is read on a thread whose read cannot be cancelled. Waiting for it would
        // keep this program running after Core has gone, holding Chrome's connection and the files
        // of its version until Chrome closes.
        runtime.shutdown_background();
        result
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Only the error's own codes: a frame's content never reaches stderr.
            eprintln!("aura-chrome-host: {error:#}");
            ExitCode::FAILURE
        }
    }
}
