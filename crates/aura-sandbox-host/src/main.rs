#![forbid(unsafe_code)]

mod runtime;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    runtime::run().await
}
