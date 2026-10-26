#![forbid(unsafe_code)]
//! Formal Runner bootstrap. Diagnostic and host mutation commands are absent.
//! UNWIRED: not in the release layout (scripts/release/layout.mjs); only the development setup
//! builds it, so released builds have no restricted-user backend.
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let [mode, root, core, pipe] = args.as_slice() else {
        anyhow::bail!("runner.authenticated_core_required")
    };
    anyhow::ensure!(
        mode == "--core" && pipe.starts_with(r"\\.\pipe\Aura.Execution."),
        "runner.invalid_bootstrap"
    );
    aura_backend_windows_user::runner::serve(std::path::Path::new(root), core.parse()?, pipe).await
}
