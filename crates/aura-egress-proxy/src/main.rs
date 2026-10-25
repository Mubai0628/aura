#![forbid(unsafe_code)]

mod runtime;
mod runtime_commands;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    runtime::run().await
}
